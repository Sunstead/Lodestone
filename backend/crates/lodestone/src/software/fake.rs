//! Offline stand-in for the real providers: a fixed catalog and tiny
//! placeholder "jars" that only the fake runtime can run.

use async_trait::async_trait;

use super::{java_for_minecraft, Launch, SoftwareProvider};
use crate::cache::{Artifact, Source};
use crate::error::AppResult;
use crate::model::{ServerSoftwareId, SoftwareCatalog, SoftwareOption, SoftwareVersion};

pub struct FakeSoftware;

fn versions(list: &[&str]) -> Vec<SoftwareVersion> {
    list.iter()
        .map(|version| SoftwareVersion {
            version: (*version).into(),
            java_major: java_for_minecraft(version),
            stable: true,
        })
        .collect()
}

pub fn catalog() -> SoftwareCatalog {
    use ServerSoftwareId::*;
    let option = |id: ServerSoftwareId, blurb: &str, list: &[&str]| SoftwareOption {
        id,
        label: id.label().into(),
        blurb: blurb.into(),
        kind: id.content_kind(),
        versions: versions(list),
    };
    SoftwareCatalog {
        software: vec![
            option(Vanilla, "The official server from Mojang. No mods or plugins.", &["1.21.4", "1.21.1", "1.20.6"]),
            option(Paper, "High-performance fork with Bukkit/Spigot plugin support.", &["1.21.4", "1.21.1", "1.20.6"]),
            option(Fabric, "Lightweight mod loader that tracks new releases quickly.", &["1.21.4", "1.21.1", "1.20.4"]),
            option(Forge, "The classic mod loader with the largest back catalogue.", &["1.21.1", "1.20.6"]),
            option(Neoforge, "Community fork of Forge for modern modpacks.", &["1.21.4", "1.21.1"]),
        ],
    }
}

#[async_trait]
impl SoftwareProvider for FakeSoftware {
    fn kind(&self) -> &'static str {
        "fake"
    }

    async fn catalog(&self) -> AppResult<SoftwareCatalog> {
        Ok(catalog())
    }

    async fn resolve(&self, software: ServerSoftwareId, version: &str) -> AppResult<Launch> {
        super::java_major(&catalog(), software, version)?;
        let filename = format!("{}-{version}.jar", software.label().to_lowercase());
        let bytes = format!("lodestone fake server: {} {version}\n", software.label()).into_bytes();
        let artifact = Artifact { filename, source: Source::Inline(bytes), hash: None, size: None, pin: None };
        Ok(Launch::Jar { artifact, java_major: None })
    }
}
