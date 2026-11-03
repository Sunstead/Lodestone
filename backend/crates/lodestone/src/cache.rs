//! Content-addressed download cache. Every server jar and mod lands in
//! `cache/blobs/<algo>/<hash>` after its hash is verified, so profiles
//! that share a file share one copy, and nothing unverified is ever run.

use std::path::{Path, PathBuf};

use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

use crate::error::{AppError, AppResult};
use crate::jobs::JobHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgo {
    Sha1,
    Sha256,
    Sha512,
}

impl HashAlgo {
    fn dir(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }

    pub fn digest(self, bytes: &[u8]) -> String {
        match self {
            Self::Sha1 => hex::encode(Sha1::digest(bytes)),
            Self::Sha256 => hex::encode(Sha256::digest(bytes)),
            Self::Sha512 => hex::encode(Sha512::digest(bytes)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hash {
    pub algo: HashAlgo,
    pub hex: String,
}

#[derive(Debug, Clone)]
pub enum Source {
    /// Downloaded over HTTPS from the publisher.
    Url(String),
    /// Bytes produced locally (fake providers, tests).
    Inline(Vec<u8>),
}

/// Something to fetch into the cache.
#[derive(Debug, Clone)]
pub struct Artifact {
    /// Human name for progress messages, e.g. `paper-1.21.4-232.jar`.
    pub filename: String,
    pub source: Source,
    /// Published hash. Required for URLs; computed for inline bytes.
    pub hash: Option<Hash>,
    pub size: Option<u64>,
}

#[derive(Clone)]
pub struct BlobCache {
    root: PathBuf,
    http: Option<reqwest::Client>,
}

impl BlobCache {
    pub fn new(root: PathBuf, http: Option<reqwest::Client>) -> Self {
        Self { root, http }
    }

    fn path_for(&self, hash: &Hash) -> PathBuf {
        self.root.join(hash.algo.dir()).join(&hash.hex)
    }

    /// Returns the cached path, downloading and verifying when missing.
    pub async fn fetch(&self, artifact: &Artifact, job: Option<&JobHandle>) -> AppResult<PathBuf> {
        if let Some(hash) = &artifact.hash {
            let path = self.path_for(hash);
            if path.is_file() {
                return Ok(path);
            }
        }
        let bytes = match &artifact.source {
            Source::Inline(bytes) => bytes.clone(),
            Source::Url(url) => self.download(url, artifact, job).await?,
        };
        let hash = match &artifact.hash {
            Some(expected) => {
                let actual = expected.algo.digest(&bytes);
                if !actual.eq_ignore_ascii_case(&expected.hex) {
                    return Err(AppError::Internal(anyhow::anyhow!(
                        "{} failed verification: expected {} {}, got {actual}. The download was discarded.",
                        artifact.filename,
                        expected.algo.dir(),
                        expected.hex
                    )));
                }
                expected.clone()
            }
            None => Hash { algo: HashAlgo::Sha256, hex: HashAlgo::Sha256.digest(&bytes) },
        };
        let path = self.path_for(&hash);
        write_atomic(&path, &bytes).await?;
        Ok(path)
    }

    async fn download(&self, url: &str, artifact: &Artifact, job: Option<&JobHandle>) -> AppResult<Vec<u8>> {
        if artifact.hash.is_none() {
            return Err(AppError::Internal(anyhow::anyhow!("refusing to download {url} without a published hash")));
        }
        let client = self
            .http
            .as_ref()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("network downloads are disabled (fake providers)")))?;
        let mut response =
            client.get(url).send().await.and_then(reqwest::Response::error_for_status).map_err(|err| {
                AppError::Internal(anyhow::anyhow!("could not download {}: {err}", artifact.filename))
            })?;
        let total = response.content_length().or(artifact.size);
        let mut bytes = Vec::with_capacity(total.unwrap_or(0) as usize);
        let mut last_report = 0usize;
        while let Some(chunk) = response.chunk().await.map_err(|err| {
            AppError::Internal(anyhow::anyhow!("download of {} interrupted: {err}", artifact.filename))
        })? {
            bytes.extend_from_slice(&chunk);
            if let Some(job) = job {
                if bytes.len() - last_report > 512 * 1024 {
                    last_report = bytes.len();
                    let progress = total.map(|total| bytes.len() as f64 / total as f64);
                    job.progress(progress, format!("Downloading {}", artifact.filename)).await;
                }
            }
        }
        Ok(bytes)
    }
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension(format!("part-{}", std::process::id()));
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn caches_inline_bytes_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(dir.path().to_path_buf(), None);
        let artifact =
            Artifact { filename: "a.jar".into(), source: Source::Inline(b"jar".to_vec()), hash: None, size: None };
        let path = cache.fetch(&artifact, None).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"jar");
        assert!(path.ends_with(HashAlgo::Sha256.digest(b"jar")));
    }

    #[tokio::test]
    async fn rejects_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(dir.path().to_path_buf(), None);
        let artifact = Artifact {
            filename: "bad.jar".into(),
            source: Source::Inline(b"tampered".to_vec()),
            hash: Some(Hash { algo: HashAlgo::Sha1, hex: HashAlgo::Sha1.digest(b"original") }),
            size: None,
        };
        let err = cache.fetch(&artifact, None).await.unwrap_err();
        assert!(err.to_string().contains("failed verification"));
    }

    #[tokio::test]
    async fn refuses_unhashed_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(dir.path().to_path_buf(), Some(reqwest::Client::new()));
        let artifact = Artifact {
            filename: "x.jar".into(),
            source: Source::Url("https://example.invalid/x.jar".into()),
            hash: None,
            size: None,
        };
        assert!(cache.fetch(&artifact, None).await.is_err());
    }
}
