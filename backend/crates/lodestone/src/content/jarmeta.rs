//! What an uploaded jar says about itself: `plugin.yml` (Bukkit/Paper),
//! `fabric.mod.json` (Fabric) or `META-INF/[neoforge.]mods.toml` (Forge,
//! NeoForge). Read with tiny line parsers; only a few keys are needed.

use std::io::{Cursor, Read};

use crate::model::ModKind;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct JarMeta {
    pub name: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    /// What the jar is, judging by which descriptor it has.
    pub kind: Option<ModKind>,
}

/// `None` when the bytes aren't a zip (so not a jar at all).
pub fn read(bytes: &[u8]) -> Option<JarMeta> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut text = |name: &str| -> Option<String> {
        let mut file = archive.by_name(name).ok()?;
        if file.size() > 1 << 20 {
            return None;
        }
        let mut out = String::new();
        file.read_to_string(&mut out).ok()?;
        Some(out)
    };
    if let Some(yaml) = text("paper-plugin.yml").or_else(|| text("plugin.yml")) {
        return Some(plugin_yml(&yaml));
    }
    if let Some(json) = text("fabric.mod.json") {
        return Some(fabric_json(&json));
    }
    if let Some(toml) = text("META-INF/neoforge.mods.toml").or_else(|| text("META-INF/mods.toml")) {
        return Some(mods_toml(&toml));
    }
    Some(JarMeta::default())
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches(|ch| ch == '"' || ch == '\'').trim().to_owned()
}

fn plugin_yml(yaml: &str) -> JarMeta {
    let mut meta = JarMeta { kind: Some(ModKind::Plugin), ..Default::default() };
    let mut in_authors = false;
    for line in yaml.lines() {
        // Top-level keys only; nested ones are indented.
        if line.starts_with([' ', '\t']) {
            if in_authors && meta.author.is_none() {
                if let Some(item) = line.trim().strip_prefix("- ") {
                    meta.author = Some(unquote(item));
                }
            }
            continue;
        }
        in_authors = false;
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = unquote(value);
        match key.trim() {
            "name" if !value.is_empty() => meta.name = Some(value),
            "version" if !value.is_empty() => meta.version = Some(value),
            "description" if !value.is_empty() => meta.description = Some(value),
            "author" if !value.is_empty() => meta.author = Some(value),
            "authors" => {
                let inline = value.trim_start_matches('[').trim_end_matches(']');
                match inline.split(',').map(unquote).find(|a| !a.is_empty()) {
                    Some(first) => meta.author = Some(first),
                    None => in_authors = true,
                }
            }
            _ => {}
        }
    }
    meta
}

fn fabric_json(json: &str) -> JarMeta {
    let value: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned).filter(|s| !s.is_empty());
    let author = value.get("authors").and_then(|a| a.as_array()).and_then(|list| list.first()).and_then(|first| {
        first.as_str().map(str::to_owned).or_else(|| first.get("name").and_then(|n| n.as_str()).map(str::to_owned))
    });
    JarMeta {
        name: text("name").or_else(|| text("id")),
        version: text("version"),
        author,
        description: text("description"),
        kind: Some(ModKind::Mod),
    }
}

fn mods_toml(toml: &str) -> JarMeta {
    let mut meta = JarMeta { kind: Some(ModKind::Mod), ..Default::default() };
    let mut in_mod = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            // Only the first [[mods]] table describes the jar.
            if in_mod && meta.name.is_some() {
                break;
            }
            in_mod = line == "[[mods]]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = unquote(value.split(" #").next().unwrap_or(value));
        let key = key.trim();
        if key == "authors" && meta.author.is_none() {
            meta.author = Some(value.clone()).filter(|v| !v.is_empty());
        }
        if !in_mod || value.is_empty() || value.contains("${") {
            continue;
        }
        match key {
            "displayName" => meta.name = Some(value),
            "version" => meta.version = Some(value),
            "description" => meta.description = Some(value),
            _ => {}
        }
    }
    meta
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    pub fn jar(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            let options = zip::write::SimpleFileOptions::default();
            for (name, body) in files {
                zip.start_file(*name, options).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        out.into_inner()
    }

    #[test]
    fn reads_a_bukkit_plugin() {
        let meta = read(&jar(&[(
            "plugin.yml",
            "name: TreeFeller\nversion: '2.1'\nmain: x.Y\nauthors:\n  - Ana\n  - Bo\ndescription: Fells trees\ncommands:\n  fell:\n    description: nested\n",
        )]))
        .unwrap();
        assert_eq!(meta.name.as_deref(), Some("TreeFeller"));
        assert_eq!(meta.version.as_deref(), Some("2.1"));
        assert_eq!(meta.author.as_deref(), Some("Ana"));
        assert_eq!(meta.description.as_deref(), Some("Fells trees"));
        assert_eq!(meta.kind, Some(ModKind::Plugin));
    }

    #[test]
    fn reads_fabric_and_forge_mods() {
        let fabric = read(&jar(&[(
            "fabric.mod.json",
            r#"{"id":"lithium","name":"Lithium","version":"0.15.1","authors":[{"name":"jellysquid3"}]}"#,
        )]))
        .unwrap();
        assert_eq!((fabric.name.as_deref(), fabric.author.as_deref()), (Some("Lithium"), Some("jellysquid3")));

        let forge = read(&jar(&[(
            "META-INF/neoforge.mods.toml",
            "modLoader=\"javafml\"\nauthors=\"Ann\"\n[[mods]]\nmodId=\"ferritecore\"\nversion=\"${file.jarVersion}\"\ndisplayName=\"FerriteCore\" # shown\n[[dependencies.ferritecore]]\nversion=\"1\"\n",
        )]))
        .unwrap();
        assert_eq!(forge.name.as_deref(), Some("FerriteCore"));
        assert_eq!(forge.version, None, "placeholders aren't versions");
        assert_eq!(forge.author.as_deref(), Some("Ann"));
    }

    #[test]
    fn rejects_what_isnt_a_jar() {
        assert_eq!(read(b"not a zip"), None);
        assert_eq!(read(&jar(&[("readme.txt", "hi")])), Some(JarMeta::default()));
    }
}
