//! Mods and plugins. A profile's content is a lockfile (`profile_content`)
//! of exact provider versions pinned by sha512; the jars live in the blob
//! cache and are linked into the profile's `plugins/` or `mods/` folder when
//! a server prepares.
//!
//! Providers (`ContentProvider`) answer search, versions and update checks.
//! Modrinth is the first; Hangar and CurseForge slot in beside it. Install
//! plans (`plan`) resolve dependencies the way Prism does: required ones are
//! added, optional ones are offered, incompatible ones block.

pub mod fake;
pub mod install;
pub mod jarmeta;
pub mod modrinth;
pub mod mrpack;
pub mod plan;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::cache::{Artifact, Hash, HashAlgo, Source};
use crate::error::{AppError, AppResult};
use crate::model::{
    AvailableUpdate, ContentDependency, ContentEntry, ContentKind, ContentProject, ContentProviderInfo,
    ContentSearchResult, ContentSource, ContentVersion, DependencyKind, ModKind, Profile, ServerSoftwareId,
};

/// What a profile can run: the loader names providers use, and its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub software: ServerSoftwareId,
    pub kind: ModKind,
    pub loaders: Vec<String>,
    pub game_version: String,
}

impl Target {
    /// Loader names as Modrinth and Hangar spell them.
    pub fn loaders_for(software: ServerSoftwareId) -> Vec<String> {
        let names: &[&str] = match software {
            ServerSoftwareId::Vanilla => &[],
            ServerSoftwareId::Paper => &["paper", "spigot", "bukkit"],
            ServerSoftwareId::Fabric => &["fabric"],
            ServerSoftwareId::Forge => &["forge"],
            ServerSoftwareId::Neoforge => &["neoforge"],
        };
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    pub fn for_profile(profile: &Profile) -> AppResult<Self> {
        let kind = match profile.software_id.content_kind() {
            ContentKind::Plugins => ModKind::Plugin,
            ContentKind::Mods => ModKind::Mod,
            ContentKind::Vanilla => {
                return Err(AppError::Conflict {
                    code: "content_unsupported",
                    message: "Vanilla servers can't load mods or plugins. Switch this configuration to Paper for plugins, or Fabric or NeoForge for mods.".into(),
                })
            }
        };
        Ok(Self {
            software: profile.software_id,
            kind,
            loaders: Self::loaders_for(profile.software_id),
            game_version: profile.version.clone(),
        })
    }

    pub fn accepts(&self, loaders: &[String], game_versions: &[String]) -> bool {
        loaders.iter().any(|loader| self.loaders.contains(loader)) && game_versions.contains(&self.game_version)
    }

    /// The profile folder the jars go into.
    pub fn folder(&self) -> &'static str {
        match self.kind {
            ModKind::Plugin => "plugins",
            ModKind::Mod => "mods",
        }
    }

    /// e.g. `Paper 1.21.4`.
    pub fn describe(&self) -> String {
        format!("{} {}", self.software.label(), self.game_version)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchSort {
    #[default]
    Relevance,
    Downloads,
    Updated,
    Newest,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub offset: u64,
    pub limit: u64,
    pub sort: SearchSort,
}

/// The parts of a project an install needs to name it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectInfo {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub page_url: String,
}

/// A version with where to download its primary file.
#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub version: ContentVersion,
    pub file: FileRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileRef {
    pub url: Option<String>,
    pub sha512: String,
    pub sha1: Option<String>,
    /// Only for the fake provider: the bytes themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline: Option<Vec<u8>>,
}

impl FileRef {
    pub fn artifact(&self, filename: &str, size: u64) -> AppResult<Artifact> {
        let hash = Hash { algo: HashAlgo::Sha512, hex: self.sha512.clone() };
        let source = match (&self.inline, &self.url) {
            (Some(bytes), _) => Source::Inline(bytes.clone()),
            (None, Some(url)) => Source::Url(url.clone()),
            (None, None) => {
                return Err(AppError::Conflict {
                    code: "content_missing",
                    message: format!("{filename} was uploaded and is no longer in the cache. Upload it again."),
                })
            }
        };
        Ok(Artifact { filename: filename.to_owned(), source, hash: Some(hash), size: Some(size), pin: None })
    }
}

#[async_trait]
pub trait ContentProvider: Send + Sync {
    fn id(&self) -> ContentSource;

    /// Why this provider can't serve `target`, if it can't.
    fn unavailable(&self, _target: &Target) -> Option<String> {
        None
    }

    /// Projects that have a version for `target`. `installed` is left false.
    async fn search(&self, target: &Target, query: &SearchQuery) -> AppResult<ContentSearchResult>;

    async fn project(&self, id: &str) -> AppResult<ProjectInfo>;

    /// Versions for `target`, newest first.
    async fn versions(&self, project_id: &str, target: &Target) -> AppResult<Vec<ResolvedVersion>>;

    async fn version(&self, version_id: &str, target: &Target) -> AppResult<ResolvedVersion>;

    /// The newest version for `target` of each file, by sha512. Files the
    /// provider doesn't know are left out.
    async fn latest_for_hashes(
        &self,
        _sha512: &[String],
        _target: &Target,
    ) -> AppResult<HashMap<String, ResolvedVersion>> {
        Ok(HashMap::new())
    }

    /// Which published version each file is, by sha512 (uploads, modpacks).
    async fn identify(&self, _sha512: &[String], _target: &Target) -> AppResult<HashMap<String, ResolvedVersion>> {
        Ok(HashMap::new())
    }
}

/// Every configured provider.
#[derive(Clone)]
pub struct Providers {
    list: Vec<Arc<dyn ContentProvider>>,
}

impl Providers {
    pub fn new(list: Vec<Arc<dyn ContentProvider>>) -> Self {
        Self { list }
    }

    pub fn get(&self, id: ContentSource) -> AppResult<Arc<dyn ContentProvider>> {
        self.list
            .iter()
            .find(|provider| provider.id() == id)
            .cloned()
            .ok_or_else(|| AppError::bad_request(format!("{} isn't set up on this Lodestone.", id.label())))
    }

    pub fn for_target(&self, id: ContentSource, target: &Target) -> AppResult<Arc<dyn ContentProvider>> {
        let provider = self.get(id)?;
        if let Some(reason) = provider.unavailable(target) {
            return Err(AppError::bad_request(reason));
        }
        Ok(provider)
    }

    pub fn info(&self, target: &Target) -> Vec<ContentProviderInfo> {
        self.list
            .iter()
            .map(|provider| {
                let reason = provider.unavailable(target);
                ContentProviderInfo {
                    id: provider.id(),
                    label: provider.id().label().into(),
                    available: reason.is_none(),
                    reason,
                }
            })
            .collect()
    }

    pub fn all(&self) -> &[Arc<dyn ContentProvider>] {
        &self.list
    }
}

/// One lockfile row, as stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentRecord {
    pub id: String,
    pub provider: ContentSource,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub slug: Option<String>,
    pub name: String,
    pub version_name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub kind: ModKind,
    pub author: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub page_url: Option<String>,
    pub file: FileRef,
    pub enabled: bool,
    pub dependency: bool,
    pub dependencies: Vec<ContentDependency>,
    /// Empty for uploads, which are assumed to fit.
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub update: Option<AvailableUpdate>,
    pub installed_at: String,
}

impl ContentRecord {
    pub fn from_version(
        id: String,
        project: &ProjectInfo,
        resolved: &ResolvedVersion,
        kind: ModKind,
        dependency: bool,
        installed_at: String,
    ) -> Self {
        let version = &resolved.version;
        Self {
            id,
            provider: version.provider,
            project_id: Some(version.project_id.clone()),
            version_id: Some(version.id.clone()),
            slug: Some(project.slug.clone()),
            name: project.name.clone(),
            version_name: version.version_number.clone(),
            filename: version.filename.clone(),
            size_bytes: version.size_bytes,
            kind,
            author: project.author.clone(),
            description: project.description.clone(),
            icon_url: project.icon_url.clone(),
            page_url: Some(project.page_url.clone()),
            file: resolved.file.clone(),
            enabled: true,
            dependency,
            dependencies: version.dependencies.clone(),
            game_versions: version.game_versions.clone(),
            loaders: version.loaders.clone(),
            update: None,
            installed_at,
        }
    }

    pub fn compatible(&self, target: &Target) -> bool {
        if self.kind != target.kind {
            return false;
        }
        self.provider == ContentSource::Upload || target.accepts(&self.loaders, &self.game_versions)
    }

    pub fn requires(&self, project_id: &str) -> bool {
        self.dependencies
            .iter()
            .any(|dep| dep.kind == DependencyKind::Required && dep.project_id.as_deref() == Some(project_id))
    }
}

/// Lockfile rows as the UI sees them, with who-needs-what filled in.
pub fn entries(records: &[ContentRecord], target: Option<&Target>) -> Vec<ContentEntry> {
    records
        .iter()
        .map(|record| {
            let required_by = match &record.project_id {
                Some(project) => records
                    .iter()
                    .filter(|other| other.id != record.id && other.enabled && other.requires(project))
                    .map(|other| other.name.clone())
                    .collect(),
                None => vec![],
            };
            ContentEntry {
                id: record.id.clone(),
                provider: record.provider,
                project_id: record.project_id.clone(),
                version_id: record.version_id.clone(),
                name: record.name.clone(),
                version_name: record.version_name.clone(),
                filename: record.filename.clone(),
                size_bytes: record.size_bytes,
                kind: record.kind,
                author: record.author.clone(),
                description: record.description.clone(),
                icon_url: record.icon_url.clone(),
                page_url: record.page_url.clone(),
                enabled: record.enabled,
                dependency: record.dependency,
                required_by,
                compatible: target.is_some_and(|target| record.compatible(target)),
                update: record.update.clone(),
                installed_at: record.installed_at.clone(),
            }
        })
        .collect()
}

/// A search hit before `installed` is known.
pub fn project_hit(
    provider: ContentSource,
    info: ProjectInfo,
    downloads: u64,
    categories: Vec<String>,
    kind: ModKind,
) -> ContentProject {
    ContentProject {
        provider,
        id: info.id,
        slug: info.slug,
        name: info.name,
        author: info.author,
        description: info.description,
        icon_url: info.icon_url,
        page_url: info.page_url,
        downloads,
        categories,
        kind,
        installed: false,
    }
}

/// A jar's file name made safe to write into a profile folder.
pub fn safe_filename(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next()?.trim();
    let cleaned: String =
        base.chars().map(|ch| if ch.is_ascii_alphanumeric() || "._+-".contains(ch) { ch } else { '_' }).collect();
    let lower = cleaned.to_ascii_lowercase();
    if cleaned.len() < 5 || cleaned.starts_with('.') || !lower.ends_with(".jar") {
        return None;
    }
    Some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_stay_inside_the_folder() {
        assert_eq!(safe_filename("WorldEdit-7.3.jar").as_deref(), Some("WorldEdit-7.3.jar"));
        assert_eq!(safe_filename("../../etc/evil.jar").as_deref(), Some("evil.jar"));
        assert_eq!(safe_filename("C:\\mods\\a b.jar").as_deref(), Some("a_b.jar"));
        assert_eq!(safe_filename("notes.txt"), None);
        assert_eq!(safe_filename(".jar"), None);
        assert_eq!(safe_filename(".hidden.jar"), None);
    }

    #[test]
    fn targets_follow_the_software() {
        assert_eq!(Target::loaders_for(ServerSoftwareId::Paper), ["paper", "spigot", "bukkit"]);
        let target = Target {
            software: ServerSoftwareId::Fabric,
            kind: ModKind::Mod,
            loaders: vec!["fabric".into()],
            game_version: "1.21.4".into(),
        };
        assert!(target.accepts(&["fabric".into(), "quilt".into()], &["1.21.4".into()]));
        assert!(!target.accepts(&["forge".into()], &["1.21.4".into()]));
        assert!(!target.accepts(&["fabric".into()], &["1.21.1".into()]));
    }
}
