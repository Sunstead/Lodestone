//! Changing a profile's lockfile, and putting its jars on disk.
//!
//! Every change goes through `Services::content_lock`, so two installs into
//! the same profile can't interleave. Files are downloaded into the blob
//! cache (sha512-verified) when installed; a server's prepare step links the
//! enabled ones into `profiles/<id>/plugins` or `mods` and removes any other
//! top-level jar there. Lodestone owns those jar files; plugin and mod
//! config folders beside them are left alone.

use std::path::Path;

use super::{entries, jarmeta, plan, safe_filename, ContentRecord, FileRef, Target};
use crate::app::AppState;
use crate::cache::{Artifact, Hash, HashAlgo, Source};
use crate::error::{AppError, AppResult};
use crate::jobs::JobHandle;
use crate::model::{
    AvailableUpdate, ContentIndex, ContentKind, ContentRef, ContentSource, JobKind, ModKind, ResourceKind,
};
use crate::util::{new_id, now_iso};

fn checked_key(profile_id: &str) -> String {
    format!("content-checked:{profile_id}")
}

pub async fn index(app: &AppState, profile_id: &str) -> AppResult<ContentIndex> {
    let profile = app.store.get_profile(profile_id).await?.profile;
    let records = app.store.list_content(profile_id).await?;
    let target = Target::for_profile(&profile).ok();
    Ok(ContentIndex {
        kind: profile.software_id.content_kind(),
        loaders: Target::loaders_for(profile.software_id),
        game_version: profile.version.clone(),
        entries: entries(&records, target.as_ref(), |file| is_cached(app, file)),
        checked_at: app.store.get_value(&checked_key(profile_id)).await?,
        providers: target.as_ref().map(|target| app.content.info(target)).unwrap_or_default(),
    })
}

fn is_cached(app: &AppState, file: &FileRef) -> bool {
    file.hash().and_then(|hash| app.cache.cached(&hash)).is_some()
}

/// The file's blob: from the cache, else downloaded. `None` for a manual
/// download nobody has uploaded yet.
async fn fetch_file(
    app: &AppState,
    file: &FileRef,
    filename: &str,
    size: u64,
) -> AppResult<Option<std::path::PathBuf>> {
    if let Some(cached) = file.hash().and_then(|hash| app.cache.cached(&hash)) {
        return Ok(Some(cached));
    }
    if file.is_manual() {
        return Ok(None);
    }
    Ok(Some(app.cache.fetch(&file.artifact(filename, size)?, None).await?))
}

/// Puts bytes in the cache under `file`'s hash (they've been checked).
async fn cache_bytes(app: &AppState, file: &FileRef, filename: &str, bytes: Vec<u8>) -> AppResult<()> {
    let size = bytes.len() as u64;
    let hash = match file.hash() {
        Some(hash) => hash,
        None => Hash { algo: HashAlgo::Sha512, hex: HashAlgo::Sha512.digest(&bytes) },
    };
    let artifact = Artifact {
        filename: filename.to_owned(),
        source: Source::Inline(bytes),
        hash: Some(hash),
        size: Some(size),
        pin: None,
    };
    app.cache.fetch(&artifact, None).await?;
    Ok(())
}

async fn target_of(app: &AppState, profile_id: &str) -> AppResult<Target> {
    Target::for_profile(&app.store.get_profile(profile_id).await?.profile)
}

fn changed(app: &AppState, profile_id: &str) {
    app.events.changed(ResourceKind::Profiles, Some(profile_id.to_owned()));
}

/// Finishes `job` with `work`'s outcome, so a failure is recorded too.
pub async fn run_job<T>(
    job: &JobHandle,
    work: impl std::future::Future<Output = AppResult<(T, String)>>,
) -> AppResult<T> {
    match work.await {
        Ok((value, done)) => {
            job.succeed(done).await;
            Ok(value)
        }
        Err(err) => {
            job.fail(err.to_string()).await;
            Err(err)
        }
    }
}

/// Installs exact versions (normally an install plan's `install` list).
/// An item for a project that's already installed replaces it in place.
pub async fn apply(app: &AppState, profile_id: &str, items: &[ContentRef]) -> AppResult<ContentIndex> {
    if items.is_empty() {
        return index(app, profile_id).await;
    }
    let _guard = app.content_lock.lock().await;
    let target = target_of(app, profile_id).await?;

    // Resolve everything before downloading anything.
    let mut resolved = Vec::with_capacity(items.len());
    for item in items {
        let version_id = item
            .version_id
            .as_deref()
            .ok_or_else(|| AppError::bad_request("Pick a version for each item to install."))?;
        let provider = app.content.for_target(item.provider, &target)?;
        let version = provider.version(version_id, &target).await?;
        if version.version.project_id != item.project_id {
            return Err(AppError::bad_request(format!("Version {version_id} doesn't belong to that project.")));
        }
        if !version.version.compatible {
            return Err(AppError::conflict(
                "incompatible",
                format!("{} isn't built for {}.", version.version.name, target.describe()),
            ));
        }
        let project = provider.project(&item.project_id).await?;
        resolved.push((project, version));
    }

    let label = match resolved.as_slice() {
        [(project, _)] => project.name.clone(),
        many => format!("{} items", many.len()),
    };
    let job = app.jobs.create(JobKind::Install, profile_id, format!("Installing {label}")).await?;
    let work = async {
        let total = resolved.len();
        for (index, (_, version)) in resolved.iter().enumerate() {
            job.progress(Some(index as f64 / total as f64), format!("Downloading {}", version.version.filename)).await;
            // Manual downloads are recorded now and uploaded by the user later.
            fetch_file(app, &version.file, &version.version.filename, version.version.size_bytes).await?;
        }
        let existing = app.store.list_content(profile_id).await?;
        let now = now_iso();
        for (project, version) in &resolved {
            // A dependency if something else in this batch, or installed, needs it.
            let needed = resolved.iter().any(|(_, other)| {
                other.version.dependencies.iter().any(|dep| {
                    dep.kind == crate::model::DependencyKind::Required
                        && dep.project_id.as_deref() == Some(project.id.as_str())
                })
            }) || existing.iter().any(|record| record.requires(&project.id));
            let previous = existing.iter().find(|record| {
                record.provider == version.version.provider && record.project_id.as_deref() == Some(project.id.as_str())
            });
            let mut record = ContentRecord::from_version(
                previous.map(|r| r.id.clone()).unwrap_or_else(|| new_id("content")),
                project,
                version,
                target.kind,
                previous.map(|r| r.dependency).unwrap_or(needed),
                previous.map(|r| r.installed_at.clone()).unwrap_or_else(|| now.clone()),
            );
            if let Some(previous) = previous {
                record.enabled = previous.enabled;
            }
            app.store.put_content(profile_id, &record).await?;
        }
        Ok(((), format!("Installed {label}")))
    };
    run_job(&job, work).await?;
    changed(app, profile_id);
    index(app, profile_id).await
}

/// Plans and installs what it takes to bring one entry to its newest
/// version, including any new required dependencies.
pub async fn update_entry(app: &AppState, profile_id: &str, entry_id: &str) -> AppResult<ContentIndex> {
    let target = target_of(app, profile_id).await?;
    let records = app.store.list_content(profile_id).await?;
    let record = records
        .iter()
        .find(|record| record.id == entry_id)
        .ok_or_else(|| AppError::not_found("mod or plugin", entry_id))?;
    let (Some(project_id), Some(update)) = (&record.project_id, &record.update) else {
        return Err(AppError::conflict("no_update", format!("{} is up to date.", record.name)));
    };
    let request = ContentRef {
        provider: record.provider,
        project_id: project_id.clone(),
        version_id: Some(update.version_id.clone()),
    };
    let plan = plan::plan(&app.content, &target, &records, &[request]).await?;
    if let Some(conflict) = plan.conflicts.first() {
        return Err(AppError::conflict("plan_conflict", conflict.message.clone()));
    }
    let items: Vec<ContentRef> = plan
        .install
        .iter()
        .map(|item| ContentRef {
            provider: item.provider,
            project_id: item.project_id.clone(),
            version_id: Some(item.version_id.clone()),
        })
        .collect();
    apply(app, profile_id, &items).await
}

pub async fn set_enabled(app: &AppState, profile_id: &str, entry_id: &str, enabled: bool) -> AppResult<ContentIndex> {
    let _guard = app.content_lock.lock().await;
    let mut record = app
        .store
        .list_content(profile_id)
        .await?
        .into_iter()
        .find(|record| record.id == entry_id)
        .ok_or_else(|| AppError::not_found("mod or plugin", entry_id))?;
    record.enabled = enabled;
    app.store.put_content(profile_id, &record).await?;
    changed(app, profile_id);
    drop(_guard);
    index(app, profile_id).await
}

pub async fn remove(app: &AppState, profile_id: &str, entry_id: &str) -> AppResult<ContentIndex> {
    let _guard = app.content_lock.lock().await;
    app.store.delete_content(profile_id, entry_id).await?;
    changed(app, profile_id);
    drop(_guard);
    index(app, profile_id).await
}

/// Asks each provider for the newest build of every installed file.
pub async fn check_updates(app: &AppState, profile_id: &str) -> AppResult<ContentIndex> {
    let _guard = app.content_lock.lock().await;
    let target = target_of(app, profile_id).await?;
    let records = app.store.list_content(profile_id).await?;
    for provider in app.content.all() {
        if provider.unavailable(&target).is_some() {
            continue;
        }
        let mine: Vec<ContentRecord> = records.iter().filter(|r| r.provider == provider.id()).cloned().collect();
        if mine.is_empty() {
            continue;
        }
        // One source being down shouldn't hide the others' updates.
        let latest = match provider.latest(&mine, &target).await {
            Ok(latest) => latest,
            Err(err) => {
                tracing::warn!(provider = provider.id().as_str(), error = %err, "update check failed");
                continue;
            }
        };
        for record in mine {
            let update = latest
                .get(&record.id)
                .filter(|found| record.version_id.as_deref() != Some(found.version.id.as_str()))
                .map(|found| AvailableUpdate {
                    version_id: found.version.id.clone(),
                    version_name: found.version.version_number.clone(),
                });
            if update != record.update {
                let mut record = record;
                record.update = update;
                app.store.put_content(profile_id, &record).await?;
            }
        }
    }
    app.store.put_value(&checked_key(profile_id), &now_iso()).await?;
    changed(app, profile_id);
    drop(_guard);
    index(app, profile_id).await
}

/// Adds an uploaded jar. If a provider recognises the file it's recorded as
/// that project (so it gets updates); otherwise its own descriptor names it.
pub async fn upload(app: &AppState, profile_id: &str, filename: &str, bytes: Vec<u8>) -> AppResult<ContentIndex> {
    let target = target_of(app, profile_id).await?;
    let filename = safe_filename(filename).ok_or_else(|| AppError::bad_request("Upload a .jar file."))?;
    // The file a manual download is waiting for: its published hash already
    // vouches for it.
    {
        let _guard = app.content_lock.lock().await;
        let existing = app.store.list_content(profile_id).await?;
        if let Some(waiting) = existing.iter().find(|r| r.file.is_manual() && r.file.matches(&bytes)) {
            cache_bytes(app, &waiting.file, &waiting.filename, bytes).await?;
            changed(app, profile_id);
            drop(_guard);
            return index(app, profile_id).await;
        }
    }
    let meta = jarmeta::read(&bytes).ok_or_else(|| AppError::bad_request(format!("{filename} isn't a valid jar.")))?;
    if let Some(kind) = meta.kind.filter(|kind| *kind != target.kind) {
        let (is, wants) = match kind {
            ModKind::Plugin => ("a plugin", "mods"),
            ModKind::Mod => ("a mod", "plugins"),
        };
        return Err(AppError::bad_request(format!(
            "{filename} is {is}, but {} runs {wants}.",
            target.software.label()
        )));
    }
    let hashes = FileRef::of(&bytes);
    let sha512 = hashes.sha512.clone().unwrap_or_default();
    let size = bytes.len() as u64;

    let _guard = app.content_lock.lock().await;
    let existing = app.store.list_content(profile_id).await?;
    cache_bytes(app, &hashes, &filename, bytes).await?;
    let mut record = None;
    for provider in app.content.all() {
        if provider.unavailable(&target).is_some() {
            continue;
        }
        let Ok(found) = provider.identify(std::slice::from_ref(&sha512), &target).await else { continue };
        let Some(version) = found.get(&sha512) else { continue };
        let Ok(project) = provider.project(&version.version.project_id).await else { continue };
        let previous = existing
            .iter()
            .find(|r| r.provider == provider.id() && r.project_id.as_deref() == Some(project.id.as_str()));
        let mut found_record = ContentRecord::from_version(
            previous.map(|r| r.id.clone()).unwrap_or_else(|| new_id("content")),
            &project,
            version,
            target.kind,
            false,
            now_iso(),
        );
        found_record.file.sha512 = Some(sha512.clone());
        record = Some(found_record);
        break;
    }
    let record = match record {
        Some(record) => record,
        None => {
            let stem = filename.trim_end_matches(".jar").trim_end_matches(".JAR").to_owned();
            ContentRecord {
                id: new_id("content"),
                provider: ContentSource::Upload,
                project_id: None,
                version_id: None,
                slug: None,
                name: meta.name.unwrap_or(stem),
                version_name: meta.version.unwrap_or_default(),
                filename: filename.clone(),
                size_bytes: size,
                kind: target.kind,
                author: meta.author.unwrap_or_default(),
                description: meta.description.unwrap_or_default(),
                icon_url: None,
                page_url: None,
                file: hashes.clone(),
                enabled: true,
                dependency: false,
                dependencies: vec![],
                game_versions: vec![],
                loaders: vec![],
                update: None,
                installed_at: now_iso(),
            }
        }
    };
    app.store.put_content(profile_id, &record).await?;
    changed(app, profile_id);
    drop(_guard);
    index(app, profile_id).await
}

/// Supplies the file for an entry Lodestone couldn't download (a manual
/// download), after checking it's exactly the published file.
pub async fn provide_file(
    app: &AppState,
    profile_id: &str,
    entry_id: &str,
    upload_name: &str,
    bytes: Vec<u8>,
) -> AppResult<ContentIndex> {
    let _guard = app.content_lock.lock().await;
    let record = app
        .store
        .list_content(profile_id)
        .await?
        .into_iter()
        .find(|record| record.id == entry_id)
        .ok_or_else(|| AppError::not_found("mod or plugin", entry_id))?;
    let unverifiable = record.file.hash().is_none();
    if !unverifiable && !record.file.matches(&bytes) {
        let page = record.file.manual_url.as_deref().unwrap_or("its download page");
        return Err(AppError::bad_request(format!(
            "{upload_name} isn't {}: its checksum doesn't match. Download exactly that file from {page}.",
            record.filename
        )));
    }
    let mut record = record;
    if unverifiable {
        // Nothing published to check against (some external links): pin
        // what the user gave us from now on.
        record.file = FileRef { manual_url: record.file.manual_url.clone(), ..FileRef::of(&bytes) };
        record.size_bytes = bytes.len() as u64;
        app.store.put_content(profile_id, &record).await?;
    }
    cache_bytes(app, &record.file, &record.filename, bytes).await?;
    changed(app, profile_id);
    drop(_guard);
    index(app, profile_id).await
}

/// Puts the profile's enabled jars in its plugin or mod folder, and takes
/// out any other top-level jar. Called when a server prepares.
pub async fn sync_files(app: &AppState, profile_id: &str, job: Option<&JobHandle>) -> AppResult<()> {
    let profile = app.store.get_profile(profile_id).await?.profile;
    if profile.software_id.content_kind() == ContentKind::Vanilla {
        return Ok(());
    }
    let target = Target::for_profile(&profile)?;
    let dir = app.paths.profile(profile_id).join(target.folder());
    tokio::fs::create_dir_all(&dir).await?;
    let records = app.store.list_content(profile_id).await?;
    let wanted: Vec<&ContentRecord> = records.iter().filter(|r| r.enabled && r.kind == target.kind).collect();
    let total = wanted.len().max(1);
    let mut missing = vec![];
    for (index, record) in wanted.iter().enumerate() {
        if let Some(job) = job {
            job.progress(Some(index as f64 / total as f64), format!("Getting {}", record.filename)).await;
        }
        match fetch_file(app, &record.file, &record.filename, record.size_bytes).await? {
            Some(blob) => place(&blob, &dir.join(&record.filename))?,
            None => missing.push(record.name.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(AppError::conflict(
            "content_missing",
            format!(
                "{} {} to be downloaded by hand first. Open Mods & Plugins on the configuration to get {}.",
                missing.join(", "),
                if missing.len() == 1 { "has" } else { "have" },
                if missing.len() == 1 { "it" } else { "them" }
            ),
        ));
    }
    let mut listing = tokio::fs::read_dir(&dir).await?;
    while let Some(entry) = listing.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_jar = name.to_ascii_lowercase().ends_with(".jar") && entry.file_type().await?.is_file();
        if is_jar && !wanted.iter().any(|record| record.filename == name) {
            tokio::fs::remove_file(entry.path()).await?;
        }
    }
    Ok(())
}

/// Hard-links the cached blob into place (a copy across filesystems).
fn place(blob: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(dest) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    if std::fs::hard_link(blob, dest).is_err() {
        std::fs::copy(blob, dest)?;
    }
    Ok(())
}
