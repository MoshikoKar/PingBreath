use std::collections::VecDeque;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY,
};

use crate::history::{self, Bucket};

pub const HISTORY: usize = 60;

#[derive(Default)]
pub struct Stats {
    /// Latest samples for the live sparkline; None = timeout / unreachable.
    pub history: VecDeque<Option<u32>>,
    /// Per-minute buckets covering the last 24 h.
    pub buckets: VecDeque<Bucket>,
    pub resolved: Option<Ipv4Addr>,
    /// Network adapter name (only for the "gateway" target).
    pub adapter: Option<String>,
}

pub type Shared = Arc<Mutex<Stats>>;

fn resolve(host: &str) -> Option<(Ipv4Addr, Option<String>)> {
    if host.eq_ignore_ascii_case("gateway") {
        return crate::net::default_gateway().map(|g| (g.ip, Some(g.adapter)));
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Some((ip, None));
    }
    (host, 0).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
        std::net::IpAddr::V4(v4) => Some((v4, None)),
        _ => None,
    })
}

/// One ICMP echo via the Win32 IP Helper API. Returns RTT in ms.
fn echo(handle: windows::Win32::Foundation::HANDLE, ip: Ipv4Addr, timeout_ms: u32) -> Option<u32> {
    let payload = [0x61u8; 32];
    let mut buf = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8];
    let n = unsafe {
        IcmpSendEcho(
            handle,
            u32::from_ne_bytes(ip.octets()),
            payload.as_ptr() as *const _,
            payload.len() as u16,
            None,
            buf.as_mut_ptr() as *mut _,
            buf.len() as u32,
            timeout_ms,
        )
    };
    if n == 0 {
        return None;
    }
    let reply = unsafe { &*(buf.as_ptr() as *const ICMP_ECHO_REPLY) };
    if reply.Status == 0 { Some(reply.RoundTripTime) } else { None }
}

pub fn spawn(
    host: String,
    interval: Duration,
    timeout_ms: u32,
    stop: Arc<AtomicBool>,
    initial: Vec<Bucket>,
) -> Shared {
    let shared: Shared = Arc::new(Mutex::new(Stats {
        buckets: initial.into(),
        ..Stats::default()
    }));
    let out = shared.clone();
    let is_gateway = host.eq_ignore_ascii_case("gateway");

    std::thread::spawn(move || {
        let Ok(handle) = (unsafe { IcmpCreateFile() }) else { return };
        let (mut ip, mut adapter) = match resolve(&host) {
            Some((i, a)) => (Some(i), a),
            None => (None, None),
        };
        let mut fails = 0u32;
        let mut tick = 0u32;

        while !stop.load(Ordering::Relaxed) {
            let started = std::time::Instant::now();
            let sample = ip.and_then(|a| echo(handle, a, timeout_ms));
            fails = if sample.is_some() { 0 } else { fails + 1 };
            tick = tick.wrapping_add(1);

            // The gateway follows the network (Wi-Fi <-> Ethernet, VPN, docking), so re-check
            // it every ~10 s. Other hosts are only re-resolved after repeated failures.
            let recheck = ip.is_none() || (is_gateway && tick % 10 == 0) || (!is_gateway && fails % 5 == 4);
            if recheck {
                if let Some((i, a)) = resolve(&host) {
                    ip = Some(i);
                    adapter = a;
                } else if is_gateway {
                    ip = None;
                    adapter = None;
                }
            }

            {
                let mut s = out.lock().unwrap();
                s.resolved = ip;
                s.adapter = adapter.clone();

                if s.history.len() == HISTORY {
                    s.history.pop_front();
                }
                s.history.push_back(sample);

                let m = history::now_minute();
                if s.buckets.back().map_or(true, |b| b.minute != m) {
                    s.buckets.push_back(Bucket { minute: m, ..Bucket::default() });
                }
                while s.buckets.front().map_or(false, |b| b.minute + history::MINUTES_24H <= m) {
                    s.buckets.pop_front();
                }
                let b = s.buckets.back_mut().unwrap();
                b.sent += 1;
                if let Some(ms) = sample {
                    b.recv += 1;
                    b.sum_ms += ms as u64;
                    b.max_ms = b.max_ms.max(ms);
                }
            }

            if let Some(rest) = interval.checked_sub(started.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        unsafe {
            let _ = IcmpCloseHandle(handle);
        }
    });
    shared
}
