//! Moving worlds in and out of Lodestone as zip files, the format players
//! already use for singleplayer saves.
//!
//! Import takes a zip holding a world folder (its `level.dat` at the root
//! or one folder down). Paper's split layout (`world`, `world_nether`,
//! `world_the_end` side by side) is recognised and kept. Export writes the
//! same shape back out.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::content::mrpack::safe_relative;
use crate::error::{AppError, AppResult};
use crate::level::{self, LevelInfo};
use crate::model::WorldLayout;

/// Where the world sits inside the zip.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    /// The level folder's path in the zip, with a trailing `/` (or empty).
    prefix: String,
    /// Paper's sibling folders, when present.
    nether: Option<String>,
    end: Option<String>,
}

fn find(names: &[String]) -> AppResult<Found> {
    let level = names
        .iter()
        .filter(|name| name.rsplit('/').next() == Some("level.dat"))
        .min_by_key(|name| name.matches('/').count())
        .ok_or_else(|| AppError::bad_request("That zip has no level.dat, so it isn't a Minecraft world."))?;
    let prefix = level.trim_end_matches("level.dat").to_owned();
    if prefix.matches('/').count() > 1 {
        return Err(AppError::bad_request("Put the world folder at the top of the zip (its level.dat was too deep)."));
    }
    let base = prefix.trim_end_matches('/');
    let sibling = |suffix: &str| {
        let candidate = if base.is_empty() { None } else { Some(format!("{base}_{suffix}/")) }?;
        names.iter().any(|name| name.starts_with(&candidate)).then_some(candidate)
    };
    Ok(Found { nether: sibling("nether"), end: sibling("the_end"), prefix })
}

const MAX_TOTAL: u64 = 64 << 30;

/// Unpacks a world zip into `universe/<folder>` (plus `_nether`/`_the_end`
/// for Paper worlds). Returns what its level.dat says and its layout.
pub fn import_zip(zip: &Path, universe: &Path, folder: &str) -> AppResult<(LevelInfo, WorldLayout)> {
    let bad = |message: &str| AppError::bad_request(message.to_owned());
    let mut archive = zip::ZipArchive::new(File::open(zip)?).map_err(|_| bad("That file isn't a zip."))?;
    let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    let found = find(&names)?;
    let targets: Vec<(String, String)> = [
        Some((found.prefix.clone(), folder.to_owned())),
        found.nether.clone().map(|p| (p, format!("{folder}_nether"))),
        found.end.clone().map(|p| (p, format!("{folder}_the_end"))),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| bad("An entry in the zip can't be read."))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let Some((rest, dest)) = targets
            .iter()
            .find_map(|(prefix, dest)| name.strip_prefix(prefix.as_str()).map(|rest| (rest.to_owned(), dest)))
        else {
            continue;
        };
        if rest == "session.lock" || rest.is_empty() {
            continue;
        }
        let relative = safe_relative(&rest).ok_or_else(|| bad(&format!("The zip has an unsafe path: {name}")))?;
        total += entry.size();
        if total > MAX_TOTAL {
            return Err(bad("That world is larger than 64 GB."));
        }
        let path = universe.join(dest).join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    let info = level::read(&universe.join(folder).join("level.dat"))
        .ok_or_else(|| bad("The world's level.dat can't be read."))?;
    let layout = if found.nether.is_some() || found.end.is_some() { WorldLayout::Bukkit } else { WorldLayout::Vanilla };
    Ok((info, layout))
}

/// Zips a world's folders (as they sit in `universe`) into `out`.
pub fn export_zip(universe: &Path, folder: &str, out: &Path) -> std::io::Result<()> {
    let file = File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for dir in [folder.to_owned(), format!("{folder}_nether"), format!("{folder}_the_end")] {
        let root = universe.join(&dir);
        if root.is_dir() {
            add_dir(&mut zip, &root, &PathBuf::from(&dir), options)?;
        }
    }
    zip.finish()?;
    Ok(())
}

fn add_dir(
    zip: &mut zip::ZipWriter<File>,
    dir: &Path,
    name: &Path,
    options: zip::write::SimpleFileOptions,
) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let entry_name = name.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            add_dir(zip, &path, &entry_name, options)?;
        } else if kind.is_file() && entry.file_name() != "session.lock" {
            zip.start_file(entry_name.to_string_lossy().replace('\\', "/"), options)?;
            let mut file = File::open(&path)?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                zip.write_all(&buffer[..read])?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn finds_the_world_in_the_zip() {
        let found = find(&names(&["My World/level.dat", "My World/region/r.0.0.mca"])).unwrap();
        assert_eq!(found, Found { prefix: "My World/".into(), nether: None, end: None });
        let root = find(&names(&["level.dat", "region/r.0.0.mca"])).unwrap();
        assert_eq!(root.prefix, "");
        let paper =
            find(&names(&["world/level.dat", "world_nether/DIM-1/region/r.0.0.mca", "world_the_end/DIM1/x"])).unwrap();
        assert_eq!((paper.nether.as_deref(), paper.end.as_deref()), (Some("world_nether/"), Some("world_the_end/")));
        assert!(find(&names(&["readme.txt"])).is_err());
        assert!(find(&names(&["a/b/level.dat"])).is_err());
    }

    #[test]
    fn exports_and_imports_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let info =
            LevelInfo { version_name: Some("1.21.4".into()), level_name: Some("Peaks".into()), ..Default::default() };
        std::fs::create_dir_all(source.join("peaks/region")).unwrap();
        std::fs::write(source.join("peaks/level.dat"), level::write(&info)).unwrap();
        std::fs::write(source.join("peaks/region/r.0.0.mca"), vec![3u8; 9000]).unwrap();
        std::fs::write(source.join("peaks/session.lock"), b"lock").unwrap();
        std::fs::create_dir_all(source.join("peaks_nether/DIM-1")).unwrap();
        std::fs::write(source.join("peaks_nether/DIM-1/x"), b"nether").unwrap();

        let zip = dir.path().join("peaks.zip");
        export_zip(&source, "peaks", &zip).unwrap();

        let target = dir.path().join("target");
        let (read, layout) = import_zip(&zip, &target, "imported").unwrap();
        assert_eq!(read.level_name.as_deref(), Some("Peaks"));
        assert_eq!(layout, WorldLayout::Bukkit);
        assert_eq!(std::fs::read(target.join("imported/region/r.0.0.mca")).unwrap().len(), 9000);
        assert_eq!(std::fs::read(target.join("imported_nether/DIM-1/x")).unwrap(), b"nether");
        assert!(!target.join("imported/session.lock").exists());
    }
}
