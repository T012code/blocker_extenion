# librqbit-tracker-comms 9.1.0 (modified)

A copy of [librqbit-tracker-comms](https://crates.io/crates/librqbit-tracker-comms) 9.1.0 from the
[rqbit](https://github.com/ikatson/rqbit) project by Igor Katson, licensed under the Apache License 2.0
(see `license` in `Cargo.toml`; full text: https://www.apache.org/licenses/LICENSE-2.0).

**Changed by Layó's Download Manager:** in `src/tracker_comms_udp.rs`, `UdpTrackerClient::new`
falls back to an IPv4 socket when an IPv6 socket can't be created, so torrents work on computers
with IPv6 turned off. Nothing else is changed. Remove this folder and the `[patch.crates-io]`
entry in the workspace `Cargo.toml` once upstream has the same fallback.
