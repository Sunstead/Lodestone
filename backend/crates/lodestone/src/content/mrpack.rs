//! Modrinth modpacks (`.mrpack`): a zip with `modrinth.index.json` (files
//! to download, by hash) plus `overrides/` and `server-overrides/` folders.
//! Importing one makes a new profile: its loader and Minecraft version come
//! from the pack's `dependencies`, its mods become lockfile entries, and
//! the overrides are copied into the profile folder. Client-only files
//! (`env.server: unsupported`) and `client-overrides/` are skipped.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::install::run_job;
use super::{safe_filename, ContentRecord, FileRef, Target};
use crate::app::AppState;
use crate::cache::{Artifact, Hash, HashAlgo, Source};
use crate::error::{AppError, AppResult};
use crate::model::{
    ContentSource, CreateProfileRequest, Job, JobKind, ModKind, ProfileMemory, ResourceKind, ServerSoftwareId,
};
use crate::util::{new_id, now_iso};

/// Hosts the format allows files to come from.
const ALLOWED_HOSTS: &[&str] = &["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"];
const MAX_ENTRY_BYTES: u64 = 256 << 20;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    format_version: u32,
    game: String,
    #[serde(default)]
    version_id: String,
    name: String,
    #[serde(default)]
    summary: Option<String>,
    files: Vec<IndexFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    path: String,
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    downloads: Vec<String>,
    file_size: u64,
}

#[derive(Debug, Clone)]
pub struct PackFile {
    pub path: PathBuf,
    pub sha512: String,
    pub sha1: Option<String>,
    pub size: u64,
    pub url: String,
}

#[derive(Debug)]
pub struct Pack {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub minecraft: String,
    pub software: ServerSoftwareId,
    pub files: Vec<PackFile>,
    /// Files from `overrides/` then `server-overrides/` (later wins).
    pub overrides: Vec<(PathBuf, Vec<u8>)>,
}

/// A relative path that stays inside the profile folder.
pub fn safe_relative(raw: &str) -> Option<PathBuf> {
    if raw.contains('\\') || raw.contains('\0') {
        return None;
    }
    let path = Path::new(raw);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str()?;
                if part.starts_with('.') && part.len() <= 2 {
                    return None;
                }
                out.push(part);
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn allowed_url(url: &str) -> bool {
    url::Url::parse(url)
        .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some_and(|host| ALLOWED_HOSTS.contains(&host)))
}

pub fn parse(bytes: &[u8]) -> AppResult<Pack> {
    let bad = |message: &str| AppError::bad_request(format!("That isn't a Modrinth modpack: {message}"));
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| bad("it isn't a zip file."))?;
    let index: Index = {
        let mut file = archive.by_name("modrinth.index.json").map_err(|_| bad("modrinth.index.json is missing."))?;
        let mut raw = String::new();
        file.read_to_string(&mut raw).map_err(|_| bad("modrinth.index.json can't be read."))?;
        serde_json::from_str(&raw).map_err(|err| bad(&format!("modrinth.index.json is malformed ({err}).")))?
    };
    if index.format_version != 1 || index.game != "minecraft" {
        return Err(bad("unsupported format version."));
    }
    let minecraft =
        index.dependencies.get("minecraft").cloned().ok_or_else(|| bad("it names no Minecraft version."))?;
    let software = if index.dependencies.contains_key("fabric-loader") {
        ServerSoftwareId::Fabric
    } else if index.dependencies.contains_key("neoforge") {
        ServerSoftwareId::Neoforge
    } else if index.dependencies.contains_key("forge") {
        ServerSoftwareId::Forge
    } else if index.dependencies.contains_key("quilt-loader") {
        return Err(AppError::bad_request("This modpack needs Quilt, which Lodestone doesn't run yet."));
    } else {
        ServerSoftwareId::Vanilla
    };

    let mut files = vec![];
    for file in index.files {
        if file.env.as_ref().and_then(|env| env.get("server")).is_some_and(|server| server == "unsupported") {
            continue;
        }
        let path = safe_relative(&file.path).ok_or_else(|| bad(&format!("unsafe path {}.", file.path)))?;
        let sha512 = file.hashes.get("sha512").cloned().ok_or_else(|| bad(&format!("{} has no sha512.", file.path)))?;
        let url = file
            .downloads
            .into_iter()
            .find(|url| allowed_url(url))
            .ok_or_else(|| bad(&format!("{} isn't hosted anywhere Lodestone downloads from.", file.path)))?;
        files.push(PackFile { path, sha512, sha1: file.hashes.get("sha1").cloned(), size: file.file_size, url });
    }

    let mut overrides: Vec<(PathBuf, Vec<u8>)> = vec![];
    for prefix in ["overrides/", "server-overrides/"] {
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|_| bad("an entry can't be read."))?;
            let Some(rest) = entry.name().strip_prefix(prefix).map(str::to_owned) else { continue };
            if entry.is_dir() || rest.is_empty() {
                continue;
            }
            let path = safe_relative(&rest).ok_or_else(|| bad(&format!("unsafe path {rest}.")))?;
            if entry.size() > MAX_ENTRY_BYTES {
                return Err(bad(&format!("{rest} is too large.")));
            }
            let mut body = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut body).map_err(|_| bad(&format!("{rest} can't be read.")))?;
            overrides.retain(|(existing, _)| existing != &path);
            overrides.push((path, body));
        }
    }

    Ok(Pack {
        name: index.name,
        version: index.version_id,
        summary: index.summary.unwrap_or_default(),
        minecraft,
        software,
        files,
        overrides,
    })
}

fn is_mod_jar(path: &Path) -> bool {
    path.parent() == Some(Path::new("mods")) && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
}

/// Creates the profile now and fills it in a job. Returns that job; its
/// `targetId` is the new profile.
pub async fn import(app: &AppState, owner: &str, bytes: Vec<u8>) -> AppResult<Job> {
    let pack = parse(&bytes)?;
    if pack.software == ServerSoftwareId::Vanilla && pack.files.iter().any(|file| is_mod_jar(&file.path)) {
        return Err(AppError::bad_request("This modpack names no mod loader."));
    }
    let request = CreateProfileRequest {
        name: if pack.version.is_empty() { pack.name.clone() } else { format!("{} {}", pack.name, pack.version) },
        description: pack.summary.clone(),
        software_id: pack.software,
        version: pack.minecraft.clone(),
        memory: ProfileMemory { min_mb: 2048, max_mb: 6144, initial_mb: 2048 },
        cpu_cores: None,
        java_args: app.store.settings().await?.default_java_args,
        auto_start: false,
        restart_on_crash: true,
        crash_threshold: 3,
    };
    let record = crate::api::profiles::new_profile(app, request).await?;
    let profile_id = record.profile.id.clone();
    app.store.insert_profile(&record, owner).await?;
    app.events.changed(ResourceKind::Profiles, Some(profile_id.clone()));

    let job = app.jobs.create(JobKind::Import, &profile_id, format!("Importing {}", pack.name)).await?;
    let snapshot = job.snapshot().await;
    let app = app.clone();
    tokio::spawn(async move {
        let work = fill(&app, &profile_id, pack, &job);
        let _ = run_job(&job, work).await;
        app.events.changed(ResourceKind::Profiles, Some(profile_id));
    });
    Ok(snapshot)
}

async fn fill(app: &AppState, profile_id: &str, pack: Pack, job: &crate::jobs::JobHandle) -> AppResult<((), String)> {
    let profile = app.store.get_profile(profile_id).await?.profile;
    let target = Target::for_profile(&profile).ok();
    let root = app.paths.profile(profile_id);

    // Recognise the mods, so they're named and can be updated later.
    let hashes: Vec<String> = pack.files.iter().map(|file| file.sha512.clone()).collect();
    let mut known = HashMap::new();
    if let Some(target) = &target {
        for provider in app.content.all() {
            if provider.unavailable(target).is_none() {
                if let Ok(found) = provider.identify(&hashes, target).await {
                    for (hash, version) in found {
                        known.entry(hash).or_insert((provider.clone(), version));
                    }
                }
            }
        }
    }

    let total = (pack.files.len() + 1) as f64;
    let mut mods = 0;
    for (index, file) in pack.files.iter().enumerate() {
        let filename = file.path.file_name().and_then(|name| name.to_str()).unwrap_or("file").to_owned();
        job.progress(Some(index as f64 / total), format!("Downloading {filename}")).await;
        let identified = known.get(&file.sha512);
        let file_ref = match identified {
            Some((_, version)) => version.file.clone(),
            None => FileRef {
                url: Some(file.url.clone()),
                sha512: Some(file.sha512.clone()),
                sha1: file.sha1.clone(),
                ..Default::default()
            },
        };
        let blob = app.cache.fetch(&file_ref.artifact(&filename, file.size)?, None).await?;

        match (&target, is_mod_jar(&file.path), safe_filename(&filename)) {
            (Some(target), true, Some(safe_name)) => {
                let mut record = match identified {
                    Some((provider, version)) => {
                        let project = provider.project(&version.version.project_id).await?;
                        ContentRecord::from_version(new_id("content"), &project, version, target.kind, false, now_iso())
                    }
                    None => upload_record(&safe_name, file_ref, file.size, target.kind),
                };
                record.filename = safe_name;
                record.size_bytes = file.size;
                app.store.put_content(profile_id, &record).await?;
                mods += 1;
            }
            _ => {
                let dest = root.join(&file.path);
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::copy(&blob, &dest).await?;
            }
        }
    }

    job.progress(Some((total - 1.0) / total), "Copying overrides").await;
    for (path, body) in pack.overrides {
        if let (Some(target), true) = (&target, is_mod_jar(&path)) {
            let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(safe_filename) else { continue };
            let sha512 = HashAlgo::Sha512.digest(&body);
            let size = body.len() as u64;
            app.cache
                .fetch(
                    &Artifact {
                        filename: name.clone(),
                        source: Source::Inline(body),
                        hash: Some(Hash { algo: HashAlgo::Sha512, hex: sha512.clone() }),
                        size: Some(size),
                        pin: None,
                    },
                    None,
                )
                .await?;
            let record =
                upload_record(&name, FileRef { sha512: Some(sha512), ..Default::default() }, size, target.kind);
            app.store.put_content(profile_id, &record).await?;
            mods += 1;
            continue;
        }
        let dest = root.join(&path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&dest, body).await?;
    }
    let noun = if target.as_ref().is_some_and(|t| t.kind == ModKind::Plugin) { "plugins" } else { "mods" };
    Ok(((), format!("Imported {} with {mods} {noun}", pack.name)))
}

fn upload_record(filename: &str, file: FileRef, size: u64, kind: ModKind) -> ContentRecord {
    ContentRecord {
        id: new_id("content"),
        provider: ContentSource::Upload,
        project_id: None,
        version_id: None,
        slug: None,
        name: filename.trim_end_matches(".jar").to_owned(),
        version_name: String::new(),
        filename: filename.to_owned(),
        size_bytes: size,
        kind,
        author: String::new(),
        description: "From the modpack".into(),
        icon_url: None,
        page_url: None,
        file,
        enabled: true,
        dependency: false,
        dependencies: vec![],
        game_versions: vec![],
        loaders: vec![],
        update: None,
        installed_at: now_iso(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::content::jarmeta::tests::jar;

    pub fn pack(index: serde_json::Value, extra: &[(&str, &str)]) -> Vec<u8> {
        let body = index.to_string();
        let mut files: Vec<(&str, &str)> = vec![("modrinth.index.json", body.as_str())];
        files.extend_from_slice(extra);
        jar(&files)
    }

    pub fn index(files: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "formatVersion": 1, "game": "minecraft", "versionId": "1.2.0", "name": "Cozy Pack",
            "summary": "A small pack", "files": files,
            "dependencies": { "minecraft": "1.21.4", "fabric-loader": "0.16.10" }
        })
    }

    #[test]
    fn reads_the_index_and_overrides() {
        let bytes = pack(
            index(serde_json::json!([
                { "path": "mods/lithium.jar", "hashes": { "sha512": "aa", "sha1": "a" }, "env": { "client": "required", "server": "required" },
                  "downloads": ["https://evil.example/x.jar", "https://cdn.modrinth.com/data/x/lithium.jar"], "fileSize": 10 },
                { "path": "mods/sodium.jar", "hashes": { "sha512": "bb" }, "env": { "client": "required", "server": "unsupported" },
                  "downloads": ["https://cdn.modrinth.com/data/y/sodium.jar"], "fileSize": 10 }
            ])),
            &[
                ("overrides/config/lithium.properties", "a=1"),
                ("server-overrides/config/lithium.properties", "a=2"),
                ("client-overrides/options.txt", "fov=90"),
            ],
        );
        let pack = parse(&bytes).unwrap();
        assert_eq!((pack.software, pack.minecraft.as_str()), (ServerSoftwareId::Fabric, "1.21.4"));
        assert_eq!(pack.files.len(), 1, "client-only mods are skipped");
        assert_eq!(pack.files[0].url, "https://cdn.modrinth.com/data/x/lithium.jar", "only allowed hosts");
        assert_eq!(pack.overrides, vec![(PathBuf::from("config/lithium.properties"), b"a=2".to_vec())]);
    }

    #[test]
    fn refuses_paths_that_escape() {
        for bad in ["../evil.jar", "/etc/passwd", "mods/../../x", "a\\b", ""] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
        assert_eq!(safe_relative("config/a.toml"), Some(PathBuf::from("config/a.toml")));
        let bytes = pack(
            index(serde_json::json!([
                { "path": "../../evil.jar", "hashes": { "sha512": "aa" }, "downloads": ["https://cdn.modrinth.com/a.jar"], "fileSize": 1 }
            ])),
            &[],
        );
        assert!(parse(&bytes).is_err());
    }

    #[test]
    fn refuses_quilt_and_junk() {
        let mut quilt = index(serde_json::json!([]));
        quilt["dependencies"] = serde_json::json!({ "minecraft": "1.21.4", "quilt-loader": "0.27" });
        assert!(parse(&pack(quilt, &[])).unwrap_err().to_string().contains("Quilt"));
        assert!(parse(b"nope").is_err());
    }
}
