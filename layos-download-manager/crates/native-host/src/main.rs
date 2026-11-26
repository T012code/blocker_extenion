//! Browser bridge for Layó's Download Manager.
//!
//! Browsers start this program when the extension sends a message, and talk to it over
//! stdin/stdout using Native Messaging: each message is a 4-byte little-endian length
//! followed by that many bytes of JSON. It passes messages on to the app over the local
//! pipe, and starts the app first if it isn't running.
//!
//! Also used by the installer:
//!   ldm-native-host --register [--extension-id ID]... [--user-data-dir DIR]
//!   ldm-native-host --unregister

mod register;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use ldm_ipc::{IpcError, Request, Response};

pub const HOST_NAME: &str = "com.layo.downloadmanager";
const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Browsers refuse messages from a host bigger than 1 MB.
const MAX_REPLY: usize = 1024 * 1024;
/// Messages from the browser are at most 64 MB.
const MAX_REQUEST: usize = 64 * 1024 * 1024;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--register") => exit_with(register::register(&args[1..])),
        Some("--unregister") => exit_with(register::unregister(&args[1..])),
        Some("--version") => println!("{HOST_VERSION}"),
        // Started by a browser. Chrome passes the caller's origin, Firefox the manifest
        // path and extension id; both are already checked by the browser.
        _ => run_host(),
    }
}

fn exit_with(result: Result<String, String>) {
    match result {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn run_host() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    // One host process can get many messages (connectNative) or just one (sendNativeMessage).
    while let Some(raw) = read_native(&mut stdin) {
        let reply = match serde_json::from_slice::<Request>(&raw) {
            Ok(req) => runtime.block_on(forward(req)),
            Err(e) => Response::error(format!("bad message: {e}")),
        };
        if write_native(&mut stdout, &reply).is_err() {
            break;
        }
    }
}

fn read_native(r: &mut impl Read) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).ok()?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_REQUEST {
        return None;
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn write_native(w: &mut impl Write, msg: &Response) -> std::io::Result<()> {
    let mut body = serde_json::to_vec(msg)?;
    if body.len() > MAX_REPLY {
        body = serde_json::to_vec(&Response::error("reply too large"))?;
    }
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

async fn forward(req: Request) -> Response {
    let path = ldm_ipc::default_path();
    let timeout = Duration::from_secs(30);
    match ldm_ipc::request(&path, &req, timeout).await {
        Ok(Response::Pong {
            app_running,
            app_version,
            ..
        }) => Response::Pong {
            app_running,
            app_version,
            host_version: Some(HOST_VERSION.into()),
        },
        Ok(resp) => resp,
        Err(IpcError::NotRunning) if !req.needs_app() => Response::Pong {
            app_running: false,
            app_version: None,
            host_version: Some(HOST_VERSION.into()),
        },
        Err(IpcError::NotRunning) => {
            if let Err(e) = launch_app() {
                return Response::error(format!("could not start Layó's Download Manager: {e}"));
            }
            // Wait for it to start listening.
            for _ in 0..150 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if ldm_ipc::is_running(&path).await {
                    return ldm_ipc::request(&path, &req, timeout)
                        .await
                        .unwrap_or_else(|e| Response::error(e.to_string()));
                }
            }
            Response::error("Layó's Download Manager did not start in time")
        }
        Err(e) => Response::error(e.to_string()),
    }
}

/// The app is installed next to this program. `LDM_APP_PATH` overrides it (tests).
fn app_path() -> std::io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("LDM_APP_PATH").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("no install folder"))?;
    let name = if cfg!(windows) {
        "LayosDownloadManager.exe"
    } else {
        "layos-download-manager"
    };
    Ok(dir.join(name))
}

fn launch_app() -> std::io::Result<()> {
    let path = app_path()?;
    let mut cmd = std::process::Command::new(&path);
    cmd.arg("--from-browser")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        // Browsers run hosts in a job that is closed with them; break away so the app keeps running.
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        match cmd.spawn() {
            Ok(_) => return Ok(()),
            Err(_) => {
                cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so it outlives the browser's host.
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_framing_round_trip() {
        let mut out = Vec::new();
        write_native(&mut out, &Response::Ok).unwrap();
        assert_eq!(&out[..4], &(out.len() as u32 - 4).to_le_bytes());
        let mut cursor = std::io::Cursor::new(out);
        let body = read_native(&mut cursor).unwrap();
        assert_eq!(
            serde_json::from_slice::<Response>(&body).unwrap(),
            Response::Ok
        );
        assert!(read_native(&mut cursor).is_none());
    }
}
