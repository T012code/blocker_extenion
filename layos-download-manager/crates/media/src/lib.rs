//! Video and audio downloads for Layó's Download Manager.
//!
//! The browser extension finds videos on a page; this crate asks
//! [yt-dlp](https://github.com/yt-dlp/yt-dlp) which formats exist, turns them into a short
//! list (1080p, 720p, audio only…), and downloads the chosen one. Streams made of many
//! small pieces (HLS, DASH) are fetched several pieces at a time, and separate video
//! and audio tracks are joined with ffmpeg. yt-dlp and ffmpeg ship with the installer.

mod choices;
mod runner;
mod tools;

pub use choices::{MediaChoice, MediaInfo, build_choices};
pub use runner::MediaRunner;
pub use tools::Tools;

use std::time::Duration;

use ldm_engine::{Error, RequestInfo, Result};

/// Asks yt-dlp what can be downloaded from `url` (a page or a stream playlist).
///
/// First with the browser's cookies and details (needed for private or signed-in
/// videos); if the site then offers no usable format, once more without them.
pub async fn formats(tools: &Tools, req: &RequestInfo, timeout: Duration) -> Result<MediaInfo> {
    let first = formats_once(tools, req, timeout, false).await;
    match first {
        Err(Error::Other(ref m))
            if tools::is_format_error(m) && tools::sends_browser_details(req) =>
        {
            tracing::info!("no formats with the browser's details; trying without: {m}");
            formats_once(tools, req, timeout, true)
                .await
                .map_err(|e| match e {
                    Error::Other(m) => Error::Other(tools::last_error(&m)),
                    e => e,
                })
        }
        Err(Error::Other(m)) => Err(Error::Other(tools::last_error(&m))),
        r => r,
    }
}

/// One try. Errors carry yt-dlp's raw message (made readable by the caller).
async fn formats_once(
    tools: &Tools,
    req: &RequestInfo,
    timeout: Duration,
    plain: bool,
) -> Result<MediaInfo> {
    let cookies = if plain {
        None
    } else {
        tools::CookieFile::write(req)?
    };
    let mut cmd = tools::ytdlp_command(tools)?;
    cmd.args(["-J", "--no-playlist", "--no-warnings", "--no-colors"]);
    if !plain {
        tools::add_request_args(&mut cmd, req, cookies.as_ref());
    }
    cmd.arg("--").arg(&req.url);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| Error::Other("looking for video formats took too long".into()))??;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let line = stderr
            .lines()
            .rev()
            .find(|l| l.starts_with("ERROR:"))
            .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("the video downloader stopped with an error");
        return Err(Error::Other(line.to_string()));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(choices::from_info(&json))
}
