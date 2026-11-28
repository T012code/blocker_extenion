use serde::{Deserialize, Serialize};

/// Where a download request came from in the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// The browser started a download and the extension took it over.
    #[default]
    Capture,
    /// "Download with Layó's DM" on a link.
    Link,
    /// "Download all links" on a page.
    AllLinks,
    /// A video or audio file found on the page.
    Media,
}

/// One file the browser wants downloaded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDownload {
    pub url: String,
    #[serde(default)]
    pub referer: Option<String>,
    /// `Cookie` header value for `url`.
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    /// The name the browser would have used.
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub mime: Option<String>,
    /// The page the link was on.
    #[serde(default)]
    pub page_url: Option<String>,
    /// Link text, used as a description.
    #[serde(default)]
    pub description: Option<String>,
    /// Set when this is a video/audio format picked from the video button.
    #[serde(default)]
    pub media: Option<MediaPick>,
}

/// A format picked in the video button's menu.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPick {
    /// The page or stream playlist the format came from.
    pub page_url: String,
    /// The format selector from [`MediaFormatChoice::format`]; `None` = best.
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Which video of a post with several, counting from 1.
    #[serde(default)]
    pub item: Option<u32>,
    /// `page_url` is a playlist: download every video in it, at `format`.
    #[serde(default)]
    pub playlist: bool,
}

/// A playlist the video button can download whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    /// The playlist's own page.
    pub url: String,
    pub title: String,
    /// How many videos can be downloaded.
    pub count: usize,
    /// The choices: (label, format, kind) for the whole list.
    pub choices: Vec<MediaFormatChoice>,
}

/// One entry of the video button's menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFormatChoice {
    pub format: String,
    pub label: String,
    /// "video" or "audio".
    pub kind: String,
    pub ext: String,
    pub height: Option<u32>,
    pub size: Option<u64>,
    /// Which video of a post with several, counting from 1.
    #[serde(default)]
    pub item: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Request {
    /// Is the app there? Never starts the app.
    Ping,
    /// Download these files.
    #[serde(rename_all = "camelCase")]
    Download {
        items: Vec<BrowserDownload>,
        #[serde(default)]
        source: Source,
        /// Show the "add download" window first. `None` follows the app's setting.
        #[serde(default)]
        ask: Option<bool>,
    },
    /// Bring the app window to the front.
    ShowApp,
    /// Current downloads with progress. Never starts the app.
    List,
    /// Pause a running download. It keeps what it has, if the server can resume.
    Pause { id: i64 },
    /// Stop a download for good: it stays in the list, keeps what it has, and only
    /// starts again when asked to.
    Stop { id: i64 },
    /// Continue a paused, failed or waiting download.
    Resume { id: i64 },
    /// Which formats can be downloaded from this page or stream? `item.url` is the page
    /// (or playlist) address, with the browser's cookies.
    #[serde(rename_all = "camelCase")]
    MediaFormats {
        item: Box<BrowserDownload>,
        /// The playlist the page is playing (YouTube's `list=`), to offer it whole.
        #[serde(default)]
        playlist_url: Option<String>,
    },
    /// An expired link: try it again, or else wait for the browser to send a fresh one
    /// for this download (from `pageUrl` in the answer).
    RefreshLink { id: i64 },
}

impl Request {
    /// Whether the bridge should start the app to answer this.
    pub fn needs_app(&self) -> bool {
        !matches!(self, Request::Ping | Request::List)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    #[serde(rename_all = "camelCase")]
    Pong {
        app_running: bool,
        app_version: Option<String>,
        host_version: Option<String>,
    },
    /// The app took the downloads. `ids` is empty when they went to the "add download" window.
    Accepted {
        ids: Vec<i64>,
    },
    Ok,
    #[serde(rename_all = "camelCase")]
    MediaFormats {
        title: String,
        duration: Option<f64>,
        thumbnail: Option<String>,
        is_live: bool,
        choices: Vec<MediaFormatChoice>,
        /// The playlist the page is playing, when asked for and found.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        playlist: Option<PlaylistSummary>,
    },
    #[serde(rename_all = "camelCase")]
    Downloads {
        app_running: bool,
        items: Vec<DownloadSummary>,
    },
    Error {
        message: String,
    },
    /// Answer to `RefreshLink`: `resumed` when the old link works again; otherwise open
    /// `page_url` in the browser and download the file there again.
    #[serde(rename_all = "camelCase")]
    Refresh {
        resumed: bool,
        page_url: Option<String>,
    },
}

/// A download as the browser extension shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSummary {
    pub id: i64,
    pub file_name: String,
    /// queued, connecting, downloading, paused, completed, failed or needsRefresh.
    pub status: String,
    pub size: Option<u64>,
    pub downloaded: u64,
    /// Bytes per second while downloading.
    pub speed: u64,
    /// The server supports byte ranges, so pausing keeps progress. When false, a paused
    /// download starts again from the beginning.
    pub resumable: bool,
    /// Sections (connections) configured for it.
    pub connections: u8,
    pub error: Option<String>,
    /// "file", "video" or "torrent".
    #[serde(default)]
    pub kind: String,
}

impl Response {
    pub fn error(message: impl Into<String>) -> Self {
        Response::Error {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape_matches_the_extension() {
        let json = r#"{"type":"download","source":"link","items":[{"url":"https://x/a.zip","userAgent":"UA","fileSize":5}]}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        let Request::Download { items, source, ask } = req else {
            panic!()
        };
        assert_eq!(source, Source::Link);
        assert_eq!(ask, None);
        assert_eq!(items[0].user_agent.as_deref(), Some("UA"));
        assert_eq!(items[0].file_size, Some(5));
        assert_eq!(
            serde_json::to_string(&Response::Accepted { ids: vec![1] }).unwrap(),
            r#"{"type":"accepted","ids":[1]}"#
        );
        let pong = Response::Pong {
            app_running: true,
            app_version: Some("1".into()),
            host_version: None,
        };
        assert!(
            serde_json::to_string(&pong)
                .unwrap()
                .contains("\"appRunning\":true")
        );
    }
}
