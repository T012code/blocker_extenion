//! How the browser bridge (`ldm-native-host`) talks to the running app.
//!
//! Messages are JSON objects, one per line, over a named pipe on Windows
//! (`\\.\pipe\layos-download-manager-<user>`) or a Unix socket elsewhere. The pipe only
//! accepts connections from this computer.

mod protocol;
mod transport;

pub use protocol::*;
pub use transport::*;
