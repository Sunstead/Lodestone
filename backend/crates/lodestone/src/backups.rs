//! World backups: `tar.zst` archives of a world's universe dir.
//!
//! While a server runs the world, Lodestone pauses autosave over RCON
//! (`save-off`, `save-all flush`) so the archive is consistent, then
//! turns it back on. Restores stop the attached instance, swap the world
//! directory atomically and start it again.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::app::AppState;
use crate::error::{AppError, AppResult};
use crate::model::{
    Backup, BackupIndex, BackupKind, BackupSchedule, BackupStatus, BackupStorage, JobKind, ResourceKind, ServerState,
    UpdateScheduleRequest,
};
use crate::supervisor;
use crate::util::{dir_size, new_id, now_iso};

fn archive_path(app: &AppState, backup: &Backup) -> PathBuf {
    app.paths.backups(&backup.world_id).join(format!("{}.tar.zst", backup.id))
}

pub async fn index(app: &AppState, world_id: &str) -> AppResult<BackupIndex> {
    app.store.get_world(world_id).await?;
    let backups = app.store.list_backups(Some(world_id)).await?;
    let schedule = app.store.get_schedule(world_id).await?;
    let dir = app.paths.backups(world_id);
    let used = tokio::task::spawn_blocking(move || dir_size(&dir)).await.unwrap_or(0);
    let host = app.host.latest();
    Ok(BackupIndex {
        backups,
        schedule,
        storage: BackupStorage {
            path: app.paths.backups(world_id).display().to_string(),
            used_bytes: used,
            quota_bytes: host.disk_total_bytes,
        },
    })
}

/// Starts a backup and returns its `running` record immediately.
pub async fn create(app: &AppState, world_id: &str, kind: BackupKind) -> AppResult<Backup> {
    let world = app.store.get_world(world_id).await?;
    let stamp = Utc::now().format("%Y-%m-%d %H:%M");
    let backup = Backup {
        id: new_id("backup"),
        world_id: world.id.clone(),
        name: format!("{} · {stamp}", world.name),
        kind,
        created_at: now_iso(),
        size_bytes: 0,
        status: BackupStatus::Running,
        instance_id: world.attached_instance_id.clone(),
        note: None,
    };
    app.store.put_backup(&backup).await?;
    app.events.changed(ResourceKind::Backups, Some(world.id.clone()));

    let app = app.clone();
    let running = backup.clone();
    tokio::spawn(async move {
        let job = match app.jobs.create(JobKind::Backup, &running.world_id, format!("Backing up {}", world.name)).await
        {
            Ok(job) => job,
            Err(err) => {
                tracing::error!(error = %err, "could not create backup job");
                return;
            }
        };
        let mut done = running.clone();
        match write_archive(&app, &running).await {
            Ok(size) => {
                done.status = BackupStatus::Complete;
                done.size_bytes = size;
                job.succeed(format!("Backed up {}", world.name)).await;
            }
            Err(err) => {
                done.status = BackupStatus::Failed;
                done.note = Some(err.to_string());
                job.fail(err.to_string()).await;
                let _ = tokio::fs::remove_file(archive_path(&app, &running)).await;
            }
        }
        let _ = app.store.put_backup(&done).await;
        if done.kind == BackupKind::Scheduled {
            let _ = prune(&app, &done.world_id).await;
        }
        app.events.changed(ResourceKind::Backups, Some(done.world_id.clone()));
    });
    Ok(backup)
}

async fn write_archive(app: &AppState, backup: &Backup) -> AppResult<u64> {
    let running_instance =
        backup.instance_id.as_ref().filter(|id| app.instances.slot(id).state() == ServerState::Running).cloned();
    if let Some(id) = &running_instance {
        let slot = app.instances.slot(id);
        supervisor::rcon(&slot, "save-off").await?;
        supervisor::rcon(&slot, "save-all flush").await?;
    }
    let source = app.paths.world(&backup.world_id);
    let target = archive_path(app, backup);
    let result = tokio::task::spawn_blocking(move || pack(&source, &target)).await.map_err(anyhow::Error::from)?;
    if let Some(id) = &running_instance {
        let _ = supervisor::rcon(&app.instances.slot(id), "save-on").await;
    }
    Ok(result?)
}

/// Writes `source` (a world universe dir) to a zstd-compressed tarball.
pub fn pack(source: &Path, target: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(source)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = target.with_extension("partial");
    {
        let file = std::fs::File::create(&partial)?;
        let encoder = zstd::Encoder::new(file, 3)?.auto_finish();
        let mut builder = tar::Builder::new(encoder);
        builder.follow_symlinks(false);
        builder.append_dir_all(".", source)?;
        builder.into_inner()?;
    }
    std::fs::rename(&partial, target)?;
    Ok(std::fs::metadata(target)?.len())
}

/// Replaces `target` with the contents of `archive`, atomically.
pub fn unpack_replace(archive: &Path, target: &Path) -> std::io::Result<()> {
    let staging = target.with_extension("restoring");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let decoder = zstd::Decoder::new(std::fs::File::open(archive)?)?;
    tar::Archive::new(decoder).unpack(&staging)?;
    let replaced = target.with_extension(format!("replaced-{}", Utc::now().timestamp()));
    if target.exists() {
        std::fs::rename(target, &replaced)?;
    }
    std::fs::rename(&staging, target)?;
    let _ = std::fs::remove_dir_all(&replaced);
    Ok(())
}

pub async fn delete(app: &AppState, world_id: &str, backup_id: &str) -> AppResult<()> {
    let backup = app.store.get_backup(backup_id).await?;
    if backup.world_id != world_id {
        return Err(AppError::not_found("backup", backup_id));
    }
    if backup.status == BackupStatus::Running {
        return Err(AppError::conflict("backup_running", "This backup is still being written."));
    }
    let _ = tokio::fs::remove_file(archive_path(app, &backup)).await;
    app.store.delete_backup(backup_id).await?;
    app.events.changed(ResourceKind::Backups, Some(world_id.to_owned()));
    Ok(())
}

/// Restores a backup in the background (stop → swap → start).
pub async fn restore(app: &AppState, world_id: &str, backup_id: &str) -> AppResult<()> {
    let backup = app.store.get_backup(backup_id).await?;
    if backup.world_id != world_id {
        return Err(AppError::not_found("backup", backup_id));
    }
    if backup.status != BackupStatus::Complete {
        return Err(AppError::conflict("backup_incomplete", "Only complete backups can be restored."));
    }
    let world = app.store.get_world(world_id).await?;
    let job = app.jobs.create(JobKind::Restore, world_id, format!("Restoring {}", backup.name)).await?;
    let app = app.clone();
    tokio::spawn(async move {
        let instance = world.attached_instance_id.clone();
        let was_active = instance.as_ref().is_some_and(|id| app.instances.slot(id).state().is_active());
        if let (true, Some(id)) = (was_active, &instance) {
            job.progress(None, "Stopping the server").await;
            if let Err(err) = supervisor::stop_and_wait(&app, id).await {
                job.fail(format!("Could not stop the server: {err}")).await;
                return;
            }
        }
        job.progress(None, "Replacing world files").await;
        let archive = archive_path(&app, &backup);
        let target = app.paths.world(&backup.world_id);
        let swapped = tokio::task::spawn_blocking(move || unpack_replace(&archive, &target)).await;
        match swapped {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return job.fail(format!("Restore failed: {err}")).await,
            Err(err) => return job.fail(format!("Restore failed: {err}")).await,
        }
        supervisor::refresh_world_size(&app, &backup.world_id).await;
        if let (true, Some(id)) = (was_active, &instance) {
            job.progress(None, "Starting the server").await;
            if let Err(err) = supervisor::start(&app, id).await {
                return job.fail(format!("Restored, but the server did not start: {err}")).await;
            }
        }
        job.succeed(format!("Restored {}", backup.name)).await;
        app.events.changed(ResourceKind::Worlds, Some(backup.world_id.clone()));
    });
    Ok(())
}

pub async fn patch_schedule(app: &AppState, world_id: &str, patch: UpdateScheduleRequest) -> AppResult<BackupIndex> {
    app.store.get_world(world_id).await?;
    let mut schedule = app.store.get_schedule(world_id).await?;
    if let Some(minutes) = patch.interval_minutes {
        if minutes < 5 {
            return Err(AppError::bad_request("Back up at most every 5 minutes."));
        }
        schedule.interval_minutes = minutes;
    }
    if let Some(retention) = patch.retention {
        schedule.retention = retention.max(1);
    }
    if let Some(enabled) = patch.enabled {
        schedule.enabled = enabled;
    }
    schedule.next_run_at = schedule.enabled.then(|| next_run(&schedule, Utc::now()).to_rfc3339());
    app.store.put_schedule(world_id, &schedule).await?;
    app.events.changed(ResourceKind::Backups, Some(world_id.to_owned()));
    index(app, world_id).await
}

fn next_run(schedule: &BackupSchedule, now: DateTime<Utc>) -> DateTime<Utc> {
    let interval = chrono::Duration::minutes(schedule.interval_minutes as i64);
    schedule
        .last_run_at
        .as_deref()
        .and_then(|last| DateTime::parse_from_rfc3339(last).ok())
        .map(|last| last.with_timezone(&Utc) + interval)
        .filter(|next| *next > now)
        .unwrap_or(now + interval)
}

/// Keeps the newest `retention` scheduled backups of a world.
async fn prune(app: &AppState, world_id: &str) -> AppResult<()> {
    let retention = app.store.get_schedule(world_id).await?.retention as usize;
    let scheduled: Vec<_> = app
        .store
        .list_backups(Some(world_id))
        .await?
        .into_iter()
        .filter(|backup| backup.kind == BackupKind::Scheduled && backup.status == BackupStatus::Complete)
        .collect();
    for old in scheduled.into_iter().skip(retention) {
        let _ = delete(app, world_id, &old.id).await;
    }
    Ok(())
}

/// Checks schedules every minute and runs the ones that are due.
pub fn spawn_scheduler(app: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if let Err(err) = run_due(&app).await {
                tracing::warn!(error = %err, "backup scheduler");
            }
        }
    });
}

pub async fn run_due(app: &AppState) -> AppResult<()> {
    let now = Utc::now();
    for (world_id, mut schedule) in app.store.enabled_schedules().await? {
        let due = schedule
            .next_run_at
            .as_deref()
            .and_then(|next| DateTime::parse_from_rfc3339(next).ok())
            .is_none_or(|next| next.with_timezone(&Utc) <= now);
        if due {
            create(app, &world_id, BackupKind::Scheduled).await?;
            schedule.last_run_at = Some(now.to_rfc3339());
            schedule.next_run_at = Some(next_run(&schedule, now).to_rfc3339());
            app.store.put_schedule(&world_id, &schedule).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_and_restore_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("world-1");
        std::fs::create_dir_all(world.join("level/region")).unwrap();
        std::fs::write(world.join("level/level.dat"), b"original").unwrap();
        std::fs::write(world.join("level/region/r.0.0.mca"), vec![7u8; 4096]).unwrap();

        let archive = dir.path().join("backup.tar.zst");
        let size = pack(&world, &archive).unwrap();
        assert!(size > 0);

        std::fs::write(world.join("level/level.dat"), b"changed").unwrap();
        std::fs::write(world.join("level/new-file"), b"x").unwrap();
        unpack_replace(&archive, &world).unwrap();

        assert_eq!(std::fs::read(world.join("level/level.dat")).unwrap(), b"original");
        assert!(!world.join("level/new-file").exists());
        assert_eq!(std::fs::read(world.join("level/region/r.0.0.mca")).unwrap().len(), 4096);
    }

    #[test]
    fn next_run_follows_last_run() {
        let schedule = BackupSchedule {
            enabled: true,
            interval_minutes: 60,
            retention: 3,
            last_run_at: Some("2026-01-01T00:00:00Z".into()),
            next_run_at: None,
        };
        let now = DateTime::parse_from_rfc3339("2026-01-01T00:30:00Z").unwrap().with_timezone(&Utc);
        assert_eq!(next_run(&schedule, now).to_rfc3339(), "2026-01-01T01:00:00+00:00");
        let late = DateTime::parse_from_rfc3339("2026-01-01T05:00:00Z").unwrap().with_timezone(&Utc);
        assert_eq!(next_run(&schedule, late), late + chrono::Duration::minutes(60));
    }
}
