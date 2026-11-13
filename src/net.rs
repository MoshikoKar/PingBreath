//! Default-gateway / network-card detection for multi-adapter machines.
//!
//! Instead of parsing `route print`, we ask Windows directly:
//!   * `GetIpForwardTable2`  -> every default route (prefix length 0) with its next hop + route metric
//!   * `GetAdaptersAddresses` -> adapter name, type, oper status and interface metric
//! and then pick the route Windows itself would prefer: an adapter that is Up, a physical
//! Ethernet / Wi-Fi / mobile card (not VPN, loopback, Hyper-V, VMware, WSL, TAP …), with the
//! lowest `route metric + interface metric`.

use std::net::Ipv4Addr;

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIpForwardTable2, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH, MIB_IPFORWARD_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::AF_INET;

#[derive(Clone, Debug)]
pub struct Gateway {
    pub ip: Ipv4Addr,
    /// Friendly adapter name, e.g. "Wi-Fi" or "Ethernet 2".
    pub adapter: String,
}

struct Adapter {
    index: u32,
    name: String,
    desc: String,
    if_type: u32,
    metric: u32,
}

const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_WIFI: u32 = 71;
const IF_TYPE_WWAN_GSM: u32 = 243;
const IF_TYPE_WWAN_CDMA: u32 = 244;

const VIRTUAL_HINTS: &[&str] = &[
    "virtual", "vmware", "hyper-v", "vethernet", "virtualbox", "vbox", "wsl", "tap-", "tap ", "tun",
    "wireguard", "tailscale", "zerotier", "openvpn", "nordlynx", "vpn", "loopback", "bluetooth",
    "docker", "npcap", "pseudo", "miniport", "teredo", "isatap", "6to4",
];

fn adapters() -> Vec<Adapter> {
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size = 16 * 1024u32;
    let mut out = Vec::new();
    unsafe {
        for _ in 0..5 {
            // u64 backing store keeps the buffer 8-byte aligned.
            let mut buf = vec![0u64; (size as usize) / 8 + 1];
            let head = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            let r = GetAdaptersAddresses(AF_INET.0 as u32, flags, None, Some(head), &mut size);
            if r == ERROR_BUFFER_OVERFLOW.0 {
                continue;
            }
            if r != ERROR_SUCCESS.0 {
                return out;
            }
            let mut p = head as *const IP_ADAPTER_ADDRESSES_LH;
            while !p.is_null() {
                let a = &*p;
                if a.OperStatus == IfOperStatusUp {
                    out.push(Adapter {
                        index: a.Anonymous1.Anonymous.IfIndex,
                        name: a.FriendlyName.to_string().unwrap_or_default(),
                        desc: a.Description.to_string().unwrap_or_default(),
                        if_type: a.IfType,
                        metric: a.Ipv4Metric,
                    });
                }
                p = a.Next;
            }
            break;
        }
    }
    out
}

fn is_physical(a: &Adapter) -> bool {
    let typ_ok = matches!(a.if_type, IF_TYPE_ETHERNET | IF_TYPE_WIFI | IF_TYPE_WWAN_GSM | IF_TYPE_WWAN_CDMA);
    let hay = format!("{} {}", a.name, a.desc).to_lowercase();
    typ_ok && !VIRTUAL_HINTS.iter().any(|h| hay.contains(h))
}

/// (interface index, next hop, route metric) for every IPv4 default route.
fn default_routes() -> Vec<(u32, Ipv4Addr, u32)> {
    let mut v = Vec::new();
    unsafe {
        let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        if GetIpForwardTable2(AF_INET, &mut table).0 != 0 || table.is_null() {
            return v;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        for r in rows {
            if r.DestinationPrefix.PrefixLength != 0 {
                continue;
            }
            let hop = Ipv4Addr::from(r.NextHop.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes());
            if hop.is_unspecified() {
                continue; // on-link default route, no gateway
            }
            v.push((r.InterfaceIndex, hop, r.Metric));
        }
        FreeMibTable(table as *const _);
    }
    v
}

/// The default gateway Windows would actually use, with the adapter it lives on.
pub fn default_gateway() -> Option<Gateway> {
    let adapters = adapters();
    let mut best: Option<((bool, u32), Gateway)> = None;
    for (index, hop, route_metric) in default_routes() {
        let Some(a) = adapters.iter().find(|a| a.index == index) else { continue };
        // Physical cards beat virtual/VPN ones; then lowest combined metric wins.
        let key = (!is_physical(a), route_metric.saturating_add(a.metric));
        if best.as_ref().map_or(true, |(k, _)| key < *k) {
            best = Some((key, Gateway { ip: hop, adapter: a.name.clone() }));
        }
    }
    best.map(|(_, g)| g)
}
