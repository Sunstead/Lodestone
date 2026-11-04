//! Offline content providers for `--providers fake` and the tests: Modrinth,
//! Hangar and CurseForge stand-ins over one catalog,
//! `shared/fake-content.json`, which the frontend demo uses too. Every
//! "jar" is a few placeholder bytes, hashed the way each real provider
//! publishes (sha512, sha256, sha1). Projects marked `restrictedOn` a
//! provider must be downloaded by hand there, like CurseForge's
//! "distribution disabled" files.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;

use super::curseforge::ApiKey;
use super::{project_hit, ContentProvider, FileRef, ProjectInfo, ResolvedVersion, SearchQuery, SearchSort, Target};
use crate::cache::HashAlgo;
use crate::error::{AppError, AppResult};
use crate::model::{ContentDependency, ContentSearchResult, ContentSource, ContentVersion, ModKind, VersionChannel};

const CATALOG: &str = include_str!("../../../../../shared/fake-content.json");

#[derive(Deserialize)]
struct Catalog {
    projects: Vec<Project>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: String,
    slug: String,
    name: String,
    author: String,
    description: String,
    categories: Vec<String>,
    downloads: u64,
    #[serde(default)]
    restricted_on: Vec<ContentSource>,
    versions: Vec<Version>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Version {
    id: String,
    version_number: String,
    channel: VersionChannel,
    published_at: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    dependencies: Vec<ContentDependency>,
}

pub struct FakeContent {
    source: ContentSource,
    projects: Vec<Project>,
    /// CurseForge's stand-in wants a key, like the real one.
    key: Option<ApiKey>,
}

impl Default for FakeContent {
    fn default() -> Self {
        Self::new(ContentSource::Modrinth, None)
    }
}

/// The placeholder bytes for a fake version.
pub fn jar_bytes(version_id: &str) -> Vec<u8> {
    format!("lodestone fake content: {version_id}\n").into_bytes()
}

impl FakeContent {
    pub fn new(source: ContentSource, key: Option<ApiKey>) -> Self {
        let catalog: Catalog = serde_json::from_str(CATALOG).expect("shared/fake-content.json is valid");
        Self { source, projects: catalog.projects, key }
    }

    fn page(&self, project: &Project) -> String {
        match self.source {
            ContentSource::Hangar => {
                format!("https://hangar.papermc.io/{}/{}", project.author.replace(' ', ""), project.slug)
            }
            ContentSource::Curseforge => format!("https://www.curseforge.com/minecraft/mc-mods/{}", project.slug),
            _ => format!("https://modrinth.com/project/{}", project.slug),
        }
    }

    fn info(&self, project: &Project) -> ProjectInfo {
        ProjectInfo {
            id: project.id.clone(),
            slug: project.slug.clone(),
            name: project.name.clone(),
            author: project.author.clone(),
            description: project.description.clone(),
            icon_url: None,
            page_url: self.page(project),
        }
    }

    fn resolve(&self, project: &Project, version: &Version, target: &Target) -> ResolvedVersion {
        let bytes = jar_bytes(&version.id);
        let filename = format!("{}-{}.jar", project.slug, version.version_number.replace('+', "-"));
        let restricted = project.restricted_on.contains(&self.source);
        let mut file = FileRef::default();
        match self.source {
            ContentSource::Hangar => file.sha256 = Some(HashAlgo::Sha256.digest(&bytes)),
            ContentSource::Curseforge => file.sha1 = Some(HashAlgo::Sha1.digest(&bytes)),
            _ => {
                file.sha512 = Some(HashAlgo::Sha512.digest(&bytes));
                file.sha1 = Some(HashAlgo::Sha1.digest(&bytes));
            }
        }
        if restricted {
            file.manual_url = Some(format!("{}/files/{}", self.page(project), version.id));
        } else {
            file.inline = Some(bytes.clone());
        }
        ResolvedVersion {
            version: ContentVersion {
                provider: self.source,
                id: version.id.clone(),
                project_id: project.id.clone(),
                name: format!("{} {}", project.name, version.version_number),
                version_number: version.version_number.clone(),
                channel: version.channel,
                published_at: version.published_at.clone(),
                downloads: project.downloads / 10,
                game_versions: version.game_versions.clone(),
                loaders: version.loaders.clone(),
                filename,
                size_bytes: bytes.len() as u64,
                dependencies: version.dependencies.clone(),
                compatible: target.accepts(&version.loaders, &version.game_versions),
            },
            file,
        }
    }

    fn fits(project: &Project, target: &Target) -> bool {
        project.versions.iter().any(|v| target.accepts(&v.loaders, &v.game_versions))
    }

    fn find(&self, id: &str) -> AppResult<&Project> {
        self.projects
            .iter()
            .find(|project| project.id == id || project.slug == id)
            .ok_or_else(|| AppError::NotFound(format!("{} has no such project or version.", self.source.label())))
    }

    fn all_versions<'a>(&'a self, target: &'a Target) -> impl Iterator<Item = ResolvedVersion> + 'a {
        self.projects
            .iter()
            .flat_map(move |project| project.versions.iter().map(move |version| self.resolve(project, version, target)))
    }
}

#[async_trait]
impl ContentProvider for FakeContent {
    fn id(&self) -> ContentSource {
        self.source
    }

    fn description(&self) -> &'static str {
        match self.source {
            ContentSource::Hangar => "Paper plugins",
            _ => "Mods and plugins",
        }
    }

    fn unavailable(&self, target: &Target) -> Option<String> {
        if self.source == ContentSource::Hangar && target.kind != ModKind::Plugin {
            return Some("Hangar only has Paper plugins.".into());
        }
        if self.key.as_ref().is_some_and(|key| key.get().is_none()) {
            return Some("CurseForge needs an API key. An admin can add one in Settings.".into());
        }
        None
    }

    async fn search(&self, target: &Target, query: &SearchQuery) -> AppResult<ContentSearchResult> {
        let needle = query.query.trim().to_lowercase();
        let mut hits: Vec<&Project> = self
            .projects
            .iter()
            .filter(|project| Self::fits(project, target))
            .filter(|project| {
                needle.is_empty()
                    || project.name.to_lowercase().contains(&needle)
                    || project.slug.contains(&needle)
                    || project.description.to_lowercase().contains(&needle)
            })
            .collect();
        match query.sort {
            SearchSort::Relevance | SearchSort::Downloads => hits.sort_by_key(|p| std::cmp::Reverse(p.downloads)),
            SearchSort::Updated | SearchSort::Newest => {
                hits.sort_by(|a, b| b.versions[0].published_at.cmp(&a.versions[0].published_at))
            }
        }
        let total = hits.len() as u64;
        Ok(ContentSearchResult {
            hits: hits
                .into_iter()
                .skip(query.offset as usize)
                .take(query.limit as usize)
                .map(|project| {
                    project_hit(
                        self.source,
                        self.info(project),
                        project.downloads,
                        project.categories.clone(),
                        target.kind,
                    )
                })
                .collect(),
            total,
            offset: query.offset,
            limit: query.limit,
        })
    }

    async fn project(&self, id: &str) -> AppResult<ProjectInfo> {
        Ok(self.info(self.find(id)?))
    }

    async fn versions(&self, project_id: &str, target: &Target) -> AppResult<Vec<ResolvedVersion>> {
        let project = self.find(project_id)?;
        Ok(project
            .versions
            .iter()
            .map(|version| self.resolve(project, version, target))
            .filter(|resolved| resolved.version.compatible)
            .collect())
    }

    async fn version(&self, version_id: &str, target: &Target) -> AppResult<ResolvedVersion> {
        self.all_versions(target)
            .find(|resolved| resolved.version.id == version_id)
            .ok_or_else(|| AppError::NotFound(format!("{} has no such project or version.", self.source.label())))
    }

    async fn identify(&self, sha512: &[String], target: &Target) -> AppResult<HashMap<String, ResolvedVersion>> {
        if self.source != ContentSource::Modrinth {
            return Ok(HashMap::new());
        }
        Ok(self
            .all_versions(target)
            .filter_map(|resolved| {
                let hash = resolved.file.sha512.clone()?;
                sha512.contains(&hash).then_some((hash, resolved))
            })
            .collect())
    }
}
