//! Tells browsers where the bridge is (run by the installer).
//!
//! Each browser reads a small JSON "host manifest" naming this program and which
//! extensions may talk to it. On Windows the manifest's location is stored in the
//! registry under HKEY_CURRENT_USER, so no admin rights are needed.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::HOST_NAME;

/// Chrome, Edge, Opera, Brave and Vivaldi extension ids allowed to use the bridge.
/// The development id comes from the `key` in the extension's manifest. Add the store
/// ids here once the extension is published.
pub const CHROMIUM_EXTENSION_IDS: &[&str] = &["klknhohjiaanbbicfdonmcnflkgjofai"];
/// Firefox extension id (`browser_specific_settings.gecko.id`).
pub const FIREFOX_EXTENSION_ID: &str = "downloadmanager@layo.dev";

#[cfg(windows)]
const CHROMIUM_REGISTRY_KEYS: &[&str] = &[
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Chromium\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
    r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
    r"Software\Vivaldi\NativeMessagingHosts",
];
#[cfg(windows)]
const FIREFOX_REGISTRY_KEY: &str = r"Software\Mozilla\NativeMessagingHosts";

#[derive(Default)]
struct Options {
    extension_ids: Vec<String>,
    /// Also register in this browser profile folder (`<dir>/NativeMessagingHosts`), for tests.
    user_data_dir: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--extension-id" => {
                let id = it.next().ok_or("--extension-id needs a value")?;
                if id.len() != 32 || !id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
                    return Err(format!("{id} is not a Chrome extension id"));
                }
                o.extension_ids.push(id.clone());
            }
            "--user-data-dir" => {
                o.user_data_dir = Some(it.next().ok_or("--user-data-dir needs a value")?.into())
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

pub fn chromium_manifest(host_path: &Path, extra_ids: &[String]) -> serde_json::Value {
    let origins: Vec<String> = CHROMIUM_EXTENSION_IDS
        .iter()
        .map(|s| s.to_string())
        .chain(extra_ids.iter().cloned())
        .map(|id| format!("chrome-extension://{id}/"))
        .collect();
    json!({
        "name": HOST_NAME,
        "description": "Layó's Download Manager browser bridge",
        "path": host_path,
        "type": "stdio",
        "allowed_origins": origins,
    })
}

pub fn firefox_manifest(host_path: &Path) -> serde_json::Value {
    json!({
        "name": HOST_NAME,
        "description": "Layó's Download Manager browser bridge",
        "path": host_path,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_EXTENSION_ID],
    })
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn register(args: &[String]) -> Result<String, String> {
    let opts = parse(args)?;
    let host = std::env::current_exe().map_err(|e| e.to_string())?;
    let chromium = chromium_manifest(&host, &opts.extension_ids);
    let firefox = firefox_manifest(&host);
    let mut done = Vec::new();

    // Manifests live next to the program; the registry (Windows) or browser folders point at them.
    let dir = host
        .parent()
        .ok_or("no install folder")?
        .join("native-messaging");
    let chromium_file = dir.join(format!("{HOST_NAME}.chromium.json"));
    let firefox_file = dir.join(format!("{HOST_NAME}.firefox.json"));
    write_json(&chromium_file, &chromium)?;
    write_json(&firefox_file, &firefox)?;

    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for key in CHROMIUM_REGISTRY_KEYS {
            let (k, _) = hkcu
                .create_subkey(format!(r"{key}\{HOST_NAME}"))
                .map_err(|e| format!("{key}: {e}"))?;
            k.set_value("", &chromium_file.as_os_str())
                .map_err(|e| format!("{key}: {e}"))?;
            done.push(format!(r"HKCU\{key}\{HOST_NAME}"));
        }
        let (k, _) = hkcu
            .create_subkey(format!(r"{FIREFOX_REGISTRY_KEY}\{HOST_NAME}"))
            .map_err(|e| format!("{FIREFOX_REGISTRY_KEY}: {e}"))?;
        k.set_value("", &firefox_file.as_os_str())
            .map_err(|e| e.to_string())?;
        done.push(format!(r"HKCU\{FIREFOX_REGISTRY_KEY}\{HOST_NAME}"));
    }

    #[cfg(not(windows))]
    for (path, is_firefox) in unix_locations() {
        write_json(&path, if is_firefox { &firefox } else { &chromium })?;
        done.push(path.display().to_string());
    }

    if let Some(profile) = &opts.user_data_dir {
        let p = profile
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json"));
        write_json(&p, &chromium)?;
        done.push(p.display().to_string());
    }
    Ok(format!(
        "Registered {HOST_NAME} for:\n  {}",
        done.join("\n  ")
    ))
}

pub fn unregister(args: &[String]) -> Result<String, String> {
    let opts = parse(args)?;
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for key in CHROMIUM_REGISTRY_KEYS.iter().chain([&FIREFOX_REGISTRY_KEY]) {
            let _ = hkcu.delete_subkey_all(format!(r"{key}\{HOST_NAME}"));
        }
    }
    #[cfg(not(windows))]
    for (path, _) in unix_locations() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(profile) = &opts.user_data_dir {
        let _ = std::fs::remove_file(
            profile
                .join("NativeMessagingHosts")
                .join(format!("{HOST_NAME}.json")),
        );
    }
    if let Ok(host) = std::env::current_exe()
        && let Some(dir) = host.parent()
    {
        let _ = std::fs::remove_dir_all(dir.join("native-messaging"));
    }
    Ok(format!("Unregistered {HOST_NAME}"))
}

/// Per-user host folders on Linux, for development. `true` marks Firefox's.
#[cfg(not(windows))]
fn unix_locations() -> Vec<(PathBuf, bool)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let file = format!("{HOST_NAME}.json");
    [
        ".config/google-chrome/NativeMessagingHosts",
        ".config/chromium/NativeMessagingHosts",
        ".config/microsoft-edge/NativeMessagingHosts",
        ".config/BraveSoftware/Brave-Browser/NativeMessagingHosts",
        ".config/vivaldi/NativeMessagingHosts",
        ".mozilla/native-messaging-hosts",
    ]
    .iter()
    .map(|d| (home.join(d).join(&file), d.contains("mozilla")))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_name_the_extensions() {
        let m = chromium_manifest(
            Path::new("/x/host"),
            &["abcdefghijklmnopabcdefghijklmnop".into()],
        );
        let origins = m["allowed_origins"].as_array().unwrap();
        assert!(origins.contains(&json!(
            "chrome-extension://klknhohjiaanbbicfdonmcnflkgjofai/"
        )));
        assert!(origins.contains(&json!(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        )));
        assert_eq!(m["name"], HOST_NAME);
        assert_eq!(
            firefox_manifest(Path::new("/x/host"))["allowed_extensions"][0],
            FIREFOX_EXTENSION_ID
        );
    }

    #[test]
    fn rejects_bad_ids() {
        assert!(parse(&["--extension-id".into(), "nope".into()]).is_err());
        assert!(parse(&["--bogus".into()]).is_err());
    }
}
