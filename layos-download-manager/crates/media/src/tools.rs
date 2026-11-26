//! Finding and running yt-dlp and ffmpeg.

use std::path::{Path, PathBuf};

use ldm_engine::{Error, RequestInfo, Result};

/// Where yt-dlp and ffmpeg are.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub ytdlp: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(exe(name)))
        .find(|p| p.is_file())
}

impl Tools {
    /// Looks in this order: `LDM_YTDLP` / `LDM_FFMPEG`, the `tools` folder next to the
    /// app (where the installer puts them), then the PATH.
    pub fn find(app_dir: Option<&Path>) -> Self {
        let pick = |env: &str, name: &str| {
            std::env::var_os(env)
                .map(PathBuf::from)
                .filter(|p| p.is_file())
                .or_else(|| {
                    app_dir
                        .map(|d| d.join("tools").join(exe(name)))
                        .filter(|p| p.is_file())
                })
                .or_else(|| on_path(name))
        };
        Self {
            ytdlp: pick("LDM_YTDLP", "yt-dlp"),
            ffmpeg: pick("LDM_FFMPEG", "ffmpeg"),
        }
    }

    pub fn ytdlp(&self) -> Result<&Path> {
        self.ytdlp.as_deref().ok_or_else(|| {
            Error::Other("yt-dlp is missing. Reinstall Layó's Download Manager to restore the video downloader.".into())
        })
    }
}

pub(crate) fn command(program: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    {
        // No console window flashing up.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// A temporary cookies.txt for yt-dlp, deleted when dropped.
pub(crate) struct CookieFile(PathBuf);

impl CookieFile {
    /// Writes the browser's `Cookie` header as a Netscape cookie file for the URL's site.
    pub fn write(req: &RequestInfo) -> Result<Option<Self>> {
        let Some(header) = req.cookies.as_deref().filter(|c| !c.trim().is_empty()) else {
            return Ok(None);
        };
        let host = req
            .url
            .split("://")
            .nth(1)
            .and_then(|r| r.split(['/', '?', '#']).next())
            .map(|h| h.rsplit_once('@').map_or(h, |(_, h)| h))
            .map(|h| h.split(':').next().unwrap_or(h))
            .unwrap_or_default();
        let secure = req.url.starts_with("https://");
        let mut text = String::from("# Netscape HTTP Cookie File\n");
        for pair in header.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            // domain, include subdomains, path, secure, expires, name, value
            text.push_str(&format!(
                ".{host}\tTRUE\t/\t{}\t2147483647\t{}\t{}\n",
                if secure { "TRUE" } else { "FALSE" },
                name.trim(),
                value.trim()
            ));
        }
        let path = std::env::temp_dir().join(format!(
            "ldm-cookies-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, text)?;
        Ok(Some(Self(path)))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Passes on the browser's referrer, user agent and cookies.
pub(crate) fn add_request_args(
    cmd: &mut tokio::process::Command,
    req: &RequestInfo,
    cookies: Option<&CookieFile>,
) {
    if let Some(r) = req.referer.as_deref().filter(|r| !r.is_empty()) {
        cmd.arg("--referer").arg(r);
    }
    if let Some(ua) = req.user_agent.as_deref().filter(|u| !u.is_empty()) {
        cmd.arg("--user-agent").arg(ua);
    }
    if let Some(c) = cookies {
        cmd.arg("--cookies").arg(c.path());
    }
    for (k, v) in &req.headers {
        cmd.arg("--add-header").arg(format!("{k}:{v}"));
    }
}

/// The most useful line of yt-dlp's error output.
pub(crate) fn last_error(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or("the video downloader stopped with an error");
    let msg = line.trim_start_matches("ERROR:").trim();
    if msg.contains("Unsupported URL") {
        "No downloadable video was found at this address.".to_string()
    } else if msg.to_lowercase().contains("drm") {
        "This video is protected (DRM) and can't be downloaded.".to_string()
    } else {
        msg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_file_for_the_site() {
        let req = RequestInfo {
            url: "https://user@www.example.com:8443/watch?v=1".into(),
            cookies: Some("a=1; session=xyz=; bad".into()),
            ..Default::default()
        };
        let f = CookieFile::write(&req).unwrap().unwrap();
        let text = std::fs::read_to_string(f.path()).unwrap();
        assert!(text.contains(".www.example.com\tTRUE\t/\tTRUE\t2147483647\ta\t1\n"));
        assert!(text.contains("\tsession\txyz=\n"));
        assert!(!text.contains("bad"));
        let p = f.path().to_path_buf();
        drop(f);
        assert!(!p.exists());
    }

    #[test]
    fn friendly_errors() {
        assert_eq!(
            last_error("WARNING: x\nERROR: Unsupported URL: https://a"),
            "No downloadable video was found at this address."
        );
        assert!(last_error("ERROR: [x] This video is DRM protected").contains("protected"));
    }
}
