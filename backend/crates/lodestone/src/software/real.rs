//! Real server software, resolved from each publisher at runtime.
//!
//! | Software | Versions from | Jar from | Verified by |
//! |---|---|---|---|
//! | Vanilla | Mojang piston-meta manifest | `downloads.server` in the version JSON | sha1 |
//! | Paper | PaperMC Fill v3 | latest STABLE build's `server:default` | sha256 |
//! | Fabric | Fabric meta (stable game versions) | the generated server launcher | pinned on first download |
//!
//! | Forge (1.17+) | `promotions_slim.json` (recommended, else latest) | the installer from Forge's Maven | its `.sha1` |
//! | NeoForge | NeoForged Maven metadata (newest stable per Minecraft version) | the installer | its `.sha1` |
//!
//! Forge and NeoForge installers run once per version (see
//! `supervisor::install_loader`); servers launch from what they write.

use std::time::Duration;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::{java_for_minecraft, newest_first, Launch, SoftwareProvider};
use crate::cache::{Artifact, Hash, HashAlgo, Source};
use crate::error::{AppError, AppResult};
use crate::model::{ServerSoftwareId, SoftwareCatalog, SoftwareOption, SoftwareVersion};
use crate::store::Store;

/// Lists change a few times a week; builds are immutable once published.
const CATALOG_TTL_SECS: i64 = 6 * 3600;
const IMMUTABLE_TTL_SECS: i64 = i64::MAX / 2;

/// Base URLs, overridable so tests can point at a mock server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub mojang_manifest: String,
    pub paper: String,
    pub fabric: String,
    /// `promotions_slim.json`.
    pub forge_promotions: String,
    pub forge_maven: String,
    pub neoforge_maven: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            mojang_manifest: "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".into(),
            paper: "https://fill.papermc.io/v3".into(),
            fabric: "https://meta.fabricmc.net/v2".into(),
            forge_promotions: "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json".into(),
            forge_maven: "https://maven.minecraftforge.net/net/minecraftforge/forge".into(),
            neoforge_maven: "https://maven.neoforged.net/releases/net/neoforged/neoforge".into(),
        }
    }
}

#[derive(Deserialize)]
struct ForgePromotions {
    promos: std::collections::BTreeMap<String, String>,
}

/// The Minecraft version a NeoForge version is for: `21.1.77` is for
/// 1.21.1, `21.0.x` for 1.21, `26.1.2.123` for 26.1.2, `26.3.0.99` for 26.3.
pub fn neoforge_minecraft(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.split('-').next()?.split('.').collect();
    let major: u32 = parts.first()?.parse().ok()?;
    if major >= 26 {
        let (minor, patch) = (parts.get(1)?, parts.get(2)?);
        Some(if *patch == "0" { format!("{major}.{minor}") } else { format!("{major}.{minor}.{patch}") })
    } else {
        let minor = parts.get(1)?;
        Some(if *minor == "0" { format!("1.{major}") } else { format!("1.{major}.{minor}") })
    }
}

/// Newest stable NeoForge per Minecraft version (a beta only where nothing
/// stable exists yet), newest Minecraft first.
pub fn neoforge_latest(metadata: &str) -> Vec<(String, String, bool)> {
    let mut versions: Vec<&str> =
        metadata.split("<version>").skip(1).filter_map(|chunk| chunk.split("</version>").next()).collect();
    versions.sort_by(|a, b| newest_first(a, b));
    let mut out: Vec<(String, String, bool)> = vec![];
    for version in versions {
        let Some(minecraft) = neoforge_minecraft(version) else { continue };
        let stable = !version.contains('-');
        match out.iter_mut().find(|(mc, _, _)| *mc == minecraft) {
            Some(existing) if !existing.2 && stable => *existing = (minecraft, version.to_owned(), true),
            Some(_) => {}
            None => out.push((minecraft, version.to_owned(), stable)),
        }
    }
    out.sort_by(|a, b| newest_first(&a.0, &b.0));
    out
}

pub struct RealSoftware {
    http: reqwest::Client,
    store: Store,
    endpoints: Endpoints,
}

/* ------------------------------- wire shapes ------------------------------ */

#[derive(Deserialize)]
struct MojangManifest {
    versions: Vec<MojangVersionRef>,
}

#[derive(Deserialize)]
struct MojangVersionRef {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MojangVersion {
    java_version: Option<MojangJava>,
    downloads: MojangDownloads,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MojangJava {
    major_version: u32,
}

#[derive(Deserialize)]
struct MojangDownloads {
    server: Option<MojangDownload>,
}

#[derive(Deserialize)]
struct MojangDownload {
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Deserialize)]
struct PaperProject {
    /// Version groups (`"1.21": ["1.21.4", "1.21.3", ...]`), newest first.
    versions: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct PaperVersion {
    version: PaperVersionInfo,
}

#[derive(Deserialize)]
struct PaperVersionInfo {
    java: Option<PaperJava>,
}

#[derive(Deserialize)]
struct PaperJava {
    version: PaperJavaVersion,
}

#[derive(Deserialize)]
struct PaperJavaVersion {
    minimum: u32,
}

#[derive(Deserialize)]
struct PaperBuild {
    id: u64,
    channel: String,
    downloads: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct PaperDownload {
    name: String,
    checksums: PaperChecksums,
    size: Option<u64>,
    url: String,
}

#[derive(Deserialize)]
struct PaperChecksums {
    sha256: String,
}

#[derive(Deserialize)]
struct FabricVersion {
    version: String,
    stable: bool,
}

/* -------------------------------- provider -------------------------------- */

impl RealSoftware {
    pub fn new(http: reqwest::Client, store: Store, endpoints: Endpoints) -> Self {
        Self { http, store, endpoints }
    }

    /// GET JSON with a cache in SQLite. Stale cache is served when the
    /// publisher is unreachable, so a flaky network does not break the UI.
    async fn get_json<T: DeserializeOwned>(&self, url: &str, ttl_secs: i64) -> AppResult<T> {
        let key = format!("GET {url}");
        if let Some(body) = self.store.cache_get(&key, ttl_secs).await? {
            if let Ok(value) = serde_json::from_str(&body) {
                return Ok(value);
            }
        }
        let fetched = async {
            let response = self.http.get(url).timeout(Duration::from_secs(20)).send().await?.error_for_status()?;
            response.text().await
        }
        .await;
        match fetched {
            Ok(body) => {
                let value = serde_json::from_str(&body)
                    .map_err(|err| AppError::Internal(anyhow::anyhow!("unexpected response from {url}: {err}")))?;
                self.store.cache_put(&key, &body).await?;
                Ok(value)
            }
            Err(err) => match self.store.cache_get(&key, i64::MAX / 2).await? {
                Some(stale) => {
                    tracing::warn!(%url, error = %err, "publisher unreachable; using cached response");
                    Ok(serde_json::from_str(&stale)?)
                }
                None => Err(AppError::Internal(anyhow::anyhow!("could not reach {url}: {err}"))),
            },
        }
    }

    /// GET text with the same cache (Maven metadata, `.sha1` files).
    async fn get_text(&self, url: &str, ttl_secs: i64) -> AppResult<String> {
        let key = format!("GET {url}");
        if let Some(body) = self.store.cache_get(&key, ttl_secs).await? {
            return Ok(body);
        }
        let fetched = async {
            let response = self.http.get(url).timeout(Duration::from_secs(20)).send().await?.error_for_status()?;
            response.text().await
        }
        .await;
        match fetched {
            Ok(body) => {
                self.store.cache_put(&key, &body).await?;
                Ok(body)
            }
            Err(err) => match self.store.cache_get(&key, i64::MAX / 2).await? {
                Some(stale) => Ok(stale),
                None => Err(AppError::Internal(anyhow::anyhow!("could not reach {url}: {err}"))),
            },
        }
    }

    /// Forge's promoted build per Minecraft version (recommended, else latest), 1.17 and up.
    async fn forge_builds(&self) -> AppResult<Vec<(String, String, bool)>> {
        let promotions: ForgePromotions = self.get_json(&self.endpoints.forge_promotions, CATALOG_TTL_SECS).await?;
        let mut out: Vec<(String, String, bool)> = vec![];
        for (key, build) in &promotions.promos {
            let Some((minecraft, kind)) = key.rsplit_once('-') else { continue };
            if crate::logparse::version_at_least(minecraft, "1.17") != Some(true) {
                continue;
            }
            let recommended = kind == "recommended";
            match out.iter_mut().find(|(mc, _, _)| mc == minecraft) {
                Some(existing) if recommended => *existing = (minecraft.to_owned(), build.clone(), true),
                Some(_) => {}
                None => out.push((minecraft.to_owned(), build.clone(), recommended)),
            }
        }
        out.sort_by(|a, b| newest_first(&a.0, &b.0));
        Ok(out)
    }

    async fn neoforge_builds(&self) -> AppResult<Vec<(String, String, bool)>> {
        let metadata =
            self.get_text(&format!("{}/maven-metadata.xml", self.endpoints.neoforge_maven), CATALOG_TTL_SECS).await?;
        Ok(neoforge_latest(&metadata))
    }

    async fn installer(&self, software: ServerSoftwareId, version: &str) -> AppResult<Launch> {
        let (builds, label) = match software {
            ServerSoftwareId::Forge => (self.forge_builds().await?, "Forge"),
            _ => (self.neoforge_builds().await?, "NeoForge"),
        };
        let (_, build, _) = builds
            .into_iter()
            .find(|(minecraft, _, _)| minecraft == version)
            .ok_or_else(|| AppError::bad_request(format!("{label} has no build for Minecraft {version}.")))?;
        let (artifact_version, url) = match software {
            ServerSoftwareId::Forge => {
                let full = format!("{version}-{build}");
                (full.clone(), format!("{}/{full}/forge-{full}-installer.jar", self.endpoints.forge_maven))
            }
            _ => (build.clone(), format!("{}/{build}/neoforge-{build}-installer.jar", self.endpoints.neoforge_maven)),
        };
        let sha1 = self.get_text(&format!("{url}.sha1"), IMMUTABLE_TTL_SECS).await?;
        let sha1 = sha1.split_whitespace().next().unwrap_or_default().to_owned();
        if sha1.len() != 40 {
            return Err(AppError::Internal(anyhow::anyhow!("{label} published no usable sha1 for {artifact_version}")));
        }
        let java = self.mojang_java(version).await.unwrap_or_else(|_| java_for_minecraft(version));
        let prefix = label.to_lowercase();
        Ok(Launch::Installer {
            installer: Artifact::url(
                format!("{prefix}-{artifact_version}-installer.jar"),
                url,
                Hash { algo: HashAlgo::Sha1, hex: sha1 },
                None,
            ),
            java_major: Some(java),
            key: format!("{prefix}-{artifact_version}"),
        })
    }

    async fn mojang_manifest(&self) -> AppResult<MojangManifest> {
        self.get_json(&self.endpoints.mojang_manifest, CATALOG_TTL_SECS).await
    }

    async fn mojang_version(&self, version: &str) -> AppResult<MojangVersion> {
        let manifest = self.mojang_manifest().await?;
        let entry = manifest
            .versions
            .iter()
            .find(|entry| entry.id == version)
            .ok_or_else(|| AppError::bad_request(format!("Mojang does not publish Minecraft {version}.")))?;
        // Version JSON URLs are content-addressed, so they never change.
        self.get_json(&entry.url, IMMUTABLE_TTL_SECS).await
    }

    async fn mojang_java(&self, version: &str) -> AppResult<u32> {
        Ok(self.mojang_version(version).await?.java_version.map(|java| java.major_version).unwrap_or(8))
    }

    async fn vanilla_versions(&self) -> AppResult<Vec<SoftwareVersion>> {
        let manifest = self.mojang_manifest().await?;
        Ok(manifest
            .versions
            .iter()
            .filter(|entry| entry.kind == "release")
            .map(|entry| SoftwareVersion {
                version: entry.id.clone(),
                java_major: java_for_minecraft(&entry.id),
                stable: true,
            })
            .collect())
    }

    async fn paper_versions(&self) -> AppResult<Vec<SoftwareVersion>> {
        let project: PaperProject =
            self.get_json(&format!("{}/projects/paper", self.endpoints.paper), CATALOG_TTL_SECS).await?;
        Ok(project
            .versions
            .values()
            .filter_map(|group| group.as_array())
            .flatten()
            .filter_map(|version| version.as_str())
            // Release candidates and pre-releases are not offered.
            .filter(|version| !version.contains('-'))
            .map(|version| SoftwareVersion {
                version: version.into(),
                java_major: java_for_minecraft(version),
                stable: true,
            })
            .collect())
    }

    async fn fabric_versions(&self) -> AppResult<Vec<SoftwareVersion>> {
        let games: Vec<FabricVersion> =
            self.get_json(&format!("{}/versions/game", self.endpoints.fabric), CATALOG_TTL_SECS).await?;
        Ok(games
            .into_iter()
            .filter(|game| game.stable)
            .map(|game| SoftwareVersion {
                java_major: java_for_minecraft(&game.version),
                version: game.version,
                stable: true,
            })
            .collect())
    }

    async fn latest_stable(&self, what: &str) -> AppResult<String> {
        let list: Vec<FabricVersion> =
            self.get_json(&format!("{}/versions/{what}", self.endpoints.fabric), CATALOG_TTL_SECS).await?;
        list.iter()
            .find(|entry| entry.stable)
            .or(list.first())
            .map(|entry| entry.version.clone())
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Fabric lists no {what} versions")))
    }

    async fn resolve_vanilla(&self, version: &str) -> AppResult<Launch> {
        let meta = self.mojang_version(version).await?;
        let server = meta
            .downloads
            .server
            .ok_or_else(|| AppError::bad_request(format!("Minecraft {version} has no dedicated server download.")))?;
        Ok(Launch::Jar {
            artifact: Artifact::url(
                format!("minecraft-server-{version}.jar"),
                server.url,
                Hash { algo: HashAlgo::Sha1, hex: server.sha1 },
                Some(server.size),
            ),
            java_major: Some(meta.java_version.map(|java| java.major_version).unwrap_or(8)),
        })
    }

    async fn resolve_paper(&self, version: &str) -> AppResult<Launch> {
        let base = format!("{}/projects/paper/versions/{version}", self.endpoints.paper);
        let builds: Vec<PaperBuild> = self
            .get_json(&format!("{base}/builds"), CATALOG_TTL_SECS)
            .await
            .map_err(|_| AppError::bad_request(format!("Paper does not publish builds for {version}.")))?;
        let build = builds
            .iter()
            .find(|build| build.channel.eq_ignore_ascii_case("STABLE"))
            .or(builds.first())
            .ok_or_else(|| AppError::bad_request(format!("Paper has no builds for {version} yet.")))?;
        let download: PaperDownload =
            build.downloads.get("server:default").cloned().map(serde_json::from_value).transpose()?.ok_or_else(
                || AppError::Internal(anyhow::anyhow!("Paper build {} has no server download", build.id)),
            )?;
        let info: PaperVersion = self.get_json(&base, CATALOG_TTL_SECS).await?;
        let java = info.version.java.map(|java| java.version.minimum).unwrap_or_else(|| java_for_minecraft(version));
        Ok(Launch::Jar {
            artifact: Artifact::url(
                download.name,
                download.url,
                Hash { algo: HashAlgo::Sha256, hex: download.checksums.sha256 },
                download.size,
            ),
            java_major: Some(java),
        })
    }

    async fn resolve_fabric(&self, version: &str) -> AppResult<Launch> {
        let games = self.fabric_versions().await?;
        if !games.iter().any(|game| game.version == version) {
            return Err(AppError::bad_request(format!("Fabric does not support Minecraft {version}.")));
        }
        let loader = self.latest_stable("loader").await?;
        let installer = self.latest_stable("installer").await?;
        let java = self.mojang_java(version).await?;
        let filename = format!("fabric-server-mc.{version}-loader.{loader}-launcher.{installer}.jar");
        let url = format!("{}/versions/loader/{version}/{loader}/{installer}/server/jar", self.endpoints.fabric);
        Ok(Launch::Jar {
            artifact: Artifact {
                filename: filename.clone(),
                source: Source::Url(url),
                // Fabric does not publish a hash for the generated launcher.
                hash: None,
                size: None,
                pin: Some(filename),
            },
            java_major: Some(java),
        })
    }
}

#[async_trait]
impl SoftwareProvider for RealSoftware {
    fn kind(&self) -> &'static str {
        "real"
    }

    async fn catalog(&self) -> AppResult<SoftwareCatalog> {
        use ServerSoftwareId::*;
        let (vanilla, paper, fabric, forge, neoforge) = tokio::join!(
            self.vanilla_versions(),
            self.paper_versions(),
            self.fabric_versions(),
            self.forge_builds(),
            self.neoforge_builds()
        );
        let loader_versions = |builds: AppResult<Vec<(String, String, bool)>>| {
            builds.map(|builds| {
                builds
                    .into_iter()
                    .map(|(version, _, stable)| SoftwareVersion {
                        java_major: java_for_minecraft(&version),
                        version,
                        stable,
                    })
                    .collect::<Vec<_>>()
            })
        };
        let option = |id: ServerSoftwareId, blurb: &str, versions: AppResult<Vec<SoftwareVersion>>| {
            versions.map(|versions| SoftwareOption {
                id,
                label: id.label().into(),
                blurb: blurb.into(),
                kind: id.content_kind(),
                versions,
            })
        };
        let mut software = Vec::new();
        // A publisher being down hides its software instead of failing the list.
        for result in [
            option(Vanilla, "The official server from Mojang. No mods or plugins.", vanilla),
            option(Paper, "High-performance fork with Bukkit/Spigot plugin support.", paper),
            option(Fabric, "Lightweight mod loader that tracks new releases quickly.", fabric),
            option(Forge, "The classic mod loader with the largest back catalogue.", loader_versions(forge)),
            option(Neoforge, "Community fork of Forge for modern modpacks.", loader_versions(neoforge)),
        ] {
            match result {
                Ok(option) => software.push(option),
                Err(err) => tracing::warn!(error = %err, "software catalog source unavailable"),
            }
        }
        if software.is_empty() {
            return Err(AppError::Internal(anyhow::anyhow!(
                "No server software could be listed: Mojang, PaperMC and Fabric are all unreachable."
            )));
        }
        Ok(SoftwareCatalog { software })
    }

    async fn resolve(&self, software: ServerSoftwareId, version: &str) -> AppResult<Launch> {
        match software {
            ServerSoftwareId::Vanilla => self.resolve_vanilla(version).await,
            ServerSoftwareId::Paper => self.resolve_paper(version).await,
            ServerSoftwareId::Fabric => self.resolve_fabric(version).await,
            ServerSoftwareId::Forge | ServerSoftwareId::Neoforge => self.installer(software, version).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn provider(server: &MockServer) -> (RealSoftware, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("test.db")).await.unwrap();
        let endpoints = Endpoints {
            mojang_manifest: format!("{}/mojang/manifest.json", server.uri()),
            paper: format!("{}/paper/v3", server.uri()),
            fabric: format!("{}/fabric/v2", server.uri()),
            forge_promotions: format!("{}/forge/promotions_slim.json", server.uri()),
            forge_maven: format!("{}/forge/maven", server.uri()),
            neoforge_maven: format!("{}/neoforge/maven", server.uri()),
        };
        (RealSoftware::new(reqwest::Client::new(), store, endpoints), dir)
    }

    /// Trimmed from the live APIs (November 2026).
    async fn mount_fixtures(server: &MockServer) {
        let uri = server.uri();
        let mount = |route: &str, body: serde_json::Value| {
            Mock::given(method("GET"))
                .and(path(route.to_owned()))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
        };
        mount(
            "/mojang/manifest.json",
            json!({
                "latest": { "release": "26.3", "snapshot": "26.4-snapshot-7" },
                "versions": [
                    { "id": "26.4-snapshot-7", "type": "snapshot", "url": format!("{uri}/mojang/26.4-snapshot-7.json") },
                    { "id": "26.3", "type": "release", "url": format!("{uri}/mojang/26.3.json") },
                    { "id": "1.21.4", "type": "release", "url": format!("{uri}/mojang/1.21.4.json") }
                ]
            }),
        )
        .mount(server)
        .await;
        mount(
            "/mojang/26.3.json",
            json!({
                "javaVersion": { "component": "java-runtime-epsilon", "majorVersion": 25 },
                "downloads": { "server": { "sha1": "33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c", "size": 62294556,
                    "url": "https://piston-data.mojang.com/v1/objects/33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c/server.jar" } }
            }),
        )
        .mount(server)
        .await;
        mount(
            "/mojang/1.21.4.json",
            json!({
                "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
                "downloads": { "server": { "sha1": "4707d00eb834b446575d89a61a11b5d548d8c001", "size": 56880250,
                    "url": "https://piston-data.mojang.com/v1/objects/4707d00eb834b446575d89a61a11b5d548d8c001/server.jar" } }
            }),
        )
        .mount(server)
        .await;
        mount(
            "/paper/v3/projects/paper",
            json!({ "project": { "id": "paper", "name": "Paper" },
                "versions": { "26.3": ["26.3", "26.3-rc-3"], "1.21": ["1.21.4", "1.21.3"] } }),
        )
        .mount(server)
        .await;
        mount(
            "/paper/v3/projects/paper/versions/1.21.4/builds",
            json!([
                { "id": 233, "channel": "BETA", "downloads": {} },
                { "id": 232, "channel": "STABLE", "downloads": { "server:default": {
                    "name": "paper-1.21.4-232.jar",
                    "checksums": { "sha256": "5ee4f542f628a14c644410b08c94ea42e772ef4d29fe92973636b6813d4eaffc" },
                    "size": 51437498,
                    "url": "https://fill-data.papermc.io/v1/objects/5ee4f542/paper-1.21.4-232.jar" } } }
            ]),
        )
        .mount(server)
        .await;
        mount(
            "/paper/v3/projects/paper/versions/1.21.4",
            json!({ "version": { "id": "1.21.4", "java": { "version": { "minimum": 21 } } } }),
        )
        .mount(server)
        .await;
        mount(
            "/fabric/v2/versions/game",
            json!([{ "version": "26.4-snapshot-7", "stable": false }, { "version": "26.3", "stable": true }, { "version": "1.21.4", "stable": true }]),
        )
        .mount(server)
        .await;
        mount(
            "/fabric/v2/versions/loader",
            json!([{ "version": "0.20.1", "stable": true }, { "version": "0.20.0", "stable": false }]),
        )
        .mount(server)
        .await;
        mount("/fabric/v2/versions/installer", json!([{ "version": "1.1.2", "stable": true }])).mount(server).await;
    }

    #[tokio::test]
    async fn catalog_lists_releases_only() {
        let server = MockServer::start().await;
        mount_fixtures(&server).await;
        let (provider, _dir) = provider(&server).await;
        let catalog = provider.catalog().await.unwrap();
        let ids: Vec<_> = catalog.software.iter().map(|option| option.id).collect();
        assert_eq!(ids, [ServerSoftwareId::Vanilla, ServerSoftwareId::Paper, ServerSoftwareId::Fabric]);

        let vanilla: Vec<_> = catalog.software[0].versions.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(vanilla, ["26.3", "1.21.4"]);
        assert_eq!(catalog.software[0].versions[0].java_major, 25);

        let paper: Vec<_> = catalog.software[1].versions.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(paper, ["26.3", "1.21.4", "1.21.3"]);

        let fabric: Vec<_> = catalog.software[2].versions.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(fabric, ["26.3", "1.21.4"]);
    }

    #[tokio::test]
    async fn resolves_vanilla_with_mojang_hash_and_java() {
        let server = MockServer::start().await;
        mount_fixtures(&server).await;
        let (provider, _dir) = provider(&server).await;
        let Launch::Jar { artifact, java_major } = provider.resolve(ServerSoftwareId::Vanilla, "26.3").await.unwrap()
        else {
            panic!("a plain jar")
        };
        assert_eq!(java_major, Some(25));
        assert_eq!(artifact.hash.unwrap().hex, "33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c");
        assert_eq!(artifact.size, Some(62294556));
        assert!(provider.resolve(ServerSoftwareId::Vanilla, "9.9").await.is_err());
    }

    #[tokio::test]
    async fn resolves_paper_latest_stable_build() {
        let server = MockServer::start().await;
        mount_fixtures(&server).await;
        let (provider, _dir) = provider(&server).await;
        let Launch::Jar { artifact, java_major } = provider.resolve(ServerSoftwareId::Paper, "1.21.4").await.unwrap()
        else {
            panic!("a plain jar")
        };
        assert_eq!(artifact.filename, "paper-1.21.4-232.jar");
        assert_eq!(artifact.hash.unwrap().algo, HashAlgo::Sha256);
        assert_eq!(java_major, Some(21));
    }

    #[tokio::test]
    async fn resolves_fabric_launcher_with_a_pin() {
        let server = MockServer::start().await;
        mount_fixtures(&server).await;
        let (provider, _dir) = provider(&server).await;
        let Launch::Jar { artifact, java_major } = provider.resolve(ServerSoftwareId::Fabric, "1.21.4").await.unwrap()
        else {
            panic!("a plain jar")
        };
        assert_eq!(java_major, Some(21));
        assert!(artifact.hash.is_none());
        assert_eq!(artifact.pin.as_deref(), Some("fabric-server-mc.1.21.4-loader.0.20.1-launcher.1.1.2.jar"));
        match artifact.source {
            Source::Url(url) => assert!(url.ends_with("/versions/loader/1.21.4/0.20.1/1.1.2/server/jar")),
            Source::Inline(_) => panic!("expected a URL"),
        }
    }

    #[tokio::test]
    async fn serves_stale_cache_when_offline() {
        let server = MockServer::start().await;
        mount_fixtures(&server).await;
        let (provider, _dir) = provider(&server).await;
        provider.catalog().await.unwrap();
        // Publishers go away; expire the cache window by asking with TTL 0.
        server.reset().await;
        let manifest: MojangManifest = provider.get_json(&provider.endpoints.mojang_manifest, 0).await.unwrap();
        assert_eq!(manifest.versions.len(), 3);
    }

    #[test]
    fn neoforge_versions_name_their_minecraft() {
        assert_eq!(neoforge_minecraft("21.1.77").as_deref(), Some("1.21.1"));
        assert_eq!(neoforge_minecraft("21.0.167").as_deref(), Some("1.21"));
        assert_eq!(neoforge_minecraft("20.4.237").as_deref(), Some("1.20.4"));
        assert_eq!(neoforge_minecraft("26.1.2.123").as_deref(), Some("26.1.2"));
        assert_eq!(neoforge_minecraft("26.3.0.99").as_deref(), Some("26.3"));
        let latest = neoforge_latest(
            "<versions><version>21.1.76</version><version>21.4.1-beta</version><version>21.1.77</version>\
             <version>26.3.0.99</version><version>26.3.0.100</version></versions>",
        );
        assert_eq!(
            latest,
            [
                ("26.3".to_owned(), "26.3.0.100".to_owned(), true),
                ("1.21.4".to_owned(), "21.4.1-beta".to_owned(), false),
                ("1.21.1".to_owned(), "21.1.77".to_owned(), true),
            ]
        );
    }

    #[tokio::test]
    async fn resolves_forge_and_neoforge_installers() {
        let server = MockServer::start().await;
        mount_fixtures(&server).await;
        let mount = |route: String, body: &str| {
            Mock::given(method("GET")).and(path(route)).respond_with(ResponseTemplate::new(200).set_body_string(body))
        };
        mount(
            "/forge/promotions_slim.json".into(),
            r#"{"promos":{"1.16.5-recommended":"36.2.34","1.21.1-latest":"52.1.1","1.21.1-recommended":"52.1.0"}}"#,
        )
        .mount(&server)
        .await;
        mount("/forge/maven/1.21.1-52.1.0/forge-1.21.1-52.1.0-installer.jar.sha1".into(), &"a".repeat(40))
            .mount(&server)
            .await;
        mount("/neoforge/maven/maven-metadata.xml".into(), "<version>21.1.76</version><version>21.1.77</version>")
            .mount(&server)
            .await;
        mount("/neoforge/maven/21.1.77/neoforge-21.1.77-installer.jar.sha1".into(), &"b".repeat(40))
            .mount(&server)
            .await;
        let (provider, _dir) = provider(&server).await;

        let catalog = provider.catalog().await.unwrap();
        let forge = catalog.software.iter().find(|s| s.id == ServerSoftwareId::Forge).unwrap();
        assert_eq!(forge.versions.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(), ["1.21.1"], "1.17+ only");

        let Launch::Installer { installer, key, .. } =
            provider.resolve(ServerSoftwareId::Forge, "1.21.1").await.unwrap()
        else {
            panic!("Forge runs an installer")
        };
        assert_eq!(key, "forge-1.21.1-52.1.0", "the recommended build");
        assert_eq!(installer.hash.unwrap().hex, "a".repeat(40));

        let Launch::Installer { key, .. } = provider.resolve(ServerSoftwareId::Neoforge, "1.21.1").await.unwrap()
        else {
            panic!("NeoForge runs an installer")
        };
        assert_eq!(key, "neoforge-21.1.77");
        assert!(provider.resolve(ServerSoftwareId::Neoforge, "1.20.1").await.is_err());
    }

    /// Hits the real publishers. `just test-live`.
    #[tokio::test]
    #[ignore = "network: talks to Mojang, PaperMC and Fabric"]
    async fn live_publishers_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("live.db")).await.unwrap();
        let http = reqwest::Client::builder().user_agent("lodestone-tests/0.1").build().unwrap();
        let provider = RealSoftware::new(http, store, Endpoints::default());
        let catalog = provider.catalog().await.unwrap();
        assert_eq!(catalog.software.len(), 5, "every publisher answered");
        for (software, version) in [
            (ServerSoftwareId::Vanilla, "1.21.4"),
            (ServerSoftwareId::Paper, "1.21.4"),
            (ServerSoftwareId::Fabric, "1.21.4"),
        ] {
            let Launch::Jar { java_major, .. } = provider.resolve(software, version).await.unwrap() else {
                panic!("{software:?} is a plain jar")
            };
            assert_eq!(java_major, Some(21), "{software:?}");
        }
        for software in [ServerSoftwareId::Forge, ServerSoftwareId::Neoforge] {
            let Launch::Installer { java_major, .. } = provider.resolve(software, "1.21.1").await.unwrap() else {
                panic!("{software:?} runs an installer")
            };
            assert_eq!(java_major, Some(21), "{software:?}");
        }
    }
}
