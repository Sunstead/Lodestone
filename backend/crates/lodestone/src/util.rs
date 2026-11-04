use chrono::{SecondsFormat, Utc};

/// RFC 3339 timestamp in UTC with millisecond precision.
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Time-ordered id with a readable prefix, e.g. `world-0192f3...`.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::now_v7().simple())
}

/// Folder-safe slug: lowercase ascii, digits and dashes.
pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "world".to_owned()
    } else {
        out
    }
}

/// Random alphanumeric secret, used for per-instance RCON passwords.
pub fn secret(len: usize) -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    rand::thread_rng().sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

/// Total size of a directory tree in bytes. Missing paths count as zero.
pub fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path).map(|entries| entries.flatten().map(|entry| dir_size(&entry.path())).sum()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_keeps_it_folder_safe() {
        assert_eq!(slugify("Survival Season 2"), "survival-season-2");
        assert_eq!(slugify("  ~~~ "), "world");
        assert_eq!(slugify("Créative!!World"), "cr-ative-world");
    }

    #[test]
    fn ids_are_prefixed_and_unique() {
        let a = new_id("world");
        let b = new_id("world");
        assert!(a.starts_with("world-"));
        assert_ne!(a, b);
    }

    #[test]
    fn dir_size_sums_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), [0u8; 10]).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b"), [0u8; 5]).unwrap();
        assert_eq!(dir_size(dir.path()), 15);
        assert_eq!(dir_size(&dir.path().join("missing")), 0);
    }
}
