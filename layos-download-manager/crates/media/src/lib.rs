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

/// A playlist's videos, listed without looking each one up (fast even for long lists).
#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
    pub title: String,
    pub entries: Vec<PlaylistEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    /// The video's own page.
    pub url: String,
    pub title: Option<String>,
    pub duration: Option<f64>,
}

/// The quality choices offered for a whole playlist: (label, yt-dlp format, is audio).
pub const PLAYLIST_QUALITIES: &[(&str, &str, bool)] = &[
    ("Best quality", "bv*+ba/b", false),
    (
        "1080p",
        "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b",
        false,
    ),
    ("720p", "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b", false),
    ("480p", "bv*[height<=480]+ba/b[height<=480]/bv*+ba/b", false),
    ("Audio only", "ba/b", true),
];

/// The playlist's own page for a video page that plays one (YouTube's `list=`), or
/// `None`. Auto-generated mixes (`RD…`) never end, so they don't count.
pub fn playlist_url(page: &str) -> Option<String> {
    let (_, query) = page.split_once('?')?;
    let query = query.split('#').next().unwrap_or(query);
    let host = page
        .split("://")
        .nth(1)?
        .split(['/', '?'])
        .next()?
        .to_ascii_lowercase();
    if !(host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be") {
        return None;
    }
    let list = query.split('&').find_map(|kv| kv.strip_prefix("list="))?;
    if list.is_empty()
        || list.starts_with("RD")
        || !list
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(format!("https://www.youtube.com/playlist?list={list}"))
}

/// Lists a playlist's videos (titles and pages, not their formats).
pub async fn playlist(tools: &Tools, req: &RequestInfo, timeout: Duration) -> Result<Playlist> {
    let mut cmd = tools::ytdlp_command(tools)?;
    cmd.args([
        "-J",
        "--flat-playlist",
        "--yes-playlist",
        "--no-warnings",
        "--no-colors",
    ]);
    cmd.arg("--").arg(&req.url);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| Error::Other("listing the playlist took too long".into()))??;
    if !out.status.success() {
        return Err(Error::Other(tools::last_error(&String::from_utf8_lossy(
            &out.stderr,
        ))));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(playlist_from(&json))
}

fn playlist_from(json: &serde_json::Value) -> Playlist {
    let entries = json
        .get("entries")
        .and_then(|e| e.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|e| {
                    let url = e
                        .get("url")
                        .or_else(|| e.get("webpage_url"))?
                        .as_str()?
                        .to_string();
                    let title = e.get("title").and_then(|t| t.as_str()).map(str::to_string);
                    // Private and deleted videos stay in lists, without a title.
                    if matches!(
                        title.as_deref(),
                        Some("[Private video]" | "[Deleted video]")
                    ) {
                        return None;
                    }
                    Some(PlaylistEntry {
                        url,
                        title,
                        duration: e.get("duration").and_then(|d| d.as_f64()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Playlist {
        title: json
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Playlist")
            .to_string(),
        entries,
    }
}

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

#[cfg(test)]
mod playlist_tests {
    use super::*;

    #[test]
    fn finds_the_playlist_of_a_video_page() {
        assert_eq!(
            playlist_url(
                "https://www.youtube.com/watch?v=jNQXAC9IVRw&list=PLBCF2DAC6FFB574DE&index=2"
            )
            .as_deref(),
            Some("https://www.youtube.com/playlist?list=PLBCF2DAC6FFB574DE")
        );
        assert_eq!(
            playlist_url("https://youtube.com/playlist?list=PLx-_9").as_deref(),
            Some("https://www.youtube.com/playlist?list=PLx-_9")
        );
        assert_eq!(playlist_url("https://www.youtube.com/watch?v=x"), None);
        assert_eq!(
            playlist_url("https://www.youtube.com/watch?v=x&list=RDx"),
            None,
            "mixes never end"
        );
        assert_eq!(playlist_url("https://example.com/watch?list=PL1"), None);
        assert_eq!(
            playlist_url("https://www.youtube.com/watch?v=x&list=a%22b"),
            None
        );
    }

    #[test]
    fn lists_entries_and_skips_private_ones() {
        let p = playlist_from(
            &serde_json::json!({"_type": "playlist", "title": "Stories", "entries": [
                {"_type": "url", "url": "https://www.youtube.com/watch?v=a", "title": "A", "duration": 90.0},
                {"_type": "url", "url": "https://www.youtube.com/watch?v=b", "title": "[Private video]"},
                {"_type": "url", "url": "https://www.youtube.com/watch?v=c", "title": null}
            ]}),
        );
        assert_eq!(p.title, "Stories");
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].title.as_deref(), Some("A"));
        assert_eq!(p.entries[1].url, "https://www.youtube.com/watch?v=c");
    }
}
