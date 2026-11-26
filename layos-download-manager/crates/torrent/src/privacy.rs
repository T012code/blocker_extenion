//! Privacy protection for torrents.
//!
//! What BitTorrent can and can't hide: every peer you exchange data with, and every
//! tracker and DHT node you ask, sees the IP address your traffic comes from. Nothing
//! inside a torrent client changes that. What a client *can* do is make sure that
//! address is a VPN's or proxy's instead of yours, never fall back to your real
//! connection when that protection drops, and avoid announcing more than it must.
//!
//! The modes, from most to least protected:
//!
//! - **Proxy**: all torrent traffic goes through a SOCKS5 proxy (most VPN providers
//!   offer one). Hostnames are looked up through the proxy too, so DNS doesn't leak.
//!   Everything that can't go through a SOCKS5 proxy is switched off: incoming
//!   connections, the DHT, local peer discovery, UPnP, and UDP trackers (which are
//!   removed from every torrent). If the proxy goes away, connections fail rather than
//!   going direct, and the kill switch pauses torrents until it's back.
//! - **Vpn**: torrents only run while the chosen VPN network adapter is up. A watcher
//!   checks it every second and pauses every torrent the moment it drops (kill switch).
//!   On Linux and macOS every socket is also tied to that adapter, so traffic can't
//!   leave any other way. Windows can't tie sockets to an adapter this way, so there
//!   the DHT is switched off and the VPN app's own kill switch is recommended as well.
//! - **Private**: no VPN, but nothing extra is shared: no local network broadcasts of
//!   what you download, and no automatic port opening on your router (UPnP).
//! - **Standard**: everything on, for the most peers.
//!
//! In every mode the client identifies itself as plain "rqbit" (the open-source engine
//! inside), which many other people use, rather than as something unique. Its peer ID
//! gets a new random part every time the app starts, so you can't be followed across
//! sessions by it.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PrivacyMode {
    Standard,
    Private,
    /// `socks5://[user:password@]host:port`
    Proxy {
        url: String,
    },
    /// The VPN's network adapter, by name (e.g. "Mullvad", "wg0", "NordLynx").
    Vpn {
        interface: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Privacy {
    pub mode: PrivacyMode,
    /// Never run a torrent without a proxy or VPN. Applies in Standard and Private
    /// mode: torrents wait instead of using your real connection.
    pub require_protection: bool,
    /// Use the DHT to find peers (needed for magnet links without trackers). Ignored
    /// where it would leak: always off in Proxy mode, and in Vpn mode on Windows.
    pub dht: bool,
    /// A list of IP ranges to never talk to: an http(s) address or a file path
    /// (P2P / eMule / DAT formats, optionally gzipped).
    #[serde(default)]
    pub blocklist: Option<String>,
}

impl Default for Privacy {
    /// The safest setting that needs no set-up: nothing extra shared, and torrents wait
    /// until a VPN or proxy is set up rather than using the real connection.
    fn default() -> Self {
        Self {
            mode: PrivacyMode::Private,
            require_protection: true,
            dht: true,
            blocklist: None,
        }
    }
}

impl Privacy {
    pub fn is_protected(&self) -> bool {
        matches!(
            self.mode,
            PrivacyMode::Proxy { .. } | PrivacyMode::Vpn { .. }
        )
    }
}

/// How the torrent session is set up for a privacy setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub dht: bool,
    pub local_discovery: bool,
    pub upnp: bool,
    /// Accept incoming connections.
    pub listen: bool,
    /// Tie every socket to this adapter (not possible on Windows).
    pub bind_device: Option<String>,
    pub proxy: Option<String>,
    /// UDP trackers can't go through a SOCKS5 proxy; they're removed from torrents.
    pub strip_udp_trackers: bool,
}

pub fn plan(p: &Privacy, windows: bool) -> Result<Plan, String> {
    Ok(match &p.mode {
        PrivacyMode::Standard => Plan {
            dht: p.dht,
            local_discovery: true,
            upnp: true,
            listen: true,
            bind_device: None,
            proxy: None,
            strip_udp_trackers: false,
        },
        PrivacyMode::Private => Plan {
            dht: p.dht,
            local_discovery: false,
            upnp: false,
            listen: true,
            bind_device: None,
            proxy: None,
            strip_udp_trackers: false,
        },
        PrivacyMode::Proxy { url } => Plan {
            dht: false,
            local_discovery: false,
            upnp: false,
            listen: false,
            bind_device: None,
            proxy: Some(normalize_proxy(url)?),
            strip_udp_trackers: true,
        },
        PrivacyMode::Vpn { interface } => {
            if interface.trim().is_empty() {
                return Err("choose your VPN's network adapter".into());
            }
            Plan {
                dht: p.dht && !windows,
                local_discovery: false,
                upnp: false,
                listen: true,
                bind_device: (!windows).then(|| interface.trim().to_string()),
                proxy: None,
                strip_udp_trackers: false,
            }
        }
    })
}

/// Accepts `host:port`, `socks5://…` or `socks5h://…` and returns a `socks5h://` URL,
/// so hostnames (trackers, peers given by name) are looked up by the proxy, not
/// locally, which would reveal them to your DNS provider.
pub fn normalize_proxy(input: &str) -> Result<String, String> {
    let s = input.trim();
    let rest = match s.split_once("://") {
        Some((scheme, rest))
            if scheme.eq_ignore_ascii_case("socks5") || scheme.eq_ignore_ascii_case("socks5h") =>
        {
            rest
        }
        Some((scheme, _)) => {
            return Err(format!(
                "{scheme} proxies aren't supported; use a SOCKS5 proxy"
            ));
        }
        None => s,
    };
    let host_port = rest
        .rsplit_once('@')
        .map_or(rest, |(_, hp)| hp)
        .trim_end_matches('/');
    let ok = host_port
        .rsplit_once(':')
        .is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok_and(|p| p > 0));
    if !ok {
        return Err("the proxy address needs a host and a port, like 10.64.0.1:1080".into());
    }
    Ok(format!("socks5h://{}", rest.trim_end_matches('/')))
}

/// Removes `tr=udp://…` trackers from a magnet link.
pub fn strip_udp_trackers_magnet(magnet: &str) -> String {
    let Some((head, query)) = magnet.split_once('?') else {
        return magnet.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| {
            let Some(v) = kv.strip_prefix("tr=") else {
                return true;
            };
            let v = percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .to_ascii_lowercase();
            !v.starts_with("udp:")
        })
        .collect();
    format!("{head}?{}", kept.join("&"))
}

// ---- Minimal bencode reading, to edit a .torrent's tracker list without touching
// ---- the "info" part (re-encoding it could change the torrent's identity).

fn skip(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'i' => Some(i + b[i..].iter().position(|&c| c == b'e')? + 1),
        b'l' | b'd' => {
            let mut j = i + 1;
            while *b.get(j)? != b'e' {
                j = skip(b, j)?;
            }
            Some(j + 1)
        }
        b'0'..=b'9' => {
            let colon = i + b[i..].iter().position(|&c| c == b':')?;
            let len: usize = std::str::from_utf8(&b[i..colon]).ok()?.parse().ok()?;
            let end = colon + 1 + len;
            (end <= b.len()).then_some(end)
        }
        _ => None,
    }
}

fn read_str(b: &[u8], i: usize) -> Option<(&[u8], usize)> {
    let colon = i + b[i..].iter().position(|&c| c == b':')?;
    let len: usize = std::str::from_utf8(&b[i..colon]).ok()?.parse().ok()?;
    let start = colon + 1;
    b.get(start..start + len).map(|s| (s, start + len))
}

fn is_udp(url: &[u8]) -> bool {
    url.len() >= 4 && url[..4].eq_ignore_ascii_case(b"udp:")
}

fn bstr(s: &[u8]) -> Vec<u8> {
    let mut out = format!("{}:", s.len()).into_bytes();
    out.extend_from_slice(s);
    out
}

/// Returns the .torrent with UDP trackers removed from `announce` and `announce-list`.
/// Everything else, including the "info" part, is copied byte for byte.
pub fn strip_udp_trackers_torrent(b: &[u8]) -> Option<Vec<u8>> {
    if b.first() != Some(&b'd') {
        return None;
    }
    let mut out = vec![b'd'];
    let mut i = 1;
    let mut announce: Option<Vec<u8>> = None;
    let mut first_http: Option<Vec<u8>> = None;
    while *b.get(i)? != b'e' {
        let (key, after_key) = read_str(b, i)?;
        let end = skip(b, after_key)?;
        match key {
            b"announce" => {
                let (url, _) = read_str(b, after_key)?;
                announce = Some(url.to_vec());
                // Written later, once we know a replacement if it's a UDP tracker.
                out.extend_from_slice(&[0xFF]);
            }
            b"announce-list" => {
                // A list of tiers, each a list of URLs.
                let mut tiers = Vec::new();
                let mut j = after_key + 1;
                while *b.get(j)? != b'e' {
                    let tier_end = skip(b, j)?;
                    let mut urls = Vec::new();
                    let mut k = j + 1;
                    while k < tier_end - 1 {
                        let (url, next) = read_str(b, k)?;
                        if !is_udp(url) {
                            first_http.get_or_insert_with(|| url.to_vec());
                            urls.push(bstr(url));
                        }
                        k = next;
                    }
                    if !urls.is_empty() {
                        tiers.push(urls);
                    }
                    j = tier_end;
                }
                if !tiers.is_empty() {
                    out.extend(bstr(b"announce-list"));
                    out.push(b'l');
                    for t in tiers {
                        out.push(b'l');
                        for u in t {
                            out.extend(u);
                        }
                        out.push(b'e');
                    }
                    out.push(b'e');
                }
            }
            _ => out.extend_from_slice(&b[i..end]),
        }
        i = end;
    }
    out.push(b'e');
    // Put "announce" back where it was: kept, replaced by an http tracker, or dropped.
    let replacement = match announce {
        Some(url) if !is_udp(&url) => Some(url),
        Some(_) => first_http,
        None => None,
    };
    let pos = out.iter().position(|&c| c == 0xFF);
    if let Some(pos) = pos {
        let insert = replacement
            .map(|u| [bstr(b"announce"), bstr(&u)].concat())
            .unwrap_or_default();
        out.splice(pos..=pos, insert);
    }
    Some(out)
}

// ---- Kill switch

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Protection {
    /// Safe to run torrents.
    Ok,
    /// Torrents must wait; the reason is shown to the user.
    Blocked(String),
}

/// One check: is the protection the setting asks for in place right now?
pub async fn check(p: &Privacy) -> Protection {
    match &p.mode {
        PrivacyMode::Standard | PrivacyMode::Private if p.require_protection => Protection::Blocked(
            "Waiting for a VPN or proxy. Set one up in Settings → Privacy, or allow torrents without one.".into(),
        ),
        PrivacyMode::Standard | PrivacyMode::Private => Protection::Ok,
        PrivacyMode::Proxy { url } => match proxy_alive(url).await {
            Ok(()) => Protection::Ok,
            Err(e) => Protection::Blocked(format!("Paused for your privacy: the proxy isn't answering ({e}).")),
        },
        PrivacyMode::Vpn { interface } => match interface_ip(interface) {
            Some(_) => Protection::Ok,
            None => Protection::Blocked(format!("Paused for your privacy: the VPN adapter \"{interface}\" is disconnected.")),
        },
    }
}

/// The first address of a network adapter that's up, or None.
pub fn interface_ip(name: &str) -> Option<IpAddr> {
    let name = name.trim();
    if_addrs::get_if_addrs()
        .ok()?
        .into_iter()
        .filter(|i| i.name.eq_ignore_ascii_case(name))
        .map(|i| i.ip())
        .min_by_key(|ip| ip.is_ipv6())
}

/// Network adapters that have an address, for the settings page's adapter list.
pub fn adapters() -> Vec<String> {
    let mut names: Vec<String> = if_addrs::get_if_addrs()
        .map(|l| {
            l.into_iter()
                .filter(|i| !i.is_loopback())
                .map(|i| i.name)
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

/// Connects to the proxy and checks it speaks SOCKS5.
pub async fn proxy_alive(url: &str) -> Result<(), String> {
    let normalized = normalize_proxy(url)?;
    let rest = normalized.trim_start_matches("socks5h://");
    let host_port = rest.rsplit_once('@').map_or(rest, |(_, hp)| hp).to_string();
    let addr: SocketAddr = tokio::task::spawn_blocking(move || {
        host_port.to_socket_addrs().ok().and_then(|mut a| a.next())
    })
    .await
    .ok()
    .flatten()
    .ok_or("can't find the proxy's address")?;
    let work = async {
        let mut s = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| e.to_string())?;
        // Greeting: version 5, two methods offered: no authentication, user/password.
        s.write_all(&[5, 2, 0, 2])
            .await
            .map_err(|e| e.to_string())?;
        let mut reply = [0u8; 2];
        s.read_exact(&mut reply).await.map_err(|e| e.to_string())?;
        if reply[0] == 5 && reply[1] != 0xFF {
            Ok(())
        } else {
            Err("it isn't a SOCKS5 proxy".to_string())
        }
    };
    tokio::time::timeout(Duration::from_secs(4), work)
        .await
        .map_err(|_| "no answer".to_string())?
}

/// Re-checks the protection every `every` and publishes the result.
pub fn watch(p: Privacy, every: Duration) -> watch::Receiver<Protection> {
    let (tx, rx) = watch::channel(Protection::Blocked("Checking your VPN or proxy…".into()));
    tokio::spawn(async move {
        loop {
            let now = check(&p).await;
            tx.send_if_modified(|cur| {
                if *cur != now {
                    *cur = now;
                    true
                } else {
                    false
                }
            });
            if tx.is_closed() {
                break;
            }
            tokio::time::sleep(every).await;
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_close_every_leak_in_proxy_mode() {
        let p = Privacy {
            mode: PrivacyMode::Proxy {
                url: "10.64.0.1:1080".into(),
            },
            dht: true,
            ..Default::default()
        };
        let plan = plan(&p, true).unwrap();
        assert_eq!(plan.proxy.as_deref(), Some("socks5h://10.64.0.1:1080"));
        assert!(
            !plan.dht
                && !plan.local_discovery
                && !plan.upnp
                && !plan.listen
                && plan.strip_udp_trackers
        );
    }

    #[test]
    fn vpn_mode_per_platform() {
        let p = Privacy {
            mode: PrivacyMode::Vpn {
                interface: "wg0".into(),
            },
            dht: true,
            ..Default::default()
        };
        let linux = plan(&p, false).unwrap();
        assert_eq!(linux.bind_device.as_deref(), Some("wg0"));
        assert!(linux.dht && !linux.upnp && !linux.local_discovery);
        let windows = plan(&p, true).unwrap();
        assert_eq!(windows.bind_device, None);
        assert!(
            !windows.dht,
            "the DHT can't be tied to the adapter on Windows"
        );
        assert!(
            plan(
                &Privacy {
                    mode: PrivacyMode::Vpn {
                        interface: " ".into()
                    },
                    ..Default::default()
                },
                false
            )
            .is_err()
        );
    }

    #[test]
    fn private_and_standard() {
        let private = plan(&Privacy::default(), true).unwrap();
        assert!(!private.local_discovery && !private.upnp && private.proxy.is_none());
        let standard = plan(
            &Privacy {
                mode: PrivacyMode::Standard,
                ..Default::default()
            },
            true,
        )
        .unwrap();
        assert!(standard.local_discovery && standard.upnp);
    }

    #[test]
    fn proxy_addresses() {
        assert_eq!(
            normalize_proxy("socks5://user:pa:ss@proxy.vpn.example:1080/").unwrap(),
            "socks5h://user:pa:ss@proxy.vpn.example:1080"
        );
        assert_eq!(
            normalize_proxy("SOCKS5H://[::1]:9050").unwrap(),
            "socks5h://[::1]:9050"
        );
        assert!(normalize_proxy("http://proxy:8080").is_err());
        assert!(normalize_proxy("proxy.example").is_err());
        assert!(normalize_proxy("proxy:0").is_err());
    }

    #[test]
    fn magnets_lose_udp_trackers_only() {
        let m = "magnet:?xt=urn:btih:abc&dn=x&tr=udp%3A%2F%2Ftracker.a%3A1337&tr=https%3A%2F%2Ftracker.b%2Fannounce&tr=UDP://c:1";
        assert_eq!(
            strip_udp_trackers_magnet(m),
            "magnet:?xt=urn:btih:abc&dn=x&tr=https%3A%2F%2Ftracker.b%2Fannounce"
        );
    }

    #[test]
    fn torrent_files_lose_udp_trackers_and_keep_info() {
        let info = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let mut t = b"d8:announce20:udp://t.example:13377:comment2:hi13:announce-listll20:udp://t.example:1337el26:https://h.example/announceel13:udp://x.y:1e2ee4:info".to_vec();
        t.extend_from_slice(info);
        t.push(b'e');
        let out = strip_udp_trackers_torrent(&t).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert!(!s.contains("udp://"), "{s}");
        assert!(
            s.contains("8:announce26:https://h.example/announce"),
            "UDP announce replaced by the http one: {s}"
        );
        assert!(
            s.contains("13:announce-listll26:https://h.example/announceee"),
            "{s}"
        );
        assert!(s.contains("7:comment2:hi"));
        assert!(
            out.windows(info.len()).any(|w| w == info),
            "info copied byte for byte"
        );
        assert_eq!(skip(&out, 0), Some(out.len()), "still valid bencode");
        assert!(strip_udp_trackers_torrent(b"not bencode").is_none());
    }
}
