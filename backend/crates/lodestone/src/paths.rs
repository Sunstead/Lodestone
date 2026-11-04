//! On-disk layout under the data dir:
//!
//! ```text
//! cache/blobs/sha256/<hash>        downloaded jars, deduplicated
//! worlds/<worldId>/                mounted as the server's universe dir
//! profiles/<profileId>/            plugins/, mods/, config/ written by the server
//! instances/<instanceId>/          run dir: server.properties, logs/, crash-reports/
//! backups/<worldId>/<backupId>.tar.zst
//! lodestone.db
//! ```

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("lodestone.db")
    }

    pub fn blobs(&self) -> PathBuf {
        self.root.join("cache").join("blobs")
    }

    pub fn installs(&self) -> PathBuf {
        self.root.join("cache").join("installs")
    }

    /// The universe dir for a world; the level folder lives inside it.
    pub fn world(&self, world_id: &str) -> PathBuf {
        self.root.join("worlds").join(world_id)
    }

    pub fn profile(&self, profile_id: &str) -> PathBuf {
        self.root.join("profiles").join(profile_id)
    }

    pub fn instance(&self, instance_id: &str) -> PathBuf {
        self.root.join("instances").join(instance_id)
    }

    pub fn backups(&self, world_id: &str) -> PathBuf {
        self.root.join("backups").join(world_id)
    }

    pub fn backups_root(&self) -> PathBuf {
        self.root.join("backups")
    }

    /// Uploads and exports in progress.
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in ["cache/blobs", "cache/installs", "worlds", "profiles", "instances", "backups", "tmp"] {
            std::fs::create_dir_all(self.root.join(dir))?;
        }
        Ok(())
    }
}
