//! Making Windows open magnet links and .torrent files with Layó's Download Manager.
//! Everything is written under HKEY_CURRENT_USER, so no admin rights are needed, and
//! only entries pointing at this app are ever removed.

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    const PROG_ID: &str = "LayosDownloadManager.torrent";

    fn command(exe: &Path) -> String {
        format!("\"{}\" \"%1\"", exe.display())
    }

    pub fn register(exe: &Path) -> std::io::Result<()> {
        let classes = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(r"Software\Classes")?
            .0;
        let icon = format!("\"{}\",0", exe.display());

        let (magnet, _) = classes.create_subkey("magnet")?;
        magnet.set_value("", &"URL:Magnet link")?;
        magnet.set_value("URL Protocol", &"")?;
        magnet
            .create_subkey("DefaultIcon")?
            .0
            .set_value("", &icon)?;
        magnet
            .create_subkey(r"shell\open\command")?
            .0
            .set_value("", &command(exe))?;

        let (prog, _) = classes.create_subkey(PROG_ID)?;
        prog.set_value("", &"Torrent file")?;
        prog.create_subkey("DefaultIcon")?.0.set_value("", &icon)?;
        prog.create_subkey(r"shell\open\command")?
            .0
            .set_value("", &command(exe))?;
        let (ext, _) = classes.create_subkey(".torrent")?;
        ext.set_value("", &PROG_ID)?;
        ext.set_value("Content Type", &"application/x-bittorrent")?;
        ext.create_subkey("OpenWithProgids")?
            .0
            .set_value(PROG_ID, &"")?;
        Ok(())
    }

    /// Removes our entries, leaving other apps' alone.
    pub fn unregister(exe: &Path) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(classes) =
            hkcu.open_subkey_with_flags(r"Software\Classes", winreg::enums::KEY_ALL_ACCESS)
        else {
            return Ok(());
        };
        let ours = |key: &str| {
            classes
                .open_subkey(format!(r"{key}\shell\open\command"))
                .and_then(|k| k.get_value::<String, _>(""))
                .is_ok_and(|v| {
                    v.to_lowercase()
                        .contains(&exe.display().to_string().to_lowercase())
                })
        };
        if ours("magnet") {
            let _ = classes.delete_subkey_all("magnet");
        }
        if let Ok(ext) = classes.open_subkey_with_flags(".torrent", winreg::enums::KEY_ALL_ACCESS) {
            if ext.get_value::<String, _>("").is_ok_and(|v| v == PROG_ID) {
                let _ = ext.delete_value("");
            }
            if let Ok(owp) =
                ext.open_subkey_with_flags("OpenWithProgids", winreg::enums::KEY_ALL_ACCESS)
            {
                let _ = owp.delete_value(PROG_ID);
            }
        }
        let _ = classes.delete_subkey_all(PROG_ID);
        Ok(())
    }

    /// Removes the "start with Windows" entry (used by the uninstaller).
    pub fn remove_startup_entry() {
        if let Ok(run) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            winreg::enums::KEY_ALL_ACCESS,
        ) {
            // The product name has a typographic apostrophe (NSIS reads ' as a quote).
            for name in [
                "Layó’s Download Manager",
                "Layó's Download Manager",
                "LayosDownloadManager",
            ] {
                let _ = run.delete_value(name);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;
    pub fn register(_exe: &Path) -> std::io::Result<()> {
        Ok(())
    }
    pub fn unregister(_exe: &Path) -> std::io::Result<()> {
        Ok(())
    }
    pub fn remove_startup_entry() {}
}

pub use imp::{register, remove_startup_entry, unregister};

/// Applies the setting: register or remove the handlers for this app.
pub fn apply(on: bool) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let result = if on { register(&exe) } else { unregister(&exe) };
    if let Err(e) = result {
        tracing::warn!("couldn't update magnet / .torrent handlers: {e}");
    }
}
