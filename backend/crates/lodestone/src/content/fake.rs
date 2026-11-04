//! Offline content provider for `--providers fake` and the tests. The
//! catalog is `shared/fake-content.json`, which the frontend demo uses too,
//! and every "jar" is a few placeholder bytes.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;

use super::{project_hit, ContentProvider, FileRef, ProjectInfo, ResolvedVersion, SearchQuery, SearchSort, Target};
use crate::cache::HashAlgo;
use crate::error::{AppError, AppResult};
use crate::model::{ContentDependency, ContentSearchResult, ContentSource, ContentVersion, VersionChannel};

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
    projects: Vec<Project>,
}

impl Default for FakeContent {
    fn default() -> Self {
        let catalog: Catalog = serde_json::from_str(CATALOG).expect("shared/fake-content.json is valid");
        Self { projects: catalog.projects }
    }
}

/// The placeholder bytes for a fake version.
pub fn jar_bytes(version_id: &str) -> Vec<u8> {
    format!("lodestone fake content: {version_id}\n").into_bytes()
}

impl Project {
    fn info(&self) -> ProjectInfo {
        ProjectInfo {
            id: self.id.clone(),
            slug: self.slug.clone(),
            name: self.name.clone(),
            author: self.author.clone(),
            description: self.description.clone(),
            icon_url: None,
            page_url: format!("https://modrinth.com/project/{}", self.slug),
        }
    }

    fn resolve(&self, version: &Version, target: &Target) -> ResolvedVersion {
        let bytes = jar_bytes(&version.id);
        let filename = format!("{}-{}.jar", self.slug, version.version_number.replace('+', "-"));
        ResolvedVersion {
            version: ContentVersion {
                provider: ContentSource::Modrinth,
                id: version.id.clone(),
                project_id: self.id.clone(),
                name: format!("{} {}", self.name, version.version_number),
                version_number: version.version_number.clone(),
                channel: version.channel,
                published_at: version.published_at.clone(),
                downloads: self.downloads / 10,
                game_versions: version.game_versions.clone(),
                loaders: version.loaders.clone(),
                filename,
                size_bytes: bytes.len() as u64,
                dependencies: version.dependencies.clone(),
                compatible: target.accepts(&version.loaders, &version.game_versions),
            },
            file: FileRef {
                url: None,
                sha512: HashAlgo::Sha512.digest(&bytes),
                sha1: Some(HashAlgo::Sha1.digest(&bytes)),
                inline: Some(bytes),
            },
        }
    }

    fn fits(&self, target: &Target) -> bool {
        self.versions.iter().any(|v| target.accepts(&v.loaders, &v.game_versions))
    }
}

impl FakeContent {
    fn find(&self, id: &str) -> AppResult<&Project> {
        self.projects
            .iter()
            .find(|project| project.id == id || project.slug == id)
            .ok_or_else(|| AppError::NotFound("Modrinth has no such project or version.".into()))
    }

    fn all_versions(&self, target: &Target) -> impl Iterator<Item = ResolvedVersion> + '_ {
        let target = target.clone();
        self.projects.iter().flat_map(move |project| {
            let target = target.clone();
            project.versions.iter().map(move |version| project.resolve(version, &target))
        })
    }
}

#[async_trait]
impl ContentProvider for FakeContent {
    fn id(&self) -> ContentSource {
        ContentSource::Modrinth
    }

    async fn search(&self, target: &Target, query: &SearchQuery) -> AppResult<ContentSearchResult> {
        let needle = query.query.trim().to_lowercase();
        let mut hits: Vec<&Project> = self
            .projects
            .iter()
            .filter(|project| project.fits(target))
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
                        ContentSource::Modrinth,
                        project.info(),
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
        Ok(self.find(id)?.info())
    }

    async fn versions(&self, project_id: &str, target: &Target) -> AppResult<Vec<ResolvedVersion>> {
        let project = self.find(project_id)?;
        Ok(project
            .versions
            .iter()
            .map(|version| project.resolve(version, target))
            .filter(|resolved| resolved.version.compatible)
            .collect())
    }

    async fn version(&self, version_id: &str, target: &Target) -> AppResult<ResolvedVersion> {
        self.all_versions(target)
            .find(|resolved| resolved.version.id == version_id)
            .ok_or_else(|| AppError::NotFound("Modrinth has no such project or version.".into()))
    }

    async fn latest_for_hashes(
        &self,
        sha512: &[String],
        target: &Target,
    ) -> AppResult<HashMap<String, ResolvedVersion>> {
        let mut out = HashMap::new();
        for hash in sha512 {
            let Some(current) = self.all_versions(target).find(|resolved| &resolved.file.sha512 == hash) else {
                continue;
            };
            let project = self.find(&current.version.project_id)?;
            let latest = project
                .versions
                .iter()
                .map(|version| project.resolve(version, target))
                .find(|resolved| resolved.version.compatible);
            if let Some(latest) = latest {
                out.insert(hash.clone(), latest);
            }
        }
        Ok(out)
    }

    async fn identify(&self, sha512: &[String], target: &Target) -> AppResult<HashMap<String, ResolvedVersion>> {
        Ok(self
            .all_versions(target)
            .filter(|resolved| sha512.contains(&resolved.file.sha512))
            .map(|resolved| (resolved.file.sha512.clone(), resolved))
            .collect())
    }
}
