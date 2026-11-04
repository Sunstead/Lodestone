//! CurseForge (api.curseforge.com/v1). Needs an API key from
//! console.curseforge.com, set by an admin in Settings or with
//! `LODESTONE_CURSEFORGE_API_KEY`; without one the source shows as
//! unavailable, with the reason.
//!
//! - Minecraft is game 432; mods are class 6, Bukkit plugins class 5.
//! - Loaders: Forge 1, Fabric 4, Quilt 5, NeoForge 6 (`modLoaderType`).
//! - Files carry a sha1. A file whose author disabled third-party
//!   distribution has no `downloadUrl`: it's recorded as a manual download
//!   (its CurseForge page), which the user fetches and uploads, and
//!   Lodestone checks it against the sha1. This is how Prism handles them.
//! - Versions are file ids, which are unique across CurseForge.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{project_hit, ContentProvider, FileRef, ProjectInfo, ResolvedVersion, SearchQuery, SearchSort, Target};
use crate::error::{AppError, AppResult};
use crate::model::{
    ContentDependency, ContentSearchResult, ContentSource, ContentVersion, DependencyKind, ModKind, ServerSoftwareId,
    VersionChannel,
};

pub const API: &str = "https://api.curseforge.com/v1";
const MINECRAFT: u32 = 432;
const CLASS_MODS: u32 = 6;
const CLASS_PLUGINS: u32 = 5;
const LOADERS: [&str; 4] = ["forge", "fabric", "quilt", "neoforge"];

/// The API key, shared with Settings so a new one applies at once.
#[derive(Clone, Default)]
pub struct ApiKey(Arc<RwLock<Option<String>>>);

impl ApiKey {
    pub fn new(value: Option<String>) -> Self {
        let key = ApiKey::default();
        key.set(value);
        key
    }

    pub fn get(&self) -> Option<String> {
        self.0.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set(&self, key: Option<String>) {
        *self.0.write().unwrap_or_else(|p| p.into_inner()) = key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
    }
}

pub struct CurseForge {
    http: reqwest::Client,
    base: String,
    key: ApiKey,
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchPage {
    data: Vec<Mod>,
    pagination: Pagination,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    index: u64,
    page_size: u64,
    total_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mod {
    id: u64,
    name: String,
    slug: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    download_count: f64,
    logo: Option<Logo>,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    links: Links,
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(default)]
    class_id: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Logo {
    thumbnail_url: Option<String>,
}

#[derive(Deserialize)]
struct Author {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Links {
    website_url: Option<String>,
}

#[derive(Deserialize)]
struct Category {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    id: u64,
    mod_id: u64,
    display_name: String,
    file_name: String,
    release_type: u8,
    file_date: String,
    #[serde(default)]
    download_count: f64,
    #[serde(default)]
    file_length: u64,
    download_url: Option<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    hashes: Vec<FileHash>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct FileHash {
    value: String,
    algo: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dependency {
    mod_id: u64,
    relation_type: u8,
}

fn loader_type(software: ServerSoftwareId) -> Option<u8> {
    match software {
        ServerSoftwareId::Forge => Some(1),
        ServerSoftwareId::Fabric => Some(4),
        ServerSoftwareId::Neoforge => Some(6),
        _ => None,
    }
}

fn class_for(target: &Target) -> u32 {
    match target.kind {
        ModKind::Plugin => CLASS_PLUGINS,
        ModKind::Mod => CLASS_MODS,
    }
}

impl Mod {
    fn page(&self) -> String {
        self.links.website_url.clone().unwrap_or_else(|| {
            let class = if self.class_id == Some(CLASS_PLUGINS) { "bukkit-plugins" } else { "mc-mods" };
            format!("https://www.curseforge.com/minecraft/{class}/{}", self.slug)
        })
    }

    fn info(&self) -> ProjectInfo {
        ProjectInfo {
            id: self.id.to_string(),
            slug: self.slug.clone(),
            name: self.name.clone(),
            author: self.authors.first().map(|a| a.name.clone()).unwrap_or_default(),
            description: self.summary.clone(),
            icon_url: self.logo.as_ref().and_then(|logo| logo.thumbnail_url.clone()),
            page_url: self.page(),
        }
    }
}

impl File {
    fn resolve(self, target: &Target, page: &str) -> ResolvedVersion {
        // `gameVersions` mixes Minecraft versions with loader names.
        let (loaders, game_versions): (Vec<String>, Vec<String>) = {
            let mut loaders = vec![];
            let mut versions = vec![];
            for entry in &self.game_versions {
                let lower = entry.to_ascii_lowercase();
                if LOADERS.contains(&lower.as_str()) {
                    loaders.push(lower);
                } else if entry.starts_with(|c: char| c.is_ascii_digit()) {
                    versions.push(entry.clone());
                }
            }
            // Plugins don't name a loader: anything Bukkit-like runs them.
            if target.kind == ModKind::Plugin {
                loaders = target.loaders.clone();
            }
            (loaders, versions)
        };
        let sha1 = self.hashes.iter().find(|hash| hash.algo == 1).map(|hash| hash.value.to_ascii_lowercase());
        let file = match self.download_url {
            Some(url) => FileRef { url: Some(url), sha1, ..Default::default() },
            None => FileRef { manual_url: Some(format!("{page}/files/{}", self.id)), sha1, ..Default::default() },
        };
        let dependencies = self
            .dependencies
            .iter()
            .filter_map(|dep| {
                let kind = match dep.relation_type {
                    1 | 6 => DependencyKind::Embedded,
                    2 => DependencyKind::Optional,
                    3 => DependencyKind::Required,
                    5 => DependencyKind::Incompatible,
                    _ => return None,
                };
                Some(ContentDependency { project_id: Some(dep.mod_id.to_string()), version_id: None, kind })
            })
            .collect();
        ResolvedVersion {
            version: ContentVersion {
                provider: ContentSource::Curseforge,
                id: self.id.to_string(),
                project_id: self.mod_id.to_string(),
                name: self.display_name,
                version_number: self.file_name.trim_end_matches(".jar").to_owned(),
                channel: match self.release_type {
                    2 => VersionChannel::Beta,
                    3 => VersionChannel::Alpha,
                    _ => VersionChannel::Release,
                },
                published_at: self.file_date,
                downloads: self.download_count as u64,
                compatible: target.accepts(&loaders, &game_versions),
                game_versions,
                loaders,
                filename: self.file_name,
                size_bytes: self.file_length,
                dependencies,
            },
            file,
        }
    }
}

impl CurseForge {
    pub fn new(http: reqwest::Client, base: impl Into<String>, key: ApiKey) -> Self {
        Self { http, base: base.into().trim_end_matches('/').to_owned(), key }
    }

    fn key(&self) -> AppResult<String> {
        self.key.get().ok_or_else(|| {
            AppError::conflict("provider_unavailable", "CurseForge needs an API key. An admin can add one in Settings.")
        })
    }

    async fn send<T: serde::de::DeserializeOwned>(&self, request: reqwest::RequestBuilder, what: &str) -> AppResult<T> {
        let response =
            request.header("x-api-key", self.key()?).header("accept", "application/json").send().await.map_err(
                |err| AppError::conflict("provider_unavailable", format!("Couldn't reach CurseForge: {err}")),
            )?;
        let status = response.status();
        match status {
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::UNAUTHORIZED => {
                return Err(AppError::conflict(
                    "provider_unavailable",
                    "CurseForge refused the API key. Check it in Settings.",
                ))
            }
            reqwest::StatusCode::NOT_FOUND => {
                return Err(AppError::NotFound("CurseForge has no such project or file.".into()))
            }
            status if !status.is_success() => {
                return Err(AppError::conflict(
                    "provider_unavailable",
                    format!("CurseForge answered {status} for {what}."),
                ))
            }
            _ => {}
        }
        response.json().await.map_err(|err| AppError::Internal(anyhow::anyhow!("CurseForge {what}: {err}")))
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> AppResult<T> {
        self.send(self.http.get(format!("{}{path}", self.base)).query(query), path).await
    }

    async fn mod_info(&self, id: &str) -> AppResult<Mod> {
        Ok(self.get::<Data<Mod>>(&format!("/mods/{id}"), &[]).await?.data)
    }
}

#[async_trait]
impl ContentProvider for CurseForge {
    fn id(&self) -> ContentSource {
        ContentSource::Curseforge
    }

    fn description(&self) -> &'static str {
        "Mods and Bukkit plugins"
    }

    fn unavailable(&self, _target: &Target) -> Option<String> {
        self.key.get().is_none().then(|| "CurseForge needs an API key. An admin can add one in Settings.".to_owned())
    }

    async fn search(&self, target: &Target, query: &SearchQuery) -> AppResult<ContentSearchResult> {
        let sort = match query.sort {
            SearchSort::Relevance => 2,
            SearchSort::Downloads => 6,
            SearchSort::Updated => 3,
            SearchSort::Newest => 11,
        };
        let mut params = vec![
            ("gameId", MINECRAFT.to_string()),
            ("classId", class_for(target).to_string()),
            ("searchFilter", query.query.clone()),
            ("gameVersion", target.game_version.clone()),
            ("sortField", sort.to_string()),
            ("sortOrder", "desc".to_owned()),
            ("index", query.offset.to_string()),
            ("pageSize", query.limit.min(50).to_string()),
        ];
        if let Some(loader) = loader_type(target.software) {
            params.push(("modLoaderType", loader.to_string()));
        }
        let page: SearchPage = self.get("/mods/search", &params).await?;
        Ok(ContentSearchResult {
            hits: page
                .data
                .into_iter()
                .map(|found| {
                    let categories = found.categories.iter().map(|c| c.name.to_lowercase()).take(3).collect();
                    project_hit(
                        ContentSource::Curseforge,
                        found.info(),
                        found.download_count as u64,
                        categories,
                        target.kind,
                    )
                })
                .collect(),
            // CurseForge won't page past 10,000.
            total: page.pagination.total_count.min(10_000),
            offset: page.pagination.index,
            limit: page.pagination.page_size,
        })
    }

    async fn project(&self, id: &str) -> AppResult<ProjectInfo> {
        Ok(self.mod_info(id).await?.info())
    }

    async fn versions(&self, project_id: &str, target: &Target) -> AppResult<Vec<ResolvedVersion>> {
        let page = self.mod_info(project_id).await?.page();
        let mut params = vec![("gameVersion", target.game_version.clone()), ("pageSize", "50".to_owned())];
        if let Some(loader) = loader_type(target.software) {
            params.push(("modLoaderType", loader.to_string()));
        }
        let files: Data<Vec<File>> = self.get(&format!("/mods/{project_id}/files"), &params).await?;
        let mut versions: Vec<ResolvedVersion> = files
            .data
            .into_iter()
            .map(|file| file.resolve(target, &page))
            .filter(|resolved| resolved.version.compatible)
            .collect();
        versions.sort_by(|a, b| b.version.published_at.cmp(&a.version.published_at));
        Ok(versions)
    }

    async fn version(&self, version_id: &str, target: &Target) -> AppResult<ResolvedVersion> {
        let id: u64 = version_id.parse().map_err(|_| AppError::NotFound("CurseForge has no such file.".into()))?;
        let files: Data<Vec<File>> = self
            .send(self.http.post(format!("{}/mods/files", self.base)).json(&json!({ "fileIds": [id] })), "/mods/files")
            .await?;
        let file =
            files.data.into_iter().next().ok_or_else(|| AppError::NotFound("CurseForge has no such file.".into()))?;
        let page = self.mod_info(&file.mod_id.to_string()).await?.page();
        Ok(file.resolve(target, &page))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn neoforge() -> Target {
        Target {
            software: ServerSoftwareId::Neoforge,
            kind: ModKind::Mod,
            loaders: vec!["neoforge".into()],
            game_version: "1.21.1".into(),
        }
    }

    /// Shapes from CurseForge's API documentation.
    fn file(id: u64, restricted: bool) -> serde_json::Value {
        json!({
            "id": id, "gameId": 432, "modId": 238222, "isAvailable": true, "displayName": "jei-1.21.1-neoforge-19.21.0.247.jar",
            "fileName": "jei-1.21.1-neoforge-19.21.0.247.jar", "releaseType": 1, "fileStatus": 4,
            "hashes": [{ "value": "ABCDEF0123456789ABCDEF0123456789ABCDEF01", "algo": 1 }, { "value": "md5", "algo": 2 }],
            "fileDate": "2026-09-01T10:00:00Z", "fileLength": 1340000, "downloadCount": 12345.0,
            "downloadUrl": if restricted { serde_json::Value::Null } else { json!(format!("https://edge.forgecdn.net/files/{id}/jei.jar")) },
            "gameVersions": ["1.21.1", "NeoForge", "Server", "Client"],
            "dependencies": [{ "modId": 306612, "relationType": 3 }, { "modId": 1, "relationType": 4 }]
        })
    }

    fn jei() -> serde_json::Value {
        json!({
            "id": 238222, "gameId": 432, "name": "Just Enough Items (JEI)", "slug": "jei", "summary": "View items and recipes",
            "downloadCount": 412000000.0, "classId": 6, "logo": { "thumbnailUrl": "https://media.forgecdn.net/jei.png" },
            "authors": [{ "name": "mezz" }], "links": { "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei" },
            "categories": [{ "name": "API and Library" }]
        })
    }

    #[tokio::test]
    async fn needs_a_key() {
        let key = ApiKey::default();
        let curseforge = CurseForge::new(reqwest::Client::new(), "http://127.0.0.1:9", key.clone());
        assert!(curseforge.unavailable(&neoforge()).unwrap().contains("API key"));
        key.set(Some("  ".into()));
        assert!(curseforge.unavailable(&neoforge()).is_some(), "blank is no key");
        key.set(Some("secret".into()));
        assert!(curseforge.unavailable(&neoforge()).is_none());
    }

    #[tokio::test]
    async fn reads_search_files_and_restricted_downloads() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/mods/search"))
            .and(header("x-api-key", "secret"))
            .and(query_param("classId", "6"))
            .and(query_param("modLoaderType", "6"))
            .and(query_param("gameVersion", "1.21.1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [jei()], "pagination": { "index": 0, "pageSize": 20, "resultCount": 1, "totalCount": 1 }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/mods/238222"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": jei() })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/mods/238222/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [file(5001, false)] })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/mods/files"))
            .and(body_json(json!({ "fileIds": [5002] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [file(5002, true)] })))
            .mount(&server)
            .await;

        let curseforge = CurseForge::new(reqwest::Client::new(), server.uri(), ApiKey::new(Some("secret".into())));
        let query = SearchQuery { query: "jei".into(), offset: 0, limit: 20, sort: SearchSort::Relevance };
        let found = curseforge.search(&neoforge(), &query).await.unwrap();
        let hit = &found.hits[0];
        assert_eq!((hit.id.as_str(), hit.author.as_str(), hit.downloads), ("238222", "mezz", 412_000_000));

        let versions = curseforge.versions("238222", &neoforge()).await.unwrap();
        let v = &versions[0];
        assert!(v.version.compatible, "{:?}", v.version);
        assert_eq!(v.version.loaders, ["neoforge"]);
        assert_eq!(v.version.game_versions, ["1.21.1"], "loader and side tags aren't versions");
        assert_eq!(v.file.sha1.as_deref(), Some("abcdef0123456789abcdef0123456789abcdef01"));
        assert_eq!(v.version.dependencies.len(), 1, "tools are dropped");
        assert_eq!(v.version.dependencies[0].kind, DependencyKind::Required);

        let restricted = curseforge.version("5002", &neoforge()).await.unwrap();
        assert!(restricted.file.is_manual());
        assert_eq!(
            restricted.file.manual_url.as_deref(),
            Some("https://www.curseforge.com/minecraft/mc-mods/jei/files/5002")
        );
    }

    #[tokio::test]
    async fn a_refused_key_says_so() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
        let curseforge = CurseForge::new(reqwest::Client::new(), server.uri(), ApiKey::new(Some("bad".into())));
        let err = curseforge.project("1").await.unwrap_err();
        assert!(err.to_string().contains("refused the API key"), "{err}");
    }
}
