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
pub async fn formats(tools: &Tools, req: &RequestInfo, timeout: Duration) -> Result<MediaInfo> {
    let ytdlp = tools.ytdlp()?;
    let cookies = tools::CookieFile::write(req)?;
    let mut cmd = tools::command(ytdlp);
    cmd.args(["-J", "--no-playlist", "--no-warnings", "--no-colors"]);
    tools::add_request_args(&mut cmd, req, cookies.as_ref());
    cmd.arg("--").arg(&req.url);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| Error::Other("looking for video formats took too long".into()))??;
    if !out.status.success() {
        return Err(Error::Other(tools::last_error(&String::from_utf8_lossy(
            &out.stderr,
        ))));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(choices::from_info(&json))
}
