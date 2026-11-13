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
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for Config {
    fn default() -> Self {
        let t = |n: &str, h: &str| Target { name: n.into(), host: h.into() };
        Config {
            targets: vec![t("Router", GATEWAY), t("Google", "8.8.8.8"), t("Cloudflare", "1.1.1.1")],
            interval_ms: 1000,
            timeout_ms: 2000,
            always_on_top: true,
            locked: false,
            check_updates: true,
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
fn normalize(c: &mut Config) {
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
        None => Target { name: "Router".into(), host: GATEWAY.into() },
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
