//! Hangar (hangar.papermc.io/api/v1): PaperMC's plugin repository. Paper
//! configurations only. No key needed.
//!
//! - Projects and versions are numeric ids; `/versions/{id}` resolves one.
//! - Files carry a sha256. A version hosted elsewhere (`externalUrl`, no
//!   `downloadUrl`) is a manual download: the user fetches and uploads it.
//! - Dependencies on other Hangar projects (by `projectId`) are followed;
//!   ones only named by an external link are not.

use async_trait::async_trait;
use serde::Deserialize;

use super::{project_hit, ContentProvider, FileRef, ProjectInfo, ResolvedVersion, SearchQuery, SearchSort, Target};
use crate::error::{AppError, AppResult};
use crate::model::{
    ContentDependency, ContentSearchResult, ContentSource, ContentVersion, DependencyKind, ModKind, VersionChannel,
};

pub const API: &str = "https://hangar.papermc.io/api/v1";
const SITE: &str = "https://hangar.papermc.io";
const PLATFORM: &str = "PAPER";

pub struct Hangar {
    http: reqwest::Client,
    base: String,
}

#[derive(Deserialize)]
struct Page<T> {
    pagination: Pagination,
    result: Vec<T>,
}

#[derive(Deserialize)]
struct Pagination {
    count: u64,
    offset: u64,
    limit: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: u64,
    name: String,
    namespace: Namespace,
    #[serde(default)]
    stats: ProjectStats,
    #[serde(default)]
    category: String,
    #[serde(default)]
    description: String,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct Namespace {
    owner: String,
    slug: String,
}

#[derive(Deserialize, Default)]
struct ProjectStats {
    #[serde(default)]
    downloads: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    id: u64,
    project_id: u64,
    name: String,
    created_at: String,
    channel: Channel,
    #[serde(default)]
    stats: VersionStats,
    #[serde(default)]
    downloads: std::collections::HashMap<String, Download>,
    #[serde(default)]
    plugin_dependencies: std::collections::HashMap<String, Vec<PluginDependency>>,
    #[serde(default)]
    platform_dependencies: std::collections::HashMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Channel {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct VersionStats {
    #[serde(default)]
    total_downloads: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Download {
    file_info: Option<FileInfo>,
    external_url: Option<String>,
    download_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileInfo {
    name: String,
    size_bytes: u64,
    sha256_hash: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginDependency {
    project_id: Option<u64>,
    #[serde(default)]
    required: bool,
}

fn channel(name: &str) -> VersionChannel {
    let name = name.to_ascii_lowercase();
    if name.contains("alpha") || name.contains("snapshot") || name.contains("dev") {
        VersionChannel::Alpha
    } else if name.contains("beta") || name.contains("pre") {
        VersionChannel::Beta
    } else {
        VersionChannel::Release
    }
}

fn page_url(namespace: &Namespace) -> String {
    format!("{SITE}/{}/{}", namespace.owner, namespace.slug)
}

impl Project {
    fn info(self) -> ProjectInfo {
        ProjectInfo {
            page_url: page_url(&self.namespace),
            id: self.id.to_string(),
            slug: self.namespace.slug,
            name: self.name,
            author: self.namespace.owner,
            description: self.description,
            icon_url: self.avatar_url,
        }
    }
}

impl Version {
    fn resolve(mut self, target: &Target) -> AppResult<ResolvedVersion> {
        let download = self
            .downloads
            .remove(PLATFORM)
            .ok_or_else(|| AppError::bad_request(format!("Hangar version {} has no Paper download.", self.name)))?;
        let game_versions = self.platform_dependencies.remove(PLATFORM).unwrap_or_default();
        let loaders = vec!["paper".to_owned()];
        let (filename, size, sha256) = match &download.file_info {
            Some(info) => (info.name.clone(), info.size_bytes, info.sha256_hash.clone()),
            None => (format!("{}.jar", self.name), 0, None),
        };
        let file = match (&download.download_url, &download.external_url) {
            (Some(url), _) => FileRef { url: Some(url.clone()), sha256, ..Default::default() },
            (None, Some(external)) => FileRef { manual_url: Some(external.clone()), sha256, ..Default::default() },
            (None, None) => return Err(AppError::bad_request(format!("Hangar version {} has no file.", self.name))),
        };
        let dependencies = self
            .plugin_dependencies
            .remove(PLATFORM)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|dep| {
                Some(ContentDependency {
                    project_id: Some(dep.project_id?.to_string()),
                    version_id: None,
                    kind: if dep.required { DependencyKind::Required } else { DependencyKind::Optional },
                })
            })
            .collect();
        Ok(ResolvedVersion {
            version: ContentVersion {
                provider: ContentSource::Hangar,
                id: self.id.to_string(),
                project_id: self.project_id.to_string(),
                name: self.name.clone(),
                version_number: self.name,
                channel: channel(&self.channel.name),
                published_at: self.created_at,
                downloads: self.stats.total_downloads,
                compatible: target.accepts(&loaders, &game_versions),
                game_versions,
                loaders,
                filename,
                size_bytes: size,
                dependencies,
            },
            file,
        })
    }
}

impl Hangar {
    pub fn new(http: reqwest::Client, base: impl Into<String>) -> Self {
        Self { http, base: base.into().trim_end_matches('/').to_owned() }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> AppResult<T> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .query(query)
            .send()
            .await
            .map_err(|err| AppError::conflict("provider_unavailable", format!("Couldn't reach Hangar: {err}")))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::NotFound("Hangar has no such project or version.".into()));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::conflict(
                "provider_unavailable",
                "Hangar is rate limiting requests. Try again in a minute.",
            ));
        }
        if !status.is_success() {
            return Err(AppError::conflict("provider_unavailable", format!("Hangar answered {status} for {path}.")));
        }
        response.json().await.map_err(|err| AppError::Internal(anyhow::anyhow!("Hangar {path}: {err}")))
    }
}

#[async_trait]
impl ContentProvider for Hangar {
    fn id(&self) -> ContentSource {
        ContentSource::Hangar
    }

    fn description(&self) -> &'static str {
        "Paper plugins"
    }

    fn unavailable(&self, target: &Target) -> Option<String> {
        (target.kind != ModKind::Plugin).then(|| "Hangar only has Paper plugins.".to_owned())
    }

    async fn search(&self, target: &Target, query: &SearchQuery) -> AppResult<ContentSearchResult> {
        let mut params = vec![
            ("q", query.query.clone()),
            ("platform", PLATFORM.to_owned()),
            ("version", target.game_version.clone()),
            ("offset", query.offset.to_string()),
            ("limit", query.limit.min(25).to_string()),
        ];
        let sort = match query.sort {
            SearchSort::Relevance if !query.query.is_empty() => None,
            SearchSort::Relevance | SearchSort::Downloads => Some("-downloads"),
            SearchSort::Updated => Some("-updated"),
            SearchSort::Newest => Some("-newest"),
        };
        if let Some(sort) = sort {
            params.push(("sort", sort.to_owned()));
        }
        let page: Page<Project> = self.get("/projects", &params).await?;
        Ok(ContentSearchResult {
            hits: page
                .result
                .into_iter()
                .map(|project| {
                    let (downloads, category) = (project.stats.downloads, project.category.replace('_', " "));
                    project_hit(ContentSource::Hangar, project.info(), downloads, vec![category], ModKind::Plugin)
                })
                .collect(),
            total: page.pagination.count,
            offset: page.pagination.offset,
            limit: page.pagination.limit,
        })
    }

    async fn project(&self, id: &str) -> AppResult<ProjectInfo> {
        Ok(self.get::<Project>(&format!("/projects/{id}"), &[]).await?.info())
    }

    async fn versions(&self, project_id: &str, target: &Target) -> AppResult<Vec<ResolvedVersion>> {
        let page: Page<Version> = self
            .get(
                &format!("/projects/{project_id}/versions"),
                &[
                    ("platform", PLATFORM.to_owned()),
                    ("platformVersion", target.game_version.clone()),
                    ("limit", "25".to_owned()),
                ],
            )
            .await?;
        Ok(page
            .result
            .into_iter()
            .filter_map(|version| version.resolve(target).ok())
            .filter(|resolved| resolved.version.compatible)
            .collect())
    }

    async fn version(&self, version_id: &str, target: &Target) -> AppResult<ResolvedVersion> {
        self.get::<Version>(&format!("/versions/{version_id}"), &[]).await?.resolve(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ServerSoftwareId;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn paper() -> Target {
        Target {
            software: ServerSoftwareId::Paper,
            kind: ModKind::Plugin,
            loaders: Target::loaders_for(ServerSoftwareId::Paper),
            game_version: "1.21.4".into(),
        }
    }

    /// Trimmed from the live API (November 2026).
    fn version(id: u64, name: &str, external: bool) -> serde_json::Value {
        let download = if external {
            json!({ "fileInfo": null, "externalUrl": "https://github.com/x/releases", "downloadUrl": null })
        } else {
            json!({
                "fileInfo": { "name": format!("flags-{name}.jar"), "sizeBytes": 801523, "sha256Hash": "8db4" },
                "externalUrl": null,
                "downloadUrl": format!("https://hangarcdn.papermc.io/plugins/tins/Flags/versions/{name}/PAPER/flags-{name}.jar")
            })
        };
        json!({
            "createdAt": "2026-10-01T05:47:04Z", "id": id, "projectId": 4948, "name": name,
            "channel": { "name": "Release" }, "stats": { "totalDownloads": 81 },
            "downloads": { "PAPER": download },
            "pluginDependencies": { "PAPER": [
                { "name": "ProtocolLib", "projectId": 433, "required": false, "externalUrl": null, "platform": "PAPER" },
                { "name": "WorldGuard", "projectId": null, "required": true, "externalUrl": "https://enginehub.org", "platform": "PAPER" }
            ]},
            "platformDependencies": { "PAPER": ["1.21.1", "1.21.4"] }
        })
    }

    #[tokio::test]
    async fn reads_search_versions_and_external_files() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/projects"))
            .and(query_param("platform", "PAPER"))
            .and(query_param("version", "1.21.4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "pagination": { "count": 1, "limit": 20, "offset": 0 },
                "result": [{
                    "id": 4948, "name": "Flags", "namespace": { "owner": "tins", "slug": "Flags" },
                    "stats": { "downloads": 1161 }, "category": "world_management",
                    "description": "More flags", "avatarUrl": "https://hangarcdn.papermc.io/avatars/project/4948.webp"
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/projects/4948/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "pagination": { "count": 1, "limit": 25, "offset": 0 }, "result": [version(31112, "4.4.6", false)]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/versions/9"))
            .respond_with(ResponseTemplate::new(200).set_body_json(version(9, "1.0", true)))
            .mount(&server)
            .await;

        let hangar = Hangar::new(reqwest::Client::new(), server.uri());
        let query = SearchQuery { query: "flags".into(), offset: 0, limit: 20, sort: SearchSort::Relevance };
        let found = hangar.search(&paper(), &query).await.unwrap();
        let hit = &found.hits[0];
        assert_eq!((hit.id.as_str(), hit.page_url.as_str()), ("4948", "https://hangar.papermc.io/tins/Flags"));
        assert_eq!(hit.categories, ["world management"]);

        let versions = hangar.versions("4948", &paper()).await.unwrap();
        let v = &versions[0];
        assert!(v.version.compatible);
        assert_eq!(v.file.sha256.as_deref(), Some("8db4"));
        assert_eq!(v.version.filename, "flags-4.4.6.jar");
        assert_eq!(
            v.version.dependencies,
            [ContentDependency { project_id: Some("433".into()), version_id: None, kind: DependencyKind::Optional }],
            "only dependencies that are Hangar projects"
        );

        let external = hangar.version("9", &paper()).await.unwrap();
        assert!(external.file.is_manual());
        assert_eq!(external.file.manual_url.as_deref(), Some("https://github.com/x/releases"));
    }

    #[test]
    fn hangar_is_for_paper_only() {
        let hangar = Hangar::new(reqwest::Client::new(), API);
        let fabric = Target {
            software: ServerSoftwareId::Fabric,
            kind: ModKind::Mod,
            loaders: vec!["fabric".into()],
            game_version: "1.21.4".into(),
        };
        assert!(hangar.unavailable(&fabric).is_some());
        assert!(hangar.unavailable(&paper()).is_none());
    }

    /// `just test-live`.
    #[tokio::test]
    #[ignore = "network: talks to Hangar"]
    async fn live_hangar() {
        let http = reqwest::Client::builder().user_agent("lodestone-tests").build().unwrap();
        let hangar = Hangar::new(http, API);
        let query = SearchQuery { query: "".into(), offset: 0, limit: 5, sort: SearchSort::Downloads };
        let found = hangar.search(&paper(), &query).await.unwrap();
        assert!(!found.hits.is_empty());
        let versions = hangar.versions(&found.hits[0].id, &paper()).await.unwrap();
        let latest = versions.first().expect("the most downloaded plugin supports 1.21.4");
        assert!(latest.file.sha256.is_some() || latest.file.is_manual());
        let again = hangar.version(&latest.version.id, &paper()).await.unwrap();
        assert_eq!(again.version.id, latest.version.id);
    }
}
