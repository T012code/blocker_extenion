//! Finding and running yt-dlp, ffmpeg and Deno.

use std::path::{Path, PathBuf};

use ldm_engine::{Error, RequestInfo, Result};

/// Where yt-dlp, ffmpeg and Deno are.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub ytdlp: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
    /// A JavaScript runtime. yt-dlp needs one for YouTube: without it most formats
    /// (often all of them) are missing.
    pub deno: Option<PathBuf>,
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
    /// Looks in this order: `LDM_YTDLP` / `LDM_FFMPEG` / `LDM_DENO`, the `tools` folder next to the
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
            deno: pick("LDM_DENO", "deno"),
        }
    }

    pub fn ytdlp(&self) -> Result<&Path> {
        self.ytdlp.as_deref().ok_or_else(|| {
            Error::Other("yt-dlp is missing. Reinstall Layó's Download Manager to restore the video downloader.".into())
        })
    }
}

/// A yt-dlp command, with the JavaScript runtime it needs for YouTube.
pub(crate) fn ytdlp_command(tools: &Tools) -> Result<tokio::process::Command> {
    let mut cmd = command(tools.ytdlp()?);
    // Output in UTF-8, not Windows' old code page, so titles and names come through whole.
    cmd.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
    if let Some(deno) = &tools.deno {
        let mut arg = std::ffi::OsString::from("deno:");
        arg.push(deno.as_os_str());
        cmd.arg("--js-runtimes").arg(arg);
    }
    Ok(cmd)
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

/// Sites where yt-dlp speaks as one of the site's own apps: a browser's user agent
/// would contradict that and makes the site refuse the formats.
fn has_own_clients(url: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .and_then(|r| r.split(['/', '?', '#', ':']).next())
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["youtube.com", "youtu.be", "youtube-nocookie.com"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// Whether the browser's details (cookies, user agent, referrer, headers) are sent.
pub(crate) fn sends_browser_details(req: &RequestInfo) -> bool {
    req.cookies.as_deref().is_some_and(|c| !c.trim().is_empty())
        || req.user_agent.as_deref().is_some_and(|u| !u.is_empty())
        || req.referer.as_deref().is_some_and(|r| !r.is_empty())
        || !req.headers.is_empty()
}

/// Passes on the browser's referrer, user agent and cookies (not the user agent where
/// yt-dlp uses the site's own apps).
pub(crate) fn add_request_args(
    cmd: &mut tokio::process::Command,
    req: &RequestInfo,
    cookies: Option<&CookieFile>,
) {
    if let Some(r) = req.referer.as_deref().filter(|r| !r.is_empty()) {
        cmd.arg("--referer").arg(r);
    }
    if let Some(ua) = req.user_agent.as_deref().filter(|u| !u.is_empty())
        && !has_own_clients(&req.url)
    {
        cmd.arg("--user-agent").arg(ua);
    }
    if let Some(c) = cookies {
        cmd.arg("--cookies").arg(c.path());
    }
    for (k, v) in &req.headers {
        cmd.arg("--add-header").arg(format!("{k}:{v}"));
    }
}

/// Plain words for the usual ways a site turns the request down.
fn site_refused(msg: &str) -> Option<&'static str> {
    let m = msg.to_ascii_lowercase();
    if m.contains("429")
        || m.contains("too many requests")
        || m.contains("rate-limit")
        || m.contains("rate limit")
    {
        Some("The site is limiting requests right now. Wait a minute, then continue the download.")
    } else if m.contains("not a bot") {
        Some(
            "The site wants to check you're not a bot. Open the video in your browser (signed in), then try again.",
        )
    } else if m.contains("ip address is blocked")
        || m.contains("not available in your country")
        || m.contains("geo restrict")
    {
        Some("The site doesn't offer this video where you are.")
    } else if m.contains("private video") || m.contains("this video is private") {
        Some("This video is private.")
    } else if m.contains("--cookies")
        || m.contains("login required")
        || m.contains("logged-in")
        || m.contains("log in")
        || m.contains("sign in")
        || m.contains("authentication")
    {
        Some(
            "The site wants you signed in for this video. Sign in to it in your browser, then try again from the page.",
        )
    } else if m.contains("no video") && m.contains("found") {
        Some("No video was found in this post.")
    } else {
        None
    }
}

/// yt-dlp found no format it could use. Often caused by the browser's cookies or user
/// agent (they make the site answer differently), so it's worth one more try without.
pub(crate) fn is_format_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("requested format is not available")
        || m.contains("no video formats found")
        || m.contains("only images are available")
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
    } else if let Some(friendly) = site_refused(msg) {
        friendly.to_string()
    } else if is_format_error(msg) {
        "The site offered no format that can be downloaded. Try again in a moment; if it keeps happening, the video downloader may need an update.".to_string()
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

#[cfg(test)]
mod request_tests {
    use super::*;

    #[test]
    fn youtube_keeps_its_own_user_agent() {
        assert!(has_own_clients("https://www.youtube.com/watch?v=x"));
        assert!(has_own_clients("https://youtu.be/x"));
        assert!(has_own_clients("https://m.youtube.com/watch?v=x"));
        assert!(!has_own_clients("https://notyoutube.com/x"));
        assert!(!has_own_clients("https://vimeo.com/1"));
    }

    #[test]
    fn site_refusals_in_plain_words() {
        for (raw, want) in [
            (
                "[Instagram] x: Unable to download webpage: HTTP Error 429: Too Many Requests",
                "limiting requests",
            ),
            (
                "[youtube] x: Sign in to confirm you’re not a bot. Use --cookies-from-browser",
                "not a bot",
            ),
            (
                "[TikTok] 1: Your IP address is blocked from accessing this post",
                "where you are",
            ),
            (
                "[Reddit] x: Account authentication is required. Use --cookies-from-browser or --cookies",
                "signed in",
            ),
            (
                "[vimeo] 1: The web client only works when logged-in. Use --cookies",
                "signed in",
            ),
        ] {
            let got = last_error(&format!("ERROR: {raw}"));
            assert!(got.contains(want), "{raw} -> {got}");
        }
    }

    #[test]
    fn spots_format_errors() {
        assert!(is_format_error(
            "[youtube] x35MG2bbKqo: Requested format is not available. Use --list-formats for a list of available formats"
        ));
        assert!(is_format_error("No video formats found!"));
        assert!(!is_format_error("HTTP Error 404: Not Found"));
    }
}
