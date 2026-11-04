//! `level.dat`: what a world says about itself. Gzipped NBT; Lodestone
//! reads which Minecraft version last saved it (to refuse downgrades), its
//! seed, the in-game day and time (shown while it's offline), and the
//! basics of an imported world.

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LevelInfo {
    pub data_version: Option<i32>,
    /// e.g. `1.21.4`.
    pub version_name: Option<String>,
    pub level_name: Option<String>,
    pub seed: Option<i64>,
    pub day_time: Option<i64>,
    pub game_type: Option<i32>,
    pub difficulty: Option<i8>,
    pub hardcore: Option<bool>,
}

#[derive(Deserialize, Serialize, Default)]
struct Root {
    #[serde(rename = "Data")]
    data: Data,
}

#[derive(Deserialize, Serialize, Default)]
struct Data {
    #[serde(rename = "DataVersion", default, skip_serializing_if = "Option::is_none")]
    data_version: Option<i32>,
    #[serde(rename = "Version", default, skip_serializing_if = "Option::is_none")]
    version: Option<Version>,
    #[serde(rename = "LevelName", default, skip_serializing_if = "Option::is_none")]
    level_name: Option<String>,
    #[serde(rename = "DayTime", default, skip_serializing_if = "Option::is_none")]
    day_time: Option<i64>,
    #[serde(rename = "WorldGenSettings", default, skip_serializing_if = "Option::is_none")]
    world_gen: Option<WorldGen>,
    /// Before 1.16 the seed sat here.
    #[serde(rename = "RandomSeed", default, skip_serializing_if = "Option::is_none")]
    random_seed: Option<i64>,
    #[serde(rename = "GameType", default, skip_serializing_if = "Option::is_none")]
    game_type: Option<i32>,
    #[serde(rename = "Difficulty", default, skip_serializing_if = "Option::is_none")]
    difficulty: Option<i8>,
    #[serde(rename = "hardcore", default, skip_serializing_if = "Option::is_none")]
    hardcore: Option<i8>,
}

#[derive(Deserialize, Serialize)]
struct Version {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Deserialize, Serialize)]
struct WorldGen {
    seed: i64,
}

/// Reads `level.dat` bytes (gzipped, or raw NBT).
pub fn parse(bytes: &[u8]) -> Option<LevelInfo> {
    let raw = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes).take(64 << 20).read_to_end(&mut out).ok()?;
        out
    } else {
        bytes.to_vec()
    };
    let root: Root = fastnbt::from_bytes(&raw).ok()?;
    let data = root.data;
    Some(LevelInfo {
        data_version: data.data_version,
        version_name: data.version.map(|v| v.name),
        level_name: data.level_name,
        seed: data.world_gen.map(|w| w.seed).or(data.random_seed),
        day_time: data.day_time,
        game_type: data.game_type,
        difficulty: data.difficulty,
        hardcore: data.hardcore.map(|h| h != 0),
    })
}

pub fn read(path: &Path) -> Option<LevelInfo> {
    parse(&std::fs::read(path).ok()?)
}

/// A minimal gzipped `level.dat`, as a server would write it. For the fake
/// runtime and tests.
pub fn write(info: &LevelInfo) -> Vec<u8> {
    use std::io::Write;
    let root = Root {
        data: Data {
            data_version: info.data_version,
            version: info.version_name.clone().map(|name| Version { name }),
            level_name: info.level_name.clone(),
            day_time: info.day_time,
            world_gen: info.seed.map(|seed| WorldGen { seed }),
            random_seed: None,
            game_type: info.game_type,
            difficulty: info.difficulty,
            hardcore: info.hardcore.map(i8::from),
        },
    };
    let nbt = fastnbt::to_bytes(&root).expect("level.dat serialises");
    let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    out.write_all(&nbt).expect("writes to memory");
    out.finish().expect("writes to memory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_what_lodestone_reads() {
        let info = LevelInfo {
            data_version: Some(4189),
            version_name: Some("1.21.4".into()),
            level_name: Some("Survival".into()),
            seed: Some(-4_172_144_997_902_289_642),
            day_time: Some(48_000 + 6_000),
            game_type: Some(0),
            difficulty: Some(2),
            hardcore: Some(false),
        };
        assert_eq!(parse(&write(&info)), Some(info));
    }

    #[test]
    fn junk_is_not_a_level() {
        assert_eq!(parse(b"fake level.dat"), None);
        assert_eq!(parse(&[0x1f, 0x8b, 0, 0]), None);
    }
}
