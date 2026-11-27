//! Adding the browser extension from Settings.
//!
//! The installer puts the extension in the app's folder: `extensions/chromium` (unpacked,
//! for Chrome, Edge, Brave and Opera) and `extensions/firefox/layos-download-manager.xpi`.
//!
//! Chromium browsers on Windows only install packed extensions (.crx) that come from
//! their web stores; one from anywhere else is refused. What they do allow is an
//! unpacked extension in Developer mode, and that is what "Add to Chrome" sets up: it
//! checks Developer mode (read from the browser's own settings file, never written),
//! opens the browser's Extensions page and shows the extension's folder in File
//! Explorer, ready to drop onto the page. Firefox opens the .xpi itself and asks.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The extension's id in Chromium browsers (fixed by the "key" in its manifest).
pub const CHROMIUM_ID: &str = "klknhohjiaanbbicfdonmcnflkgjofai";
/// The extension's id in Firefox.
pub const FIREFOX_ID: &str = "downloadmanager@layo.dev";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Browser {
    Chrome,
    Edge,
    Brave,
    Opera,
    Firefox,
}

impl Browser {
    pub const ALL: [Browser; 5] = [
        Browser::Chrome,
        Browser::Edge,
        Browser::Brave,
        Browser::Opera,
        Browser::Firefox,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "Chrome",
            Browser::Edge => "Edge",
            Browser::Brave => "Brave",
            Browser::Opera => "Opera",
            Browser::Firefox => "Firefox",
        }
    }

    pub fn is_chromium(self) -> bool {
        self != Browser::Firefox
    }

    /// The page that lists extensions, typed in the address bar.
    pub fn extensions_page(self) -> &'static str {
        match self {
            Browser::Chrome => "chrome://extensions",
            Browser::Edge => "edge://extensions",
            Browser::Brave => "brave://extensions",
            Browser::Opera => "opera://extensions",
            Browser::Firefox => "about:addons",
        }
    }

    /// The program's name under "App Paths" in the registry.
    fn exe_name(self) -> &'static str {
        match self {
            Browser::Chrome => "chrome.exe",
            Browser::Edge => "msedge.exe",
            Browser::Brave => "brave.exe",
            Browser::Opera => "opera.exe",
            Browser::Firefox => "firefox.exe",
        }
    }

    /// Where the program usually is, relative to Program Files (both) and Local AppData.
    fn usual_places(self) -> &'static [&'static str] {
        match self {
            Browser::Chrome => &[r"Google\Chrome\Application\chrome.exe"],
            Browser::Edge => &[r"Microsoft\Edge\Application\msedge.exe"],
            Browser::Brave => &[r"BraveSoftware\Brave-Browser\Application\brave.exe"],
            Browser::Opera => &[r"Programs\Opera\launcher.exe", r"Opera\launcher.exe"],
            Browser::Firefox => &[r"Mozilla Firefox\firefox.exe"],
        }
    }

    /// The folder with the browser's profiles ("User Data").
    fn user_data(self, env: &Env) -> Option<PathBuf> {
        Some(match self {
            Browser::Chrome => env.local.as_ref()?.join(r"Google\Chrome\User Data"),
            Browser::Edge => env.local.as_ref()?.join(r"Microsoft\Edge\User Data"),
            Browser::Brave => env
                .local
                .as_ref()?
                .join(r"BraveSoftware\Brave-Browser\User Data"),
            Browser::Opera => env.roaming.as_ref()?.join(r"Opera Software\Opera Stable"),
            Browser::Firefox => env.roaming.as_ref()?.join(r"Mozilla\Firefox\Profiles"),
        })
    }
}

/// The folders the lookups start from (from the environment; tests pass their own).
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub local: Option<PathBuf>,
    pub roaming: Option<PathBuf>,
    pub program_files: Vec<PathBuf>,
}

impl Env {
    pub fn current() -> Self {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self {
            local: var("LOCALAPPDATA"),
            roaming: var("APPDATA"),
            program_files: ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
                .iter()
                .filter_map(|k| var(k))
                .collect(),
        }
    }
}

/// What Settings shows for one browser.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub browser: Browser,
    pub name: &'static str,
    /// The browser is on this computer.
    pub found: bool,
    /// The extension is in one of the browser's profiles.
    pub added: bool,
    /// Developer mode, in the profile used last (Chromium browsers; `None` = unknown).
    pub developer_mode: Option<bool>,
}

/// What clicking a browser did, for the steps Settings shows next.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub browser: Browser,
    pub name: &'static str,
    /// "already", "devMode" (Developer mode is off), "drop" (drop the folder onto the
    /// page), or "firefox" (Firefox is asking).
    pub step: &'static str,
    /// The folder (Chromium) or file (Firefox) of the extension.
    pub path: String,
    pub page: &'static str,
}

pub fn status(env: &Env) -> Vec<BrowserStatus> {
    Browser::ALL
        .iter()
        .map(|&b| BrowserStatus {
            browser: b,
            name: b.name(),
            found: find_exe(b, env).is_some(),
            added: is_added(b, env),
            developer_mode: if b.is_chromium() {
                developer_mode(b, env)
            } else {
                None
            },
        })
        .collect()
}

/// The extension that came with the app.
pub fn packaged(resources: &Path, browser: Browser) -> PathBuf {
    if browser.is_chromium() {
        resources.join("extensions").join("chromium")
    } else {
        resources
            .join("extensions")
            .join("firefox")
            .join("layos-download-manager.xpi")
    }
}

/// Opens the browser to add the extension. Errors are shown as they are.
pub fn add(browser: Browser, resources: &Path, env: &Env) -> Result<AddResult, String> {
    let name = browser.name();
    let path = packaged(resources, browser);
    if !path.exists() {
        return Err(format!(
            "The extension isn't in the app's folder ({}). Reinstall Layó's Download Manager.",
            path.display()
        ));
    }
    let exe = find_exe(browser, env)
        .ok_or_else(|| format!("{name} isn't installed on this computer."))?;
    let result = |step| AddResult {
        browser,
        name,
        step,
        path: path.display().to_string(),
        page: browser.extensions_page(),
    };

    if !browser.is_chromium() {
        // Firefox treats the .xpi as an add-on to install and asks the person.
        launch(&exe, &[path.as_os_str()])?;
        return Ok(result("firefox"));
    }

    let added = is_added(browser, env);
    launch(&exe, &[std::ffi::OsStr::new(browser.extensions_page())])?;
    if added {
        return Ok(result("already"));
    }
    if developer_mode(browser, env) == Some(false) {
        return Ok(result("devMode"));
    }
    reveal(&path);
    Ok(result("drop"))
}

fn launch(exe: &Path, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    std::process::Command::new(exe)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start {}: {e}", exe.display()))
}

/// Shows the folder in File Explorer, selected, so it can be dragged.
fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut arg = std::ffi::OsString::from("/select,\"");
        arg.push(path.as_os_str());
        arg.push("\"");
        let _ = std::process::Command::new("explorer.exe")
            .raw_arg(arg)
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = path;
}

pub fn find_exe(browser: Browser, env: &Env) -> Option<PathBuf> {
    if let Some(p) = registered_exe(browser).filter(|p| p.is_file()) {
        return Some(p);
    }
    let mut roots: Vec<&PathBuf> = env.program_files.iter().collect();
    roots.extend(env.local.iter());
    for root in roots {
        for rel in browser.usual_places() {
            let p = root.join(rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// "App Paths", where Windows itself looks up programs by name.
#[cfg(windows)]
fn registered_exe(browser: Browser) -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let key = format!(
        r"Software\Microsoft\Windows\CurrentVersion\App Paths\{}",
        browser.exe_name()
    );
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Ok(k) = RegKey::predef(hive).open_subkey(&key)
            && let Ok(v) = k.get_value::<String, _>("")
        {
            let v = v.trim().trim_matches('"');
            if !v.is_empty() {
                return Some(PathBuf::from(v));
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn registered_exe(browser: Browser) -> Option<PathBuf> {
    let _ = browser.exe_name();
    None
}

/// The profile folders of a browser: Default, Profile 1… (Opera keeps its first profile
/// in the top folder itself).
fn profiles(browser: Browser, env: &Env) -> Vec<PathBuf> {
    let Some(root) = browser.user_data(env) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if browser.is_chromium() && root.join("Preferences").is_file() {
        out.push(root.clone());
    }
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let p = e.path();
            let marker = if browser.is_chromium() {
                "Preferences"
            } else {
                "prefs.js"
            };
            if p.join(marker).is_file() {
                out.push(p);
            }
        }
    }
    out
}

/// Whether the extension is in any profile of the browser.
pub fn is_added(browser: Browser, env: &Env) -> bool {
    profiles(browser, env).iter().any(|p| {
        if browser.is_chromium() {
            ["Secure Preferences", "Preferences"]
                .iter()
                .any(|f| chromium_has_extension(&p.join(f)))
        } else {
            read_json(&p.join("extensions.json"))
                .and_then(|j| j.get("addons").and_then(|a| a.as_array()).cloned())
                .is_some_and(|addons| {
                    addons
                        .iter()
                        .any(|a| a.get("id").and_then(|i| i.as_str()) == Some(FIREFOX_ID))
                })
        }
    })
}

fn chromium_has_extension(prefs: &Path) -> bool {
    let Some(json) = read_json(prefs) else {
        return false;
    };
    json.pointer(&format!("/extensions/settings/{CHROMIUM_ID}"))
        .is_some()
}

/// Developer mode on the Extensions page of the profile used last.
pub fn developer_mode(browser: Browser, env: &Env) -> Option<bool> {
    let root = browser.user_data(env)?;
    let profile = read_json(&root.join("Local State"))
        .and_then(|j| {
            j.pointer("/profile/last_used")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .filter(|s| !s.is_empty() && !s.contains(['/', '\\', '.']))
        .map(|name| root.join(name))
        .filter(|p| p.join("Preferences").is_file())
        .or_else(|| {
            // Opera: the top folder; others: "Default".
            [root.clone(), root.join("Default")]
                .into_iter()
                .find(|p| p.join("Preferences").is_file())
        })?;
    let prefs = read_json(&profile.join("Preferences"))?;
    Some(
        prefs
            .pointer("/extensions/ui/developer_mode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    )
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    fn env(dir: &Path) -> Env {
        Env {
            local: Some(dir.join("local")),
            roaming: Some(dir.join("roaming")),
            program_files: vec![dir.join("pf")],
        }
    }

    #[test]
    fn finds_browsers_in_the_usual_places() {
        let t = tempfile::tempdir().unwrap();
        let e = env(t.path());
        assert_eq!(find_exe(Browser::Chrome, &e), None);
        // The usual places are written with backslashes, as on Windows.
        if cfg!(windows) {
            write(
                &t.path().join(r"pf\Google\Chrome\Application\chrome.exe"),
                "",
            );
            assert!(find_exe(Browser::Chrome, &e).is_some());
            write(&t.path().join(r"local\Programs\Opera\launcher.exe"), "");
            assert!(find_exe(Browser::Opera, &e).is_some());
        }
    }

    #[test]
    fn reads_developer_mode_and_the_extension_from_the_profile() {
        let t = tempfile::tempdir().unwrap();
        let e = env(t.path());
        let data = Browser::Edge.user_data(&e).unwrap();
        assert_eq!(developer_mode(Browser::Edge, &e), None);
        assert!(!is_added(Browser::Edge, &e));

        write(
            &data.join("Local State"),
            r#"{"profile":{"last_used":"Profile 1"}}"#,
        );
        write(
            &data.join("Default").join("Preferences"),
            r#"{"extensions":{"ui":{"developer_mode":true}}}"#,
        );
        write(
            &data.join("Profile 1").join("Preferences"),
            r#"{"extensions":{}}"#,
        );
        assert_eq!(
            developer_mode(Browser::Edge, &e),
            Some(false),
            "the profile used last decides"
        );

        write(
            &data.join("Profile 1").join("Preferences"),
            r#"{"extensions":{"ui":{"developer_mode":true}}}"#,
        );
        assert_eq!(developer_mode(Browser::Edge, &e), Some(true));

        write(
            &data.join("Default").join("Secure Preferences"),
            &format!(r#"{{"extensions":{{"settings":{{"{CHROMIUM_ID}":{{"path":"x"}}}}}}}}"#),
        );
        assert!(is_added(Browser::Edge, &e));
        assert!(!is_added(Browser::Chrome, &e));
    }

    #[test]
    fn opera_keeps_its_profile_in_the_top_folder() {
        let t = tempfile::tempdir().unwrap();
        let e = env(t.path());
        let data = Browser::Opera.user_data(&e).unwrap();
        write(
            &data.join("Preferences"),
            r#"{"extensions":{"ui":{"developer_mode":false}}}"#,
        );
        assert_eq!(developer_mode(Browser::Opera, &e), Some(false));
        write(
            &data.join("Preferences"),
            &format!(
                r#"{{"extensions":{{"ui":{{"developer_mode":true}},"settings":{{"{CHROMIUM_ID}":{{}}}}}}}}"#
            ),
        );
        assert_eq!(developer_mode(Browser::Opera, &e), Some(true));
        assert!(is_added(Browser::Opera, &e));
    }

    #[test]
    fn firefox_lists_its_add_ons_in_extensions_json() {
        let t = tempfile::tempdir().unwrap();
        let e = env(t.path());
        let data = Browser::Firefox.user_data(&e).unwrap();
        let profile = data.join("abc.default-release");
        write(&profile.join("prefs.js"), "");
        write(
            &profile.join("extensions.json"),
            r#"{"addons":[{"id":"other@x"}]}"#,
        );
        assert!(!is_added(Browser::Firefox, &e));
        write(
            &profile.join("extensions.json"),
            &format!(r#"{{"addons":[{{"id":"{FIREFOX_ID}"}}]}}"#),
        );
        assert!(is_added(Browser::Firefox, &e));
    }

    #[test]
    fn the_ids_match_the_bridge() {
        let bridge = include_str!("../../../crates/native-host/src/register.rs");
        assert!(bridge.contains(&format!("\"{CHROMIUM_ID}\"")));
        assert!(bridge.contains(&format!("\"{FIREFOX_ID}\"")));
    }

    #[test]
    fn adding_without_the_packaged_extension_says_so() {
        let t = tempfile::tempdir().unwrap();
        let err = add(Browser::Chrome, t.path(), &env(t.path())).unwrap_err();
        assert!(err.contains("Reinstall"), "{err}");
    }
}
