//! Modrinth (api.modrinth.com/v2). No key needed; requests carry a
//! `lodestone/<version>` user agent, as Modrinth asks.
//!
//! - Search: facets from the profile (project type, loader, Minecraft
//!   version, runs on servers).
//! - Versions: `/project/{id}/version` filtered by loader and version.
//! - Updates: `POST /version_files/update` by sha512, like Prism.
//! - Uploads and modpacks: `POST /version_files` to recognise files.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{project_hit, ContentProvider, FileRef, ProjectInfo, ResolvedVersion, SearchQuery, SearchSort, Target};
use crate::error::{AppError, AppResult};
use crate::model::{
    ContentDependency, ContentSearchResult, ContentSource, ContentVersion, DependencyKind, ModKind, VersionChannel,
};

pub const API: &str = "https://api.modrinth.com/v2";

pub struct Modrinth {
    http: reqwest::Client,
    base: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
    total_hits: u64,
    offset: u64,
    limit: u64,
}

#[derive(Deserialize)]
struct SearchHit {
    project_id: String,
    slug: String,
    title: String,
    author: String,
    description: String,
    #[serde(default)]
    display_categories: Vec<String>,
    #[serde(default)]
    downloads: u64,
    icon_url: Option<String>,
}

#[derive(Deserialize)]
struct Project {
    id: String,
    slug: String,
    title: String,
    description: String,
    icon_url: Option<String>,
    team: String,
    #[serde(default)]
    organization: Option<String>,
}

#[derive(Deserialize)]
struct TeamMember {
    user: TeamUser,
    #[serde(default)]
    role: String,
    #[serde(default)]
    ordering: i64,
}

#[derive(Deserialize)]
struct TeamUser {
    username: String,
}

#[derive(Deserialize)]
struct Organization {
    name: String,
}

#[derive(Deserialize)]
struct Version {
    id: String,
    project_id: String,
    name: String,
    version_number: String,
    version_type: String,
    date_published: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    files: Vec<VersionFile>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct VersionFile {
    url: String,
    filename: String,
    size: u64,
    #[serde(default)]
    primary: bool,
    hashes: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Dependency {
    version_id: Option<String>,
    project_id: Option<String>,
    dependency_type: String,
}

fn channel(raw: &str) -> VersionChannel {
    match raw {
        "beta" => VersionChannel::Beta,
        "alpha" => VersionChannel::Alpha,
        _ => VersionChannel::Release,
    }
}

fn dependency_kind(raw: &str) -> Option<DependencyKind> {
    Some(match raw {
        "required" => DependencyKind::Required,
        "optional" => DependencyKind::Optional,
        "incompatible" => DependencyKind::Incompatible,
        "embedded" => DependencyKind::Embedded,
        _ => return None,
    })
}

impl Version {
    fn resolve(self, target: &Target) -> AppResult<ResolvedVersion> {
        let compatible = target.accepts(&self.loaders, &self.game_versions);
        let file = self
            .files
            .iter()
            .position(|file| file.primary)
            .or_else(|| self.files.iter().position(|file| file.filename.ends_with(".jar")))
            .and_then(|index| self.files.into_iter().nth(index))
            .ok_or_else(|| AppError::bad_request(format!("{} has no file to download.", self.name)))?;
        let sha512 = file.hashes.get("sha512").cloned().ok_or_else(|| {
            AppError::bad_request(format!("{} has no sha512, so it can't be verified.", file.filename))
        })?;
        Ok(ResolvedVersion {
            version: ContentVersion {
                provider: ContentSource::Modrinth,
                id: self.id,
                project_id: self.project_id,
                name: self.name,
                version_number: self.version_number,
                channel: channel(&self.version_type),
                published_at: self.date_published,
                downloads: self.downloads,
                game_versions: self.game_versions,
                loaders: self.loaders,
                filename: file.filename,
                size_bytes: file.size,
                dependencies: self
                    .dependencies
                    .into_iter()
                    .filter_map(|dep| {
                        Some(ContentDependency {
                            project_id: dep.project_id,
                            version_id: dep.version_id,
                            kind: dependency_kind(&dep.dependency_type)?,
                        })
                    })
                    .collect(),
                compatible,
            },
            file: FileRef { url: Some(file.url), sha512, sha1: file.hashes.get("sha1").cloned(), inline: None },
        })
    }
}

impl Modrinth {
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
            .map_err(|err| unreachable_error(&err))?;
        decode(response, path).await
    }

    async fn post<T: serde::de::DeserializeOwned>(&self, path: &str, body: serde_json::Value) -> AppResult<T> {
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .map_err(|err| unreachable_error(&err))?;
        decode(response, path).await
    }

    async fn author(&self, project: &Project) -> String {
        if let Some(org) = &project.organization {
            if let Ok(org) = self.get::<Organization>(&format!("/organization/{org}"), &[]).await {
                return org.name;
            }
        }
        match self.get::<Vec<TeamMember>>(&format!("/team/{}/members", project.team), &[]).await {
            Ok(mut members) => {
                members.sort_by_key(|member| (member.role != "Owner", member.ordering));
                members.into_iter().next().map(|member| member.user.username).unwrap_or_default()
            }
            Err(_) => String::new(),
        }
    }
}

fn unreachable_error(err: &reqwest::Error) -> AppError {
    AppError::Conflict { code: "provider_unavailable", message: format!("Couldn't reach Modrinth: {err}") }
}

async fn decode<T: serde::de::DeserializeOwned>(response: reqwest::Response, path: &str) -> AppResult<T> {
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::NotFound("Modrinth has no such project or version.".into()));
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(AppError::conflict(
            "provider_unavailable",
            "Modrinth is rate limiting requests. Try again in a minute.",
        ));
    }
    if !status.is_success() {
        return Err(AppError::conflict("provider_unavailable", format!("Modrinth answered {status} for {path}.")));
    }
    response.json().await.map_err(|err| AppError::Internal(anyhow::anyhow!("Modrinth {path}: {err}")))
}

fn page_url(slug: &str) -> String {
    format!("https://modrinth.com/project/{slug}")
}

fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).expect("strings serialise")
}

/// Search facets: AND across the outer list, OR within each inner list.
pub fn facets(target: &Target) -> String {
    let project_type = match target.kind {
        ModKind::Plugin => "project_type:plugin",
        ModKind::Mod => "project_type:mod",
    };
    let loaders: Vec<String> = target.loaders.iter().map(|loader| format!("categories:{loader}")).collect();
    let value = json!([
        [project_type],
        loaders,
        [format!("versions:{}", target.game_version)],
        ["server_side:required", "server_side:optional"],
    ]);
    value.to_string()
}

#[async_trait]
impl ContentProvider for Modrinth {
    fn id(&self) -> ContentSource {
        ContentSource::Modrinth
    }

    async fn search(&self, target: &Target, query: &SearchQuery) -> AppResult<ContentSearchResult> {
        let index = match query.sort {
            SearchSort::Relevance => "relevance",
            SearchSort::Downloads => "downloads",
            SearchSort::Updated => "updated",
            SearchSort::Newest => "newest",
        };
        let response: SearchResponse = self
            .get(
                "/search",
                &[
                    ("query", query.query.clone()),
                    ("facets", facets(target)),
                    ("index", index.into()),
                    ("offset", query.offset.to_string()),
                    ("limit", query.limit.min(100).to_string()),
                ],
            )
            .await?;
        Ok(ContentSearchResult {
            hits: response
                .hits
                .into_iter()
                .map(|hit| {
                    let info = ProjectInfo {
                        page_url: page_url(&hit.slug),
                        id: hit.project_id,
                        slug: hit.slug,
                        name: hit.title,
                        author: hit.author,
                        description: hit.description,
                        icon_url: hit.icon_url,
                    };
                    let categories =
                        hit.display_categories.into_iter().filter(|cat| !target.loaders.contains(cat)).collect();
                    project_hit(ContentSource::Modrinth, info, hit.downloads, categories, target.kind)
                })
                .collect(),
            total: response.total_hits,
            offset: response.offset,
            limit: response.limit,
        })
    }

    async fn project(&self, id: &str) -> AppResult<ProjectInfo> {
        let project: Project = self.get(&format!("/project/{id}"), &[]).await?;
        let author = self.author(&project).await;
        Ok(ProjectInfo {
            page_url: page_url(&project.slug),
            id: project.id,
            slug: project.slug,
            name: project.title,
            author,
            description: project.description,
            icon_url: project.icon_url,
        })
    }

    async fn versions(&self, project_id: &str, target: &Target) -> AppResult<Vec<ResolvedVersion>> {
        let versions: Vec<Version> = self
            .get(
                &format!("/project/{project_id}/version"),
                &[
                    ("loaders", json_list(&target.loaders)),
                    ("game_versions", json_list(std::slice::from_ref(&target.game_version))),
                ],
            )
            .await?;
        Ok(versions.into_iter().filter_map(|version| version.resolve(target).ok()).collect())
    }

    async fn version(&self, version_id: &str, target: &Target) -> AppResult<ResolvedVersion> {
        let version: Version = self.get(&format!("/version/{version_id}"), &[]).await?;
        version.resolve(target)
    }

    async fn latest_for_hashes(
        &self,
        sha512: &[String],
        target: &Target,
    ) -> AppResult<HashMap<String, ResolvedVersion>> {
        if sha512.is_empty() {
            return Ok(HashMap::new());
        }
        let found: HashMap<String, Version> = self
            .post(
                "/version_files/update",
                json!({
                    "hashes": sha512,
                    "algorithm": "sha512",
                    "loaders": target.loaders,
                    "game_versions": [target.game_version],
                }),
            )
            .await?;
        Ok(found.into_iter().filter_map(|(hash, version)| Some((hash, version.resolve(target).ok()?))).collect())
    }

    async fn identify(&self, sha512: &[String], target: &Target) -> AppResult<HashMap<String, ResolvedVersion>> {
        if sha512.is_empty() {
            return Ok(HashMap::new());
        }
        let found: HashMap<String, Version> =
            self.post("/version_files", json!({ "hashes": sha512, "algorithm": "sha512" })).await?;
        // A version can carry several files; report the one with this hash.
        let mut out = HashMap::new();
        for (hash, version) in found {
            let Some(file) = version.files.iter().find(|file| file.hashes.get("sha512") == Some(&hash)) else {
                continue;
            };
            let (filename, size) = (file.filename.clone(), file.size);
            let Ok(mut resolved) = version.resolve(target) else { continue };
            resolved.version.filename = filename;
            resolved.version.size_bytes = size;
            resolved.file.sha512 = hash.clone();
            out.insert(hash, resolved);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ServerSoftwareId;
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

    fn version_json(id: &str, number: &str, deps: serde_json::Value) -> serde_json::Value {
        json!({
            "id": id, "project_id": "DKY9btbd", "name": format!("WorldGuard {number}"), "version_number": number,
            "version_type": "release", "date_published": "2025-02-16T22:47:14Z", "downloads": 120669,
            "game_versions": ["1.21.4"], "loaders": ["bukkit", "paper", "spigot"], "status": "listed",
            "dependencies": deps,
            "files": [
                { "url": "https://cdn.modrinth.com/data/DKY9btbd/versions/x/worldguard-sources.jar", "filename": "worldguard-sources.jar", "size": 10, "primary": false, "hashes": { "sha512": "bb", "sha1": "b" } },
                { "url": format!("https://cdn.modrinth.com/data/DKY9btbd/versions/{id}/worldguard-bukkit-{number}.jar"), "filename": format!("worldguard-bukkit-{number}.jar"), "size": 1843200, "primary": true, "hashes": { "sha512": "aa", "sha1": "a" } }
            ]
        })
    }

    #[test]
    fn facets_follow_the_profile() {
        let facets: serde_json::Value = serde_json::from_str(&facets(&paper())).unwrap();
        assert_eq!(
            facets,
            json!([
                ["project_type:plugin"],
                ["categories:paper", "categories:spigot", "categories:bukkit"],
                ["versions:1.21.4"],
                ["server_side:required", "server_side:optional"]
            ])
        );
    }

    #[tokio::test]
    async fn reads_search_projects_and_versions() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("query", "guard"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "hits": [{
                    "project_id": "DKY9btbd", "project_type": "mod", "slug": "worldguard", "author": "me4502",
                    "title": "WorldGuard", "description": "Protect regions", "display_categories": ["paper", "management"],
                    "downloads": 1046600, "icon_url": null, "versions": ["1.21.4"]
                }],
                "offset": 0, "limit": 20, "total_hits": 1
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/project/DKY9btbd/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([version_json(
                "f9NoeotB",
                "7.0.13",
                json!([{ "version_id": null, "project_id": "1u6JkXh5", "file_name": null, "dependency_type": "required" }])
            )])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/project/worldguard"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "DKY9btbd", "slug": "worldguard", "title": "WorldGuard", "description": "Protect regions",
                "icon_url": "https://cdn.modrinth.com/i.webp", "team": "x52pg5zS", "organization": "TmFQVfk0"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/organization/TmFQVfk0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "EngineHub" })))
            .mount(&server)
            .await;

        let modrinth = Modrinth::new(reqwest::Client::new(), server.uri());
        let query = SearchQuery { query: "guard".into(), offset: 0, limit: 20, sort: SearchSort::Relevance };
        let result = modrinth.search(&paper(), &query).await.unwrap();
        assert_eq!(result.total, 1);
        let hit = &result.hits[0];
        assert_eq!((hit.name.as_str(), hit.kind), ("WorldGuard", ModKind::Plugin));
        assert_eq!(hit.categories, ["management"], "loader names aren't categories");
        assert_eq!(hit.page_url, "https://modrinth.com/project/worldguard");

        let versions = modrinth.versions("DKY9btbd", &paper()).await.unwrap();
        let v = &versions[0];
        assert!(v.version.compatible);
        assert_eq!(v.version.filename, "worldguard-bukkit-7.0.13.jar", "the primary file, not sources");
        assert_eq!(v.file.sha512, "aa");
        assert_eq!(v.version.dependencies[0].kind, DependencyKind::Required);

        let project = modrinth.project("worldguard").await.unwrap();
        assert_eq!(project.author, "EngineHub");
    }

    #[tokio::test]
    async fn checks_updates_by_hash() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/version_files/update"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "old-hash": version_json("new", "7.0.14", json!([])) })),
            )
            .mount(&server)
            .await;
        let modrinth = Modrinth::new(reqwest::Client::new(), server.uri());
        let found = modrinth.latest_for_hashes(&["old-hash".into()], &paper()).await.unwrap();
        assert_eq!(found["old-hash"].version.version_number, "7.0.14");
    }

    #[tokio::test]
    async fn an_outage_is_a_clear_conflict() {
        let modrinth = Modrinth::new(reqwest::Client::new(), "http://127.0.0.1:9");
        let err = modrinth.project("x").await.unwrap_err();
        assert!(matches!(err, AppError::Conflict { code: "provider_unavailable", .. }), "{err:?}");
    }

    /// `just test-live`: the real API, end to end.
    #[tokio::test]
    #[ignore]
    async fn live_modrinth() {
        let http = reqwest::Client::builder().user_agent("lodestone-tests").build().unwrap();
        let modrinth = Modrinth::new(http, API);
        let query = SearchQuery { query: "worldguard".into(), offset: 0, limit: 5, sort: SearchSort::Relevance };
        let result = modrinth.search(&paper(), &query).await.unwrap();
        let hit = result.hits.iter().find(|hit| hit.slug == "worldguard").expect("WorldGuard is on Modrinth");
        let versions = modrinth.versions(&hit.id, &paper()).await.unwrap();
        assert!(versions.iter().all(|v| v.version.compatible));
        let latest = &versions[0];
        assert!(latest.version.dependencies.iter().any(|dep| dep.kind == DependencyKind::Required));
        let updates = modrinth.latest_for_hashes(std::slice::from_ref(&latest.file.sha512), &paper()).await.unwrap();
        assert!(updates.contains_key(&latest.file.sha512));
    }
}
