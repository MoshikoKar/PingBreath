use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Host value that means "auto-detect the default gateway".
pub const GATEWAY: &str = "gateway";
const MAX_TARGETS: usize = 8;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Target {
    pub name: String,
    /// IPv4 address, hostname, or the special value "gateway" (auto-detected).
    pub host: String,
    /// Latency (ms) at/above which the target shows as degraded. Default 80.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn_ms: Option<u32>,
    /// Latency (ms) at/above which the target shows as bad. Default 200.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bad_ms: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Destinations to monitor. The router ("gateway", or a fixed router IP) is
    /// mandatory and always kept first; everything after it is yours to change.
    pub targets: Vec<Target>,
    pub interval_ms: u64,
    pub timeout_ms: u32,
    pub always_on_top: bool,
    pub locked: bool,
    /// Check GitHub for a newer release at startup (a prompt appears only if one exists).
    pub check_updates: bool,
    /// "stable" (default) or "beta" (also offers pre-releases).
    pub update_channel: String,
    /// Windows toast when a target goes down / recovers.
    pub notifications: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for Config {
    fn default() -> Self {
        let t = |n: &str, h: &str| Target { name: n.into(), host: h.into(), warn_ms: None, bad_ms: None };
        Config {
            targets: vec![t("Router", GATEWAY), t("Google", "8.8.8.8"), t("Cloudflare", "1.1.1.1")],
            interval_ms: 1000,
            timeout_ms: 2000,
            always_on_top: true,
            locked: false,
            check_updates: true,
            update_channel: "stable".into(),
            notifications: true,
            x: None,
            y: None,
        }
    }
}

/// Folder the running exe lives in.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

/// Portable mode: a `portable.txt` next to the exe keeps config + history beside it.
pub fn is_portable() -> bool {
    exe_dir().is_some_and(|d| d.join("portable.txt").exists())
}

pub fn path() -> PathBuf {
    if is_portable() {
        if let Some(d) = exe_dir() {
            return d.join("data").join("config.json");
        }
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("NetPulse").join("config.json")
}

/// Router must exist and come first; drop blank/duplicate entries; cap the count.
pub(crate) fn normalize(c: &mut Config) {
    c.targets.retain(|t| !t.host.trim().is_empty());
    for t in &mut c.targets {
        t.host = t.host.trim().to_string();
        if t.name.trim().is_empty() {
            t.name = t.host.clone();
        }
    }
    // The router is the first target whose host is "gateway"; if none, add one.
    let pos = c.targets.iter().position(|t| t.host.eq_ignore_ascii_case(GATEWAY));
    let router = match pos {
        Some(i) => c.targets.remove(i),
        None => Target { name: "Router".into(), host: GATEWAY.into(), warn_ms: None, bad_ms: None },
    };
    c.targets.insert(0, router);
    let mut seen = std::collections::HashSet::new();
    c.targets.retain(|t| seen.insert(t.host.to_ascii_lowercase()));
    c.targets.truncate(MAX_TARGETS);
}

pub fn load() -> Config {
    let p = path();
    match std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok()) {
        Some(mut c) => {
            normalize(&mut c);
            c
        }
        None => {
            let c = Config::default();
            save(&c);
            c
        }
    }
}

pub fn save(c: &Config) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(c) {
        let _ = std::fs::write(p, s);
    }
}

/// Load, modify, save — used for position / toggle changes.
pub fn update(f: impl FnOnce(&mut Config)) {
    let mut c = load();
    f(&mut c);
    save(&c);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: &str, h: &str) -> Target {
        Target { name: n.into(), host: h.into(), warn_ms: None, bad_ms: None }
    }

    #[test]
    fn router_is_added_when_missing() {
        let mut c = Config { targets: vec![t("Google", "8.8.8.8")], ..Config::default() };
        normalize(&mut c);
        assert_eq!(c.targets[0].host, GATEWAY);
        assert_eq!(c.targets.len(), 2);
    }

    #[test]
    fn router_is_moved_first() {
        let mut c = Config { targets: vec![t("Google", "8.8.8.8"), t("GW", "Gateway")], ..Config::default() };
        normalize(&mut c);
        assert_eq!(c.targets[0].name, "GW");
    }

    #[test]
    fn blanks_and_duplicates_dropped_and_capped() {
        let mut targets = vec![t("a", "  "), t("Dup", "1.1.1.1"), t("Dup2", "1.1.1.1")];
        for i in 0..20 {
            targets.push(t("x", &format!("10.0.0.{i}")));
        }
        let mut c = Config { targets, ..Config::default() };
        normalize(&mut c);
        assert!(c.targets.len() <= MAX_TARGETS);
        assert_eq!(c.targets.iter().filter(|t| t.host == "1.1.1.1").count(), 1);
        assert!(c.targets.iter().all(|t| !t.host.trim().is_empty()));
    }

    #[test]
    fn old_config_without_new_keys_still_parses() {
        let c: Config = serde_json::from_str(r#"{"interval_ms":500,"targets":[{"name":"R","host":"gateway"}]}"#).unwrap();
        assert_eq!(c.interval_ms, 500);
        assert!(c.always_on_top && c.notifications && c.check_updates);
        assert_eq!(c.update_channel, "stable");
    }
}
