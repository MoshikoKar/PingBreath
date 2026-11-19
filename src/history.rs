//! 24-hour rolling history, stored as one aggregate bucket per minute.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MINUTES_24H: u64 = 24 * 60;

#[derive(Serialize, Deserialize, Clone, Copy, Default, Debug)]
pub struct Bucket {
    /// Minutes since the Unix epoch.
    pub minute: u64,
    pub sent: u32,
    pub recv: u32,
    pub sum_ms: u64,
    pub max_ms: u32,
}

pub fn now_minute() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() / 60).unwrap_or(0)
}

pub fn key(name: &str, host: &str) -> String {
    format!("{name}|{host}")
}

fn path() -> PathBuf {
    crate::config::path().with_file_name("history.json")
}

pub fn load() -> HashMap<String, Vec<Bucket>> {
    let now = now_minute();
    let mut map: HashMap<String, Vec<Bucket>> = std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for v in map.values_mut() {
        v.retain(|b| b.minute + MINUTES_24H > now);
    }
    map
}

pub fn save(map: &HashMap<String, Vec<Bucket>>) {
    if let Ok(s) = serde_json::to_string(map) {
        let _ = std::fs::write(path(), s);
    }
}

pub struct Summary {
    pub sent: u64,
    pub uptime_pct: f32,
    pub max_ms: u32,
    /// 24 hourly health codes, oldest first: 0 no data, 1 good, 2 degraded, 3 bad.
    pub hours: Vec<i32>,
    /// Hover text for each of the 24 hourly segments.
    pub tips: Vec<String>,
}

/// Minutes to add to UTC to get local time (0 if unavailable).
fn local_offset_min() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    let r = unsafe { GetTimeZoneInformation(&mut tz) };
    let extra = match r {
        1 => tz.StandardBias, // standard time
        2 => tz.DaylightBias, // daylight time
        _ => 0,
    };
    -((tz.Bias + extra) as i64)
}

fn clock(minute: u64, off: i64) -> String {
    let m = (minute as i64 + off).rem_euclid(1440);
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub fn summarize(buckets: &[Bucket]) -> Summary {
    let now = now_minute();
    let (mut sent, mut recv, mut max) = (0u64, 0u64, 0u32);
    let mut hours = vec![(0u64, 0u64, 0u64); 24]; // sent, recv, sum per hour
    let mut hmax = vec![0u32; 24];
    let mut lossy: Vec<Vec<(u64, bool)>> = vec![Vec::new(); 24]; // (minute, fully down)

    for b in buckets {
        if b.minute + MINUTES_24H <= now {
            continue;
        }
        sent += b.sent as u64;
        recv += b.recv as u64;
        max = max.max(b.max_ms);
        let age = now.saturating_sub(b.minute);
        let slot = 23 - (age / 60).min(23) as usize; // 23 = most recent hour
        hours[slot].0 += b.sent as u64;
        hours[slot].1 += b.recv as u64;
        hours[slot].2 += b.sum_ms;
        hmax[slot] = hmax[slot].max(b.max_ms);
        if b.sent > b.recv {
            lossy[slot].push((b.minute, b.recv == 0));
        }
    }
    let off = local_offset_min();

    let codes = hours
        .iter()
        .map(|&(s, r, ms)| {
            if s == 0 {
                return 0;
            }
            let loss = 100.0 * (s - r) as f32 / s as f32;
            let avg = if r > 0 { ms as f32 / r as f32 } else { f32::MAX };
            if loss >= 10.0 || avg >= 200.0 {
                3
            } else if loss >= 2.0 || avg >= 80.0 {
                2
            } else {
                1
            }
        })
        .collect();

    let tips = (0..24)
        .map(|i| {
            let (s, r, ms) = hours[i];
            let end = now.saturating_sub((23 - i as u64) * 60);
            let start = end.saturating_sub(60);
            let range = format!("{}-{}", clock(start, off), clock(end, off));
            if s == 0 {
                return format!("{range} · no data");
            }
            let mut v = lossy[i].clone();
            v.sort();
            let mut out = format!("{range} · ");
            if v.is_empty() {
                out += "no loss";
            } else {
                // contiguous runs of minutes with loss
                let mut runs: Vec<(u64, u64, bool)> = Vec::new();
                for (m, down) in v {
                    match runs.last_mut() {
                        Some(l) if m == l.1 + 1 => {
                            l.1 = m;
                            l.2 |= down;
                        }
                        _ => runs.push((m, m, down)),
                    }
                }
                let total: u64 = runs.iter().map(|r| r.1 - r.0 + 1).sum();
                let kind = if runs.iter().all(|r| r.2) { "down" } else { "loss" };
                out += &format!("{total} min {kind} (");
                for (k, r) in runs.iter().take(2).enumerate() {
                    if k > 0 {
                        out += ", ";
                    }
                    out += &if r.0 == r.1 {
                        format!("{}", clock(r.0, off))
                    } else {
                        format!("{}-{}", clock(r.0, off), clock(r.1 + 1, off))
                    };
                }
                if runs.len() > 2 {
                    out += &format!(" +{}", runs.len() - 2);
                }
                out += ")";
            }
            let loss = 100.0 * (s - r) as f32 / s as f32;
            if r > 0 {
                out += &format!(" · {loss:.1}% lost · avg {} ms · max {} ms", ms / r, hmax[i]);
            } else {
                out += " · 100% lost";
            }
            out
        })
        .collect();

    Summary {
        tips,
        sent,
        uptime_pct: if sent == 0 { 0.0 } else { 100.0 * recv as f32 / sent as f32 },
        max_ms: max,
        hours: codes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_no_data() {
        let s = summarize(&[]);
        assert_eq!(s.sent, 0);
        assert!(s.hours.iter().all(|&h| h == 0));
    }

    #[test]
    fn uptime_and_max_and_recent_hour() {
        let now = now_minute();
        let b = Bucket { minute: now, sent: 10, recv: 9, sum_ms: 90, max_ms: 25 };
        let s = summarize(&[b]);
        assert_eq!(s.sent, 10);
        assert!((s.uptime_pct - 90.0).abs() < 0.01);
        assert_eq!(s.max_ms, 25);
        assert_eq!(s.hours[23], 3); // 10 % loss is "bad"
    }

    #[test]
    fn old_buckets_ignored() {
        let now = now_minute();
        let b = Bucket { minute: now.saturating_sub(MINUTES_24H + 5), sent: 5, recv: 5, sum_ms: 5, max_ms: 1 };
        assert_eq!(summarize(&[b]).sent, 0);
    }
}
