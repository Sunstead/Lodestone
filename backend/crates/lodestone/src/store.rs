//! SQLite persistence. Assets are JSON documents; relational rules
//! (world attachment, delete guards) are enforced by the schema.

use std::path::Path;
use std::str::FromStr;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::error::{AppError, AppResult};
use crate::model::{
    AppSettings, Backup, BackupSchedule, BanEntry, InstancePorts, Job, JobState, Profile, StringMap, World,
};
use crate::util::now_iso;

/// A player named on an access list. `uuid` is filled once the server has seen them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerRef {
    pub name: String,
    pub uuid: Option<String>,
}

/// Ops, whitelist and bans. Owned by the profile: access policy travels
/// with the configuration, and Lodestone writes the JSON files on start.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccessLists {
    pub ops: Vec<PlayerRef>,
    pub whitelist: Vec<PlayerRef>,
    pub bans: Vec<BanEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRecord {
    pub profile: Profile,
    /// Overrides for profile-scoped `server.properties` keys.
    pub properties: StringMap,
    pub access: AccessLists,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRecord {
    pub id: String,
    pub name: String,
    pub profile_id: String,
    pub world_id: String,
    pub ports: InstancePorts,
    pub environment: StringMap,
    /// Generated per instance; RCON is only published on 127.0.0.1.
    pub rcon_password: String,
    pub created_at: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorldPlayerRow {
    pub name: String,
    pub uuid: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub playtime_seconds: u64,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

fn decode<T: DeserializeOwned>(raw: &str) -> AppResult<T> {
    Ok(serde_json::from_str(raw)?)
}

impl Store {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(8).connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /* ------------------------------ worlds ------------------------------ */

    /// Worlds with `attachedInstanceId` resolved from the instances table.
    /// `owner` limits the list to one user's worlds; `None` lists all.
    pub async fn list_worlds(&self, owner: Option<&str>) -> AppResult<Vec<World>> {
        let rows = sqlx::query(
            "SELECT w.data, i.id AS instance_id FROM worlds w LEFT JOIN instances i ON i.world_id = w.id
             WHERE ?1 IS NULL OR w.owner_id = ?1 ORDER BY w.created_at",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let mut world: World = decode(row.get::<&str, _>("data"))?;
                world.attached_instance_id = row.get("instance_id");
                Ok(world)
            })
            .collect()
    }

    pub async fn get_world(&self, id: &str) -> AppResult<World> {
        let row = sqlx::query(
            "SELECT w.data, i.id AS instance_id FROM worlds w LEFT JOIN instances i ON i.world_id = w.id WHERE w.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::not_found("world", id))?;
        let mut world: World = decode(row.get::<&str, _>("data"))?;
        world.attached_instance_id = row.get("instance_id");
        Ok(world)
    }

    pub async fn insert_world(&self, world: &World, owner: &str) -> AppResult<()> {
        sqlx::query("INSERT INTO worlds (id, data, created_at, owner_id) VALUES (?, ?, ?, ?)")
            .bind(&world.id)
            .bind(serde_json::to_string(world)?)
            .bind(&world.created_at)
            .bind(owner)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_world(&self, world: &World) -> AppResult<()> {
        sqlx::query("UPDATE worlds SET data = ? WHERE id = ?")
            .bind(serde_json::to_string(world)?)
            .bind(&world.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_world(&self, id: &str) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM worlds WHERE id = ?").bind(id).execute(&self.pool).await;
        map_fk(
            result,
            "world_attached",
            "This world is attached to an instance. Detach or delete the instance first.",
        )?;
        Ok(())
    }

    /* ----------------------------- profiles ----------------------------- */

    pub async fn list_profiles(&self, owner: Option<&str>) -> AppResult<Vec<ProfileRecord>> {
        let rows = sqlx::query(
            "SELECT p.data, (SELECT COUNT(*) FROM instances i WHERE i.profile_id = p.id) AS uses FROM profiles p
             WHERE ?1 IS NULL OR p.owner_id = ?1 ORDER BY p.created_at",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(profile_from_row).collect()
    }

    pub async fn get_profile(&self, id: &str) -> AppResult<ProfileRecord> {
        let row = sqlx::query(
            "SELECT p.data, (SELECT COUNT(*) FROM instances i WHERE i.profile_id = p.id) AS uses FROM profiles p WHERE p.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::not_found("profile", id))?;
        profile_from_row(&row)
    }

    pub async fn insert_profile(&self, record: &ProfileRecord, owner: &str) -> AppResult<()> {
        sqlx::query("INSERT INTO profiles (id, data, created_at, owner_id) VALUES (?, ?, ?, ?)")
            .bind(&record.profile.id)
            .bind(serde_json::to_string(record)?)
            .bind(&record.profile.created_at)
            .bind(owner)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_profile(&self, record: &ProfileRecord) -> AppResult<()> {
        sqlx::query("UPDATE profiles SET data = ? WHERE id = ?")
            .bind(serde_json::to_string(record)?)
            .bind(&record.profile.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_profile(&self, id: &str) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM profiles WHERE id = ?").bind(id).execute(&self.pool).await;
        map_fk(result, "profile_in_use", "Instances still use this configuration. Delete or re-point them first.")?;
        Ok(())
    }

    /* ----------------------------- instances ---------------------------- */

    pub async fn list_instances(&self, owner: Option<&str>) -> AppResult<Vec<InstanceRecord>> {
        let rows = sqlx::query("SELECT data FROM instances WHERE ?1 IS NULL OR owner_id = ?1 ORDER BY created_at")
            .bind(owner)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|row| decode(row.get::<&str, _>("data"))).collect()
    }

    pub async fn get_instance(&self, id: &str) -> AppResult<InstanceRecord> {
        let row = sqlx::query("SELECT data FROM instances WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::not_found("instance", id))?;
        decode(row.get::<&str, _>("data"))
    }

    pub async fn update_instance(&self, record: &InstanceRecord) -> AppResult<()> {
        let result = sqlx::query("UPDATE instances SET data = ?, profile_id = ?, world_id = ? WHERE id = ?")
            .bind(serde_json::to_string(record)?)
            .bind(&record.profile_id)
            .bind(&record.world_id)
            .bind(&record.id)
            .execute(&self.pool)
            .await;
        map_unique(result, "world_attached", "That world is already attached to another instance.")?;
        Ok(())
    }

    pub async fn delete_instance(&self, id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM instances WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Inserts an instance plus any new world/profile in one transaction,
    /// so a failure never leaves orphaned assets behind.
    pub async fn create_instance_bundle(
        &self,
        world: Option<&World>,
        profile: Option<&ProfileRecord>,
        instance: &InstanceRecord,
        owner: &str,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        if let Some(world) = world {
            sqlx::query("INSERT INTO worlds (id, data, created_at, owner_id) VALUES (?, ?, ?, ?)")
                .bind(&world.id)
                .bind(serde_json::to_string(world)?)
                .bind(&world.created_at)
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(profile) = profile {
            sqlx::query("INSERT INTO profiles (id, data, created_at, owner_id) VALUES (?, ?, ?, ?)")
                .bind(&profile.profile.id)
                .bind(serde_json::to_string(profile)?)
                .bind(&profile.profile.created_at)
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }
        let result = sqlx::query(
            "INSERT INTO instances (id, profile_id, world_id, data, created_at, owner_id) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&instance.id)
        .bind(&instance.profile_id)
        .bind(&instance.world_id)
        .bind(serde_json::to_string(instance)?)
        .bind(&instance.created_at)
        .bind(owner)
        .execute(&mut *tx)
        .await;
        map_unique(result, "world_attached", "That world is already attached to another instance.")?;
        tx.commit().await?;
        Ok(())
    }

    /* ------------------------------ backups ----------------------------- */

    pub async fn list_backups(&self, world_id: Option<&str>) -> AppResult<Vec<Backup>> {
        let rows = match world_id {
            Some(id) => {
                sqlx::query("SELECT data FROM backups WHERE world_id = ? ORDER BY created_at DESC")
                    .bind(id)
                    .fetch_all(&self.pool)
                    .await?
            }
            None => sqlx::query("SELECT data FROM backups ORDER BY created_at DESC").fetch_all(&self.pool).await?,
        };
        rows.iter().map(|row| decode(row.get::<&str, _>("data"))).collect()
    }

    pub async fn get_backup(&self, id: &str) -> AppResult<Backup> {
        let row = sqlx::query("SELECT data FROM backups WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::not_found("backup", id))?;
        decode(row.get::<&str, _>("data"))
    }

    pub async fn put_backup(&self, backup: &Backup) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO backups (id, world_id, data, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
        )
        .bind(&backup.id)
        .bind(&backup.world_id)
        .bind(serde_json::to_string(backup)?)
        .bind(&backup.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_backup(&self, id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM backups WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn get_schedule(&self, world_id: &str) -> AppResult<BackupSchedule> {
        let row = sqlx::query("SELECT data FROM backup_schedules WHERE world_id = ?")
            .bind(world_id)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => decode(row.get::<&str, _>("data")),
            None => Ok(BackupSchedule {
                enabled: false,
                interval_minutes: 360,
                retention: 7,
                last_run_at: None,
                next_run_at: None,
            }),
        }
    }

    pub async fn put_schedule(&self, world_id: &str, schedule: &BackupSchedule) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO backup_schedules (world_id, data) VALUES (?, ?)
             ON CONFLICT(world_id) DO UPDATE SET data = excluded.data",
        )
        .bind(world_id)
        .bind(serde_json::to_string(schedule)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `(world_id, schedule)` for every enabled schedule.
    pub async fn enabled_schedules(&self) -> AppResult<Vec<(String, BackupSchedule)>> {
        let rows = sqlx::query("SELECT world_id, data FROM backup_schedules").fetch_all(&self.pool).await?;
        let mut out = Vec::new();
        for row in rows {
            let schedule: BackupSchedule = decode(row.get::<&str, _>("data"))?;
            if schedule.enabled {
                out.push((row.get::<String, _>("world_id"), schedule));
            }
        }
        Ok(out)
    }

    /* ---------------------------- world players ------------------------- */

    pub async fn player_joined(&self, world_id: &str, name: &str, uuid: Option<&str>) -> AppResult<()> {
        let now = now_iso();
        sqlx::query(
            "INSERT INTO world_players (world_id, name, uuid, first_seen, last_seen) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(world_id, name) DO UPDATE SET last_seen = excluded.last_seen,
               uuid = COALESCE(excluded.uuid, world_players.uuid)",
        )
        .bind(world_id)
        .bind(name)
        .bind(uuid)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn player_left(&self, world_id: &str, name: &str, session_seconds: u64) -> AppResult<()> {
        sqlx::query(
            "UPDATE world_players SET last_seen = ?, playtime_seconds = playtime_seconds + ? WHERE world_id = ? AND name = ?",
        )
        .bind(now_iso())
        .bind(session_seconds as i64)
        .bind(world_id)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn world_players(&self, world_id: &str) -> AppResult<Vec<WorldPlayerRow>> {
        let rows = sqlx::query(
            "SELECT name, uuid, first_seen, last_seen, playtime_seconds FROM world_players WHERE world_id = ? ORDER BY last_seen DESC",
        )
        .bind(world_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| WorldPlayerRow {
                name: row.get("name"),
                uuid: row.get("uuid"),
                first_seen: row.get("first_seen"),
                last_seen: row.get("last_seen"),
                playtime_seconds: row.get::<i64, _>("playtime_seconds").max(0) as u64,
            })
            .collect())
    }

    /* -------------------------------- jobs ------------------------------ */

    pub async fn put_job(&self, job: &Job) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO jobs (id, data, created_at) VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE SET data = excluded.data",
        )
        .bind(&job.id)
        .bind(serde_json::to_string(job)?)
        .bind(&job.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_job(&self, id: &str) -> AppResult<Job> {
        let row = sqlx::query("SELECT data FROM jobs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::not_found("job", id))?;
        decode(row.get::<&str, _>("data"))
    }

    pub async fn recent_jobs(&self, limit: u32) -> AppResult<Vec<Job>> {
        let rows = sqlx::query("SELECT data FROM jobs ORDER BY created_at DESC LIMIT ?")
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|row| decode(row.get::<&str, _>("data"))).collect()
    }

    /// Jobs left running by a previous process can never finish.
    pub async fn fail_interrupted_jobs(&self) -> AppResult<()> {
        let rows = sqlx::query("SELECT data FROM jobs").fetch_all(&self.pool).await?;
        for row in rows {
            let mut job: Job = decode(row.get::<&str, _>("data"))?;
            if matches!(job.state, JobState::Queued | JobState::Running) {
                job.state = JobState::Failed;
                job.error = Some("Interrupted: Lodestone restarted while this was running.".into());
                job.finished_at = Some(now_iso());
                self.put_job(&job).await?;
            }
        }
        Ok(())
    }

    /* ------------------------------ settings ---------------------------- */

    pub async fn settings(&self) -> AppResult<AppSettings> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = 'app'").fetch_optional(&self.pool).await?;
        match row {
            Some(row) => decode(row.get::<&str, _>("value")),
            None => Ok(AppSettings {
                public_address: String::new(),
                default_java_args: "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200".into(),
            }),
        }
    }

    pub async fn put_settings(&self, settings: &AppSettings) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('app', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(serde_json::to_string(settings)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /* ------------------------------ content ----------------------------- */

    pub async fn list_content(&self, profile_id: &str) -> AppResult<Vec<crate::content::ContentRecord>> {
        let rows = sqlx::query("SELECT data FROM profile_content WHERE profile_id = ? ORDER BY created_at, id")
            .bind(profile_id)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|row| decode(row.get::<&str, _>("data"))).collect()
    }

    /// Inserts or replaces a lockfile row (by id).
    pub async fn put_content(&self, profile_id: &str, record: &crate::content::ContentRecord) -> AppResult<()> {
        let result = sqlx::query(
            "INSERT INTO profile_content (id, profile_id, provider, project_id, filename, data, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET provider = excluded.provider, project_id = excluded.project_id,
                 filename = excluded.filename, data = excluded.data",
        )
        .bind(&record.id)
        .bind(profile_id)
        .bind(record.provider.as_str())
        .bind(&record.project_id)
        .bind(&record.filename)
        .bind(serde_json::to_string(record)?)
        .bind(&record.installed_at)
        .execute(&self.pool)
        .await;
        map_unique(
            result,
            "content_exists",
            &format!("This configuration already has {} (or a file named {}).", record.name, record.filename),
        )?;
        Ok(())
    }

    pub async fn delete_content(&self, profile_id: &str, id: &str) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM profile_content WHERE profile_id = ? AND id = ?")
            .bind(profile_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::not_found("mod or plugin", id));
        }
        Ok(())
    }

    /// A small per-key value in the settings table.
    pub async fn get_value(&self, key: &str) -> AppResult<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("value")))
    }

    pub async fn put_value(&self, key: &str, value: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /* ------------------------------ http cache -------------------------- */

    pub async fn cache_get(&self, key: &str, max_age_secs: i64) -> AppResult<Option<String>> {
        let row = sqlx::query("SELECT body, fetched_at FROM http_cache WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|row| {
            let fetched: i64 = row.get("fetched_at");
            (chrono::Utc::now().timestamp() - fetched <= max_age_secs).then(|| row.get("body"))
        }))
    }

    pub async fn cache_put(&self, key: &str, body: &str) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO http_cache (key, body, fetched_at) VALUES (?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at",
        )
        .bind(key)
        .bind(body)
        .bind(chrono::Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn profile_from_row(row: &sqlx::sqlite::SqliteRow) -> AppResult<ProfileRecord> {
    let mut record: ProfileRecord = decode(row.get::<&str, _>("data"))?;
    record.profile.instance_count = row.get::<i64, _>("uses").max(0) as u32;
    Ok(record)
}

fn is_constraint(err: &sqlx::Error, needle: &str) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.message().contains(needle))
}

fn map_fk<T>(result: Result<T, sqlx::Error>, code: &'static str, message: &str) -> AppResult<T> {
    result.map_err(
        |err| {
            if is_constraint(&err, "FOREIGN KEY") {
                AppError::conflict(code, message)
            } else {
                err.into()
            }
        },
    )
}

fn map_unique<T>(result: Result<T, sqlx::Error>, code: &'static str, message: &str) -> AppResult<T> {
    result.map_err(|err| if is_constraint(&err, "UNIQUE") { AppError::conflict(code, message) } else { err.into() })
}

/* -------------------------------- users --------------------------------- */

/// Someone who signs in. Owns worlds, profiles and instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub admin: bool,
    pub eula_accepted_at: Option<String>,
    pub created_at: String,
    pub last_seen_at: String,
}

/// Which table an owned asset lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asset {
    World,
    Profile,
    Instance,
}

impl Asset {
    fn table(self) -> &'static str {
        match self {
            Self::World => "worlds",
            Self::Profile => "profiles",
            Self::Instance => "instances",
        }
    }
}

/// A sign-in in progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcFlow {
    pub state: String,
    pub pkce_verifier: String,
    pub nonce: String,
    pub return_to: String,
}

pub const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
const FLOW_TTL_SECS: i64 = 10 * 60;

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn hash_token(token: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(token.as_bytes()))
}

fn user_from_row(row: &sqlx::sqlite::SqliteRow) -> UserRow {
    UserRow {
        id: row.get("id"),
        username: row.get("username"),
        display_name: row.get("display_name"),
        admin: row.get::<i64, _>("admin") != 0,
        eula_accepted_at: row.get("eula_accepted_at"),
        created_at: row.get("created_at"),
        last_seen_at: row.get("last_seen_at"),
    }
}

impl Store {
    /// Finds or creates the user for an identity, refreshing the name and
    /// admin flag the provider reports. Usernames can change; the subject
    /// can't, so it's the key.
    pub async fn upsert_user(
        &self,
        issuer: &str,
        subject: &str,
        username: &str,
        display_name: &str,
        admin: bool,
    ) -> AppResult<UserRow> {
        let now = now_iso();
        sqlx::query(
            "INSERT INTO users (id, issuer, subject, username, display_name, admin, created_at, last_seen_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(issuer, subject) DO UPDATE SET username = excluded.username,
                 display_name = excluded.display_name, admin = excluded.admin, last_seen_at = excluded.last_seen_at",
        )
        .bind(crate::util::new_id("user"))
        .bind(issuer)
        .bind(subject)
        .bind(username)
        .bind(display_name)
        .bind(admin as i64)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query("SELECT * FROM users WHERE issuer = ? AND subject = ?")
            .bind(issuer)
            .bind(subject)
            .fetch_one(&self.pool)
            .await?;
        Ok(user_from_row(&row))
    }

    pub async fn get_user(&self, id: &str) -> AppResult<UserRow> {
        let row = sqlx::query("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::not_found("user", id))?;
        Ok(user_from_row(&row))
    }

    pub async fn list_users(&self) -> AppResult<Vec<(UserRow, [u64; 3])>> {
        let rows = sqlx::query(
            "SELECT u.*,
                (SELECT COUNT(*) FROM worlds w WHERE w.owner_id = u.id) AS worlds,
                (SELECT COUNT(*) FROM profiles p WHERE p.owner_id = u.id) AS profiles,
                (SELECT COUNT(*) FROM instances i WHERE i.owner_id = u.id) AS instances
             FROM users u ORDER BY u.username",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| {
                let count = |name: &str| row.get::<i64, _>(name) as u64;
                (user_from_row(row), [count("worlds"), count("profiles"), count("instances")])
            })
            .collect())
    }

    pub async fn set_eula(&self, user_id: &str, accepted: bool) -> AppResult<()> {
        sqlx::query("UPDATE users SET eula_accepted_at = ? WHERE id = ?")
            .bind(accepted.then(now_iso))
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Who owns an asset. `None` for assets made before accounts existed.
    pub async fn owner_of(&self, asset: Asset, id: &str) -> AppResult<Option<String>> {
        let row = sqlx::query(&format!("SELECT owner_id FROM {} WHERE id = ?", asset.table()))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => Ok(row.get("owner_id")),
            None => Err(AppError::not_found(
                match asset {
                    Asset::World => "world",
                    Asset::Profile => "configuration",
                    Asset::Instance => "server",
                },
                id,
            )),
        }
    }

    /// Gives assets nobody owns (made before sign-in existed) to `user_id`.
    pub async fn claim_orphans(&self, user_id: &str) -> AppResult<u64> {
        let mut claimed = 0;
        for table in ["worlds", "profiles", "instances"] {
            claimed += sqlx::query(&format!("UPDATE {table} SET owner_id = ? WHERE owner_id IS NULL"))
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected();
        }
        Ok(claimed)
    }

    /* ------------------------------ sessions ---------------------------- */

    /// Starts a session and returns its token (shown to the browser once).
    pub async fn create_session(&self, user_id: &str) -> AppResult<String> {
        let token = crate::util::secret(32);
        let now = unix_now();
        sqlx::query("INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
            .bind(hash_token(&token))
            .bind(user_id)
            .bind(now)
            .bind(now + SESSION_TTL_SECS)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM sessions WHERE expires_at < ?").bind(now).execute(&self.pool).await?;
        Ok(token)
    }

    /// The session's user, sliding its expiry forward.
    pub async fn session_user(&self, token: &str) -> AppResult<Option<UserRow>> {
        let now = unix_now();
        let hash = hash_token(token);
        let row = sqlx::query(
            "SELECT u.* FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(&hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        if row.is_some() {
            sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
                .bind(now + SESSION_TTL_SECS)
                .bind(&hash)
                .execute(&self.pool)
                .await?;
        }
        Ok(row.as_ref().map(user_from_row))
    }

    pub async fn delete_session(&self, token: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?").bind(hash_token(token)).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn put_flow(&self, flow: &OidcFlow) -> AppResult<()> {
        let now = unix_now();
        sqlx::query("DELETE FROM oidc_flows WHERE created_at < ?")
            .bind(now - FLOW_TTL_SECS)
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO oidc_flows (state, data, created_at) VALUES (?, ?, ?)")
            .bind(&flow.state)
            .bind(serde_json::to_string(flow)?)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Takes a flow once: a callback can't be replayed.
    pub async fn take_flow(&self, state: &str) -> AppResult<Option<OidcFlow>> {
        let row = sqlx::query("DELETE FROM oidc_flows WHERE state = ? AND created_at >= ? RETURNING data")
            .bind(state)
            .bind(unix_now() - FLOW_TTL_SECS)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| decode(row.get::<&str, _>("data"))).transpose()
    }
}

impl Store {
    /// Who owns whatever `id` names (a world, profile, instance or backup).
    /// `None` when nothing has that id; `Some(None)` when it has no owner.
    pub async fn owner_of_any(&self, id: &str) -> AppResult<Option<Option<String>>> {
        let row = sqlx::query(
            "SELECT owner_id FROM worlds WHERE id = ?1
             UNION ALL SELECT owner_id FROM profiles WHERE id = ?1
             UNION ALL SELECT owner_id FROM instances WHERE id = ?1
             UNION ALL SELECT w.owner_id FROM backups b JOIN worlds w ON w.id = b.world_id WHERE b.id = ?1
             LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.get("owner_id")))
    }
}
