//! Parsing of Minecraft server output and command replies.
//!
//! Formats seen in the wild:
//!
//! ```text
//! [12:34:56] [Server thread/INFO]: Done (3.2s)! For help, type "help"      vanilla, fabric
//! [12:34:56 INFO]: Done (3.2s)! For help, type "help"                      paper
//! [12:34:56] [Server thread/INFO] [minecraft/DedicatedServer]: Done (...)  forge, neoforge
//! ```

use crate::model::LogLevel;

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed<'a> {
    pub level: LogLevel,
    pub message: &'a str,
}

/// Splits a raw log line into level and message. Lines that do not look
/// like log4j output (stack traces, plain prints) keep the whole line.
pub fn parse_line(raw: &str) -> Parsed<'_> {
    let line = raw.trim_end_matches(['\r', '\n']);
    if let Some(rest) = line.strip_prefix('[') {
        if let Some(close) = rest.find("]: ") {
            let header = &rest[..close];
            let message = &rest[close + 3..];
            return Parsed { level: level_in(header).unwrap_or(LogLevel::Info), message };
        }
    }
    let level = if is_stack_trace(line) { LogLevel::Error } else { LogLevel::Info };
    Parsed { level, message: line }
}

/// Unstructured output that belongs to a stack trace, e.g.
/// `java.lang.IllegalStateException: boom`, `\tat net.minecraft...`.
fn is_stack_trace(line: &str) -> bool {
    let first = line.split(':').next().unwrap_or("");
    line.starts_with("\tat ")
        || line.starts_with("Caused by")
        || line.starts_with("Exception")
        || (first.contains('.') && !first.contains(' ') && (first.ends_with("Exception") || first.ends_with("Error")))
}

fn level_in(header: &str) -> Option<LogLevel> {
    // The level is the last `/LEVEL]`-ish or ` LEVEL` token in the header.
    for (token, level) in [
        ("ERROR", LogLevel::Error),
        ("FATAL", LogLevel::Error),
        ("WARN", LogLevel::Warn),
        ("DEBUG", LogLevel::Debug),
        ("TRACE", LogLevel::Debug),
        ("INFO", LogLevel::Info),
    ] {
        if header.contains(&format!("/{token}]"))
            || header.ends_with(&format!("/{token}"))
            || header.ends_with(&format!(" {token}"))
            || header.contains(&format!(" {token}]"))
        {
            return Some(level);
        }
    }
    None
}

/// `Done (3.2s)! For help, type "help"` means the server accepts players.
pub fn is_done(message: &str) -> bool {
    message.starts_with("Done (") && message.contains("For help")
}

pub fn joined(message: &str) -> Option<&str> {
    message.strip_suffix(" joined the game").filter(|name| is_player_name(name))
}

pub fn left(message: &str) -> Option<&str> {
    message.strip_suffix(" left the game").filter(|name| is_player_name(name))
}

/// `UUID of player Steve is 8667ba71-b85a-4004-af54-457a9734eed7`
pub fn player_uuid(message: &str) -> Option<(&str, &str)> {
    let rest = message.strip_prefix("UUID of player ")?;
    let (name, uuid) = rest.split_once(" is ")?;
    is_player_name(name).then_some((name, uuid.trim()))
}

fn is_player_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Strips `§x` colour codes from command replies.
pub fn strip_colors(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '§' {
            chars.next();
        } else {
            out.push(ch);
        }
    }
    out
}

/// Paper's `tps`: `TPS from last 1m, 5m, 15m: 20.0, 19.98, 20.0` (first value).
pub fn parse_paper_tps(reply: &str) -> Option<f64> {
    let clean = strip_colors(reply);
    let (_, values) = clean.split_once(": ")?;
    values.split(',').next()?.trim().trim_start_matches('*').parse().ok()
}

/// Vanilla 1.20.3+ `tick query`: returns `(tps, ms_per_tick)`.
///
/// ```text
/// The game is running normally
/// Target tick rate: 20.0 per second.
/// Average time per tick: 3.4ms (Target: 50.0ms)
/// ```
pub fn parse_tick_query(reply: &str) -> Option<(f64, f64)> {
    let clean = strip_colors(reply);
    let target = clean
        .split("Target tick rate: ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.trim_end_matches('.').parse::<f64>().ok())
        .unwrap_or(20.0);
    let mspt: f64 = clean.split("Average time per tick: ").nth(1)?.split("ms").next()?.trim().parse().ok()?;
    let tps = if mspt <= 0.0 { target } else { (1000.0 / mspt).min(target) };
    Some((tps, mspt))
}

/// `time query day` / `time query daytime` reply: `The time is 1234`.
pub fn parse_time(reply: &str) -> Option<i64> {
    strip_colors(reply).trim().strip_prefix("The time is ")?.trim().parse().ok()
}

/// `0..24000` ticks to a wall-clock string, e.g. `06:00` at tick 0.
pub fn daytime_label(ticks: i64) -> String {
    let ticks = ticks.rem_euclid(24_000);
    let minutes = ((ticks as f64 / 1000.0 + 6.0) * 60.0) as i64 % (24 * 60);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Compares dotted Minecraft versions (`1.20.3` >= `1.20`). Snapshots and
/// unparseable versions return `None`.
pub fn version_at_least(version: &str, minimum: &str) -> Option<bool> {
    let parse = |value: &str| -> Option<Vec<u32>> { value.split('.').map(|part| part.parse().ok()).collect() };
    Some(parse(version)? >= parse(minimum)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vanilla_paper_and_forge_lines() {
        let vanilla = parse_line("[12:00:01] [Server thread/WARN]: Can't keep up!");
        assert_eq!(vanilla, Parsed { level: LogLevel::Warn, message: "Can't keep up!" });
        let paper = parse_line("[12:00:01 ERROR]: Could not load plugin");
        assert_eq!(paper, Parsed { level: LogLevel::Error, message: "Could not load plugin" });
        let forge = parse_line(
            "[12:00:01] [Server thread/INFO] [minecraft/DedicatedServer]: Done (1.0s)! For help, type \"help\"",
        );
        assert_eq!(forge.level, LogLevel::Info);
        assert!(is_done(forge.message));
        assert_eq!(parse_line("\tat net.minecraft.Foo").level, LogLevel::Error);
        assert_eq!(parse_line("java.lang.IllegalStateException: boom").level, LogLevel::Error);
        assert_eq!(parse_line("Loading 12 mods: fabric.api").level, LogLevel::Info);
        assert_eq!(parse_line("Starting net.minecraft.server.Main").message, "Starting net.minecraft.server.Main");
    }

    #[test]
    fn detects_players() {
        assert_eq!(joined("Steve joined the game"), Some("Steve"));
        assert_eq!(left("Alex_99 left the game"), Some("Alex_99"));
        assert_eq!(joined("<Steve> I joined the game"), None);
        assert_eq!(
            player_uuid("UUID of player Steve is 8667ba71-b85a-4004-af54-457a9734eed7"),
            Some(("Steve", "8667ba71-b85a-4004-af54-457a9734eed7"))
        );
    }

    #[test]
    fn parses_tps_replies() {
        assert_eq!(parse_paper_tps("§6TPS from last 1m, 5m, 15m: §a*20.0, §a19.98, §a20.0"), Some(20.0));
        let (tps, mspt) = parse_tick_query(
            "The game is running normally\nTarget tick rate: 20.0 per second.\nAverage time per tick: 4.0ms (Target: 50.0ms)",
        )
        .unwrap();
        assert_eq!(tps, 20.0);
        assert_eq!(mspt, 4.0);
        let (slow, _) = parse_tick_query("Average time per tick: 80.0ms (Target: 50.0ms)").unwrap();
        assert_eq!(slow, 12.5);
    }

    #[test]
    fn parses_time() {
        assert_eq!(parse_time("The time is 42"), Some(42));
        assert_eq!(daytime_label(0), "06:00");
        assert_eq!(daytime_label(18_000), "00:00");
    }

    #[test]
    fn compares_versions() {
        assert_eq!(version_at_least("1.20.3", "1.20.3"), Some(true));
        assert_eq!(version_at_least("1.21", "1.20.3"), Some(true));
        assert_eq!(version_at_least("1.20.1", "1.20.3"), Some(false));
        assert_eq!(version_at_least("24w14a", "1.20.3"), None);
    }
}
