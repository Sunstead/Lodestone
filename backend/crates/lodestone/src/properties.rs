//! `server.properties`: field metadata, validation and rendering.
//!
//! Metadata comes from `shared/server-properties.schema.json`, the same
//! file the frontend demo uses. Every key has exactly one owner:
//! world keys come from the World, profile keys from the Profile's
//! overrides, and instance/managed keys are computed by Lodestone.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::model::{PropertyControl, PropertyField, PropertyScope, StringMap, World};

/// Ports the server listens on inside its container. Docker maps the
/// instance's host ports onto these.
pub const CONTAINER_GAME_PORT: u16 = 25565;
pub const CONTAINER_RCON_PORT: u16 = 25575;
pub const CONTAINER_QUERY_PORT: u16 = 25565;

#[derive(Deserialize)]
struct Schema {
    fields: Vec<PropertyField>,
}

static SCHEMA: LazyLock<Vec<PropertyField>> = LazyLock::new(|| {
    let raw = include_str!("../../../../shared/server-properties.schema.json");
    serde_json::from_str::<Schema>(raw).expect("shared/server-properties.schema.json is valid").fields
});

pub fn schema() -> &'static [PropertyField] {
    &SCHEMA
}

pub fn field(key: &str) -> Option<&'static PropertyField> {
    SCHEMA.iter().find(|field| field.key == key)
}

/// Profile-scoped fields with the profile's overrides applied.
pub fn profile_fields(overrides: &StringMap) -> Vec<PropertyField> {
    SCHEMA
        .iter()
        .filter(|field| field.scope == PropertyScope::Profile)
        .map(|field| {
            let mut out = field.clone();
            out.value = if field.sensitive == Some(true) {
                // Secrets are never echoed back.
                String::new()
            } else {
                overrides.get(&field.key).cloned().unwrap_or_else(|| field.default_value.clone())
            };
            out
        })
        .collect()
}

/// Checks a batch of profile property edits against the schema.
pub fn validate_profile_changes(changes: &StringMap) -> AppResult<()> {
    for (key, value) in changes {
        let field =
            field(key).ok_or_else(|| AppError::bad_request(format!("`{key}` is not a known server property.")))?;
        if field.scope != PropertyScope::Profile {
            let owner = match field.scope {
                PropertyScope::World => "the world",
                PropertyScope::Instance => "the instance",
                PropertyScope::Profile => unreachable!(),
            };
            return Err(AppError::bad_request(format!("`{key}` belongs to {owner}; edit it there.")));
        }
        if field.read_only == Some(true) {
            return Err(AppError::bad_request(format!("`{key}` is managed by Lodestone and cannot be edited.")));
        }
        validate_value(field, value)?;
    }
    Ok(())
}

fn validate_value(field: &PropertyField, value: &str) -> AppResult<()> {
    let bad = |why: String| AppError::bad_request(format!("{} (`{}`): {why}", field.label, field.key));
    match field.control {
        PropertyControl::Switch => {
            if value != "true" && value != "false" {
                return Err(bad("must be true or false".into()));
            }
        }
        PropertyControl::Number | PropertyControl::Slider => {
            let number: f64 = value.trim().parse().map_err(|_| bad(format!("`{value}` is not a number")))?;
            if let Some(min) = field.min {
                if number < min {
                    return Err(bad(format!("must be at least {min}")));
                }
            }
            if let Some(max) = field.max {
                if number > max {
                    return Err(bad(format!("must be at most {max}")));
                }
            }
        }
        PropertyControl::Select => {
            if let Some(options) = &field.options {
                if !options.iter().any(|option| option.value == value) {
                    return Err(bad(format!("`{value}` is not one of the allowed values")));
                }
            }
        }
        PropertyControl::Text | PropertyControl::Password | PropertyControl::Textarea => {
            if value.contains('\n') && field.control != PropertyControl::Textarea {
                return Err(bad("must be a single line".into()));
            }
        }
    }
    Ok(())
}

/// World-owned keys, derived from the World's fields.
pub fn world_values(world: &World) -> StringMap {
    let bool_str = |value: bool| value.to_string();
    BTreeMap::from([
        ("level-name".into(), world.folder.clone()),
        ("level-seed".into(), world.seed.clone().unwrap_or_default()),
        ("level-type".into(), world.level_type.as_str().into()),
        ("generate-structures".into(), bool_str(world.generate_structures)),
        ("max-world-size".into(), world.max_world_size.to_string()),
        ("allow-nether".into(), bool_str(world.allow_nether)),
        ("difficulty".into(), world.difficulty.as_str().into()),
        ("gamemode".into(), world.gamemode.as_str().into()),
        ("hardcore".into(), bool_str(world.hardcore)),
        ("pvp".into(), bool_str(world.pvp)),
        ("allow-flight".into(), bool_str(world.allow_flight)),
        ("enable-command-block".into(), bool_str(world.enable_command_block)),
        ("spawn-protection".into(), world.spawn_protection.to_string()),
        ("spawn-monsters".into(), bool_str(world.spawn_monsters)),
        ("spawn-npcs".into(), bool_str(world.spawn_npcs)),
        ("initial-enabled-packs".into(), world.data_packs.enabled.join(",")),
        ("initial-disabled-packs".into(), world.data_packs.disabled.join(",")),
    ])
}

/// The full `server.properties` for one launch.
pub fn render(world: &World, profile_overrides: &StringMap, rcon_password: &str, query_enabled: bool) -> String {
    let mut values: StringMap = SCHEMA.iter().map(|field| (field.key.clone(), field.default_value.clone())).collect();
    values.extend(profile_overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
    values.extend(world_values(world));
    // Managed by Lodestone: the console, player actions and backups need RCON,
    // and the container always listens on fixed ports that Docker maps.
    values.insert("enable-rcon".into(), "true".into());
    values.insert("rcon.port".into(), CONTAINER_RCON_PORT.to_string());
    values.insert("rcon.password".into(), rcon_password.into());
    values.insert("server-port".into(), CONTAINER_GAME_PORT.to_string());
    values.insert("server-ip".into(), String::new());
    values.insert("enable-query".into(), query_enabled.to_string());
    values.insert("query.port".into(), CONTAINER_QUERY_PORT.to_string());

    let mut out = String::from("# Written by Lodestone on every start. Edit settings in the Lodestone UI.\n");
    for (key, value) in values {
        out.push_str(&key);
        out.push('=');
        out.push_str(&escape(&value));
        out.push('\n');
    }
    out
}

/// Escapes a value the way `java.util.Properties` reads it.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (index, ch) in value.chars().enumerate() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ' ' if index == 0 => out.push_str("\\ "),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(ch);
            }
            ch if ch.is_ascii() => out.push(ch),
            ch => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out
}

/// Parses `server.properties` text (used to read files back from disk).
pub fn parse(text: &str) -> StringMap {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('!'))
        .filter_map(|line| {
            let (key, value) = split_unescaped(line)?;
            Some((key.trim().to_owned(), unescape(value)))
        })
        .collect()
}

fn split_unescaped(line: &str) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '=' | ':' => return Some((&line[..index], &line[index + 1..])),
            _ => {}
        }
    }
    None
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    let mut pending_high: Option<u16> = None;
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Ok(unit) = u16::from_str_radix(&hex, 16) {
                    if let Some(high) = pending_high.take() {
                        if let Some(ch) = char::decode_utf16([high, unit]).next().and_then(Result::ok) {
                            out.push(ch);
                        }
                    } else if (0xD800..0xDC00).contains(&unit) {
                        pending_high = Some(unit);
                    } else if let Some(ch) = char::from_u32(unit as u32) {
                        out.push(ch);
                    }
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_world() -> World {
        serde_json::from_value(serde_json::json!({
            "id": "world-1", "name": "Test", "folder": "test", "levelType": "minecraft:flat",
            "seed": "42", "generateStructures": false, "generated": false, "layout": "vanilla",
            "dataVersion": null, "maxWorldSize": 1000, "allowNether": false, "difficulty": "hard",
            "gamemode": "creative", "hardcore": false, "spawnProtection": 0, "spawnMonsters": true,
            "spawnNpcs": true, "pvp": false, "allowFlight": true, "enableCommandBlock": true,
            "dataPacks": { "enabled": ["vanilla"], "disabled": [] }, "sizeBytes": 0,
            "createdAt": "", "updatedAt": "", "attachedInstanceId": null
        }))
        .unwrap()
    }

    #[test]
    fn every_key_has_one_owner() {
        let mut seen = std::collections::HashSet::new();
        for field in schema() {
            assert!(seen.insert(&field.key), "duplicate key {}", field.key);
        }
        assert!(field("level-name").is_some_and(|f| f.scope == PropertyScope::World));
        assert!(field("motd").is_some_and(|f| f.scope == PropertyScope::Profile));
    }

    #[test]
    fn rejects_edits_on_the_wrong_owner() {
        let world_key = StringMap::from([("difficulty".into(), "hard".into())]);
        assert!(validate_profile_changes(&world_key).unwrap_err().to_string().contains("belongs to the world"));
        let managed = StringMap::from([("rcon.port".into(), "1".into())]);
        assert!(validate_profile_changes(&managed).unwrap_err().to_string().contains("managed by Lodestone"));
    }

    #[test]
    fn validates_values() {
        assert!(validate_profile_changes(&StringMap::from([("max-players".into(), "50".into())])).is_ok());
        assert!(validate_profile_changes(&StringMap::from([("max-players".into(), "lots".into())])).is_err());
        assert!(validate_profile_changes(&StringMap::from([("online-mode".into(), "yes".into())])).is_err());
        assert!(validate_profile_changes(&StringMap::from([("nonsense".into(), "1".into())])).is_err());
    }

    #[test]
    fn render_merges_owners_and_forces_managed_keys() {
        let overrides = StringMap::from([("motd".into(), "Héllo = world".into()), ("rcon.port".into(), "1".into())]);
        let text = render(&sample_world(), &overrides, "s3cret", false);
        let parsed = parse(&text);
        assert_eq!(parsed["motd"], "Héllo = world");
        assert_eq!(parsed["level-name"], "test");
        assert_eq!(parsed["level-type"], "minecraft:flat");
        assert_eq!(parsed["difficulty"], "hard");
        assert_eq!(parsed["enable-rcon"], "true");
        assert_eq!(parsed["rcon.port"], "25575");
        assert_eq!(parsed["rcon.password"], "s3cret");
        assert_eq!(parsed["max-players"], "20");
    }

    #[test]
    fn escapes_round_trip() {
        for value in ["plain", " leading", "a=b:c#d!e", "back\\slash", "emoji 🎮", "§aColour"] {
            let line = format!("k={}", escape(value));
            assert_eq!(parse(&line)["k"], value, "value {value:?}");
        }
    }

    #[test]
    fn profile_fields_hide_secrets() {
        let overrides = StringMap::from([("rcon.password".into(), "nope".into())]);
        let fields = profile_fields(&overrides);
        assert!(fields.iter().all(|field| field.scope == PropertyScope::Profile));
        assert_eq!(fields.iter().find(|f| f.key == "rcon.password").unwrap().value, "");
    }
}
