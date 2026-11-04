//! Server software resolution: which versions exist, which Java they need,
//! and what to download to run them. Nothing is bundled with Lodestone;
//! every jar comes from its publisher at runtime.

pub mod fake;
pub mod real;

use async_trait::async_trait;

use crate::cache::Artifact;
use crate::error::{AppError, AppResult};
use crate::model::{ServerSoftwareId, SoftwareCatalog};

/// How to launch a resolved server.
#[derive(Debug, Clone)]
pub enum Launch {
    /// `java ... -jar server.jar`: vanilla, Paper, Fabric's server launcher.
    Jar {
        artifact: Artifact,
        /// The exact Java the publisher asks for, when it says.
        java_major: Option<u32>,
    },
    /// Forge and NeoForge: run the installer once (`--installServer`) into
    /// `cache/installs/<key>`, then launch with the `@.../unix_args.txt`
    /// file it writes under `libraries/`.
    Installer {
        installer: Artifact,
        java_major: Option<u32>,
        /// Names the install, e.g. `neoforge-21.1.77`.
        key: String,
    },
}

/// Orders dotted versions numerically, newest first (`1.21.10` > `1.21.9`).
pub fn newest_first(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u64> { v.split(['.', '-']).map(|p| p.parse().unwrap_or(0)).collect() };
    parse(b).cmp(&parse(a))
}

#[async_trait]
pub trait SoftwareProvider: Send + Sync {
    /// `real` or `fake`, reported on `/system`.
    fn kind(&self) -> &'static str;

    /// Every software and version Lodestone can run, newest first.
    async fn catalog(&self) -> AppResult<SoftwareCatalog>;

    /// The artifact(s) to run `software` at Minecraft `version`.
    async fn resolve(&self, software: ServerSoftwareId, version: &str) -> AppResult<Launch>;
}

/// The Java major version a catalog entry needs.
pub fn java_major(catalog: &SoftwareCatalog, software: ServerSoftwareId, version: &str) -> AppResult<u32> {
    let option = catalog
        .software
        .iter()
        .find(|option| option.id == software)
        .ok_or_else(|| AppError::bad_request(format!("{} is not available.", software.label())))?;
    option
        .versions
        .iter()
        .find(|entry| entry.version == version)
        .map(|entry| entry.java_major)
        .ok_or_else(|| AppError::bad_request(format!("{} does not publish Minecraft {version}.", option.label)))
}

/// Java a release needs, used for display when a provider has not said yet.
/// Resolution reads the exact value from the publisher's metadata.
pub fn java_for_minecraft(version: &str) -> u32 {
    let parts: Vec<u32> = version.split(['.', '-']).filter_map(|part| part.parse().ok()).collect();
    // Year-based versions (26.1 onwards) need Java 25.
    if parts.first().is_some_and(|major| *major >= 26) {
        return 25;
    }
    let (minor, patch) = (parts.get(1).copied().unwrap_or(0), parts.get(2).copied().unwrap_or(0));
    match (minor, patch) {
        (m, _) if m >= 21 => 21,
        (20, p) if p >= 5 => 21,
        (m, _) if m >= 18 => 17,
        (17, _) => 16,
        _ => 8,
    }
}

/// The stock JRE image for a Java major version.
pub fn image_for(java_major: u32) -> String {
    format!("eclipse-temurin:{java_major}-jre")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_versions_follow_mojang() {
        assert_eq!(java_for_minecraft("1.21.4"), 21);
        assert_eq!(java_for_minecraft("1.20.6"), 21);
        assert_eq!(java_for_minecraft("1.20.4"), 17);
        assert_eq!(java_for_minecraft("1.18.2"), 17);
        assert_eq!(java_for_minecraft("1.17.1"), 16);
        assert_eq!(java_for_minecraft("1.12.2"), 8);
        assert_eq!(java_for_minecraft("26.3"), 25);
    }
}
