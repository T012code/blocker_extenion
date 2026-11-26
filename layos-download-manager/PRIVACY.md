# Torrent privacy in Layó's Download Manager

This page explains what the torrent privacy settings protect, how, and what they can't do.

## The one thing to know

**BitTorrent shows your IP address to everyone you share with.** Every peer that sends you a piece, every tracker, and every DHT node you ask about a torrent sees the address your traffic comes from. That's how the protocol works, and no client setting changes it.

So the protection is to make sure that address is **your VPN's or proxy's, never your own**, and to stop the moment that protection drops instead of quietly carrying on over your real connection. That's what Layó's Download Manager does.

## Default: torrents wait for protection

Out of the box, **torrents don't start until a VPN or proxy is set up.** A torrent added before that shows **Waiting for VPN** and contacts no one: no peers, no trackers, no DHT. This is tested; see *How it's tested* below.

You can allow torrents without a VPN in *Settings → Privacy*, but it's a deliberate choice, not something that happens by accident.

## The modes

| | Proxy (recommended) | VPN adapter | Private | Standard |
| --- | :---: | :---: | :---: | :---: |
| Peers see the VPN's address, not yours | ✅ | ✅ | ❌ | ❌ |
| Kill switch: torrents pause if protection drops | ✅ instant | ✅ within ~1 s | — | — |
| Traffic can't leave any other way | ✅ | ✅ Linux/macOS · ⚠️ Windows* | — | — |
| Hostname lookups hidden from your DNS provider | ✅ | via VPN | ❌ | ❌ |
| Not announced to your local network | ✅ | ✅ | ✅ | ❌ |
| No port opened on your router (UPnP) | ✅ | ✅ | ✅ | ❌ |
| Accepts incoming connections | ❌ | ✅ (on the VPN address only) | ✅ | ✅ |
| DHT (finds peers for magnets without trackers) | ❌ | ✅ · ❌ on Windows* | optional | optional |

\* *Windows can't tie a program's network connections to one adapter the way Linux and macOS can. In VPN adapter mode on Windows the kill switch watches the adapter and pauses all torrents within about a second of it dropping, and the DHT (which keeps talking in the background) is switched off. For airtight protection on Windows, use **Proxy mode**, or also turn on your VPN app's own kill switch.*

### Proxy mode (recommended)

Use the SOCKS5 proxy most VPN providers include (look for "SOCKS5" in your VPN account; it usually only works while the VPN is connected, which is exactly what you want).

- Every connection to a peer, every tracker request and the torrent metadata go through the proxy.
- Hostnames are resolved by the proxy (`socks5h`), so your DNS provider doesn't see which trackers you use.
- Anything that can't go through a SOCKS5 proxy is switched off rather than allowed to leak: incoming connections, the DHT, local peer discovery, UPnP, and **UDP trackers, which are removed from every torrent and magnet link**.
- If the proxy stops answering, connections simply fail: there's no fallback to your own connection. The kill switch notices within a second and shows *Waiting for VPN*; downloads carry on when the proxy is back, without re-downloading anything.

Trade-off: only HTTP(S) trackers can be used, so some torrents find fewer peers, and torrents that only list UDP trackers may find none.

### VPN adapter mode

Choose your VPN's network adapter (for example *Mullvad*, *NordLynx*, *ProtonVPN*, *wg0*). Torrents only run while it's connected, and only accept incoming connections on the VPN's address. On Linux and macOS every torrent socket is bound to the adapter, so traffic physically can't use another connection.

### Private and Standard

No VPN: peers see your real IP. *Private* still avoids announcing your downloads to the local network and doesn't open a port on your router. *Standard* turns everything on for the most peers.

## Always, in every mode

- **Blending in, not standing out.** The client identifies itself as plain *rqbit*, the open-source torrent engine inside, used by many other people, rather than with a unique name. A unique fingerprint would make you easier to pick out.
- **No cross-session ID.** The random part of the peer ID is new every time the app starts.
- **IP blocklist (optional).** Give a blocklist address or file (P2P, eMule or DAT format) and the client never talks to those addresses.
- **No hidden uploads.** After finishing, a torrent shares until you've uploaded as much as you downloaded, then stops. You can stop sharing any time, or set the ratio to 0.

## What this can't do

Being honest about limits is part of good security:

- **Your VPN or proxy provider can see your torrent traffic.** Choose one with a no-logs policy that allows P2P.
- **Peers still see the VPN's IP, and what you download.** Privacy here means they can't connect it to *you*; it doesn't make the activity invisible.
- **Torrent traffic isn't disguised.** Without a VPN or proxy, your internet provider can tell you're using BitTorrent. (Protocol encryption isn't available in the engine; a VPN or proxy hides it completely.)
- **Downloads from websites** (not torrents) use your normal connection, like your browser.
- **Tor isn't supported for torrents**, on purpose: BitTorrent over Tor is slow, overloads the Tor network, and leaks easily. Use a VPN.
- **Legal use is your responsibility.** Protection is for your privacy, not a licence to share what you don't have the rights to.

## How it's tested

The tests in `crates/torrent/tests/torrent.rs` run real transfers between two BitTorrent sessions:

- **Proxy mode:** the download completes, and the test proxy's log shows the peer was reached through it.
- **Kill switch:** the proxy is cut mid-download. The torrent stops within about 50 ms, makes no progress while blocked, and finishes byte-for-byte correctly when the proxy returns.
- **Default protection:** with no VPN or proxy, a torrent waits and a honeypot peer receives **zero** connection attempts.
- **VPN mode:** with the adapter missing, the torrent waits and contacts no one; with the adapter up (loopback on Linux), the download completes over it.

The rules (which features each mode switches off, removing UDP trackers from magnets and `.torrent` files while leaving the torrent's identity untouched, proxy address handling) have unit tests in `crates/torrent/src/privacy.rs`.
