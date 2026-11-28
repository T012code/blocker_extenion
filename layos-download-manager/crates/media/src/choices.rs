//! Turns yt-dlp's long list of formats into a short menu: one entry per picture
//! height, plus "audio only", with the size where it's known.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaChoice {
    /// yt-dlp format selector, e.g. `137+140` or `bv*+ba/b`.
    pub format: String,
    /// What the user sees, e.g. "1080p60 MP4".
    pub label: String,
    /// "video" or "audio".
    pub kind: String,
    pub ext: String,
    pub height: Option<u32>,
    /// Bytes, exact or estimated. `None` when unknown (live streams, some sites).
    pub size: Option<u64>,
    /// Which video of a post with several (an Instagram carousel, an X post with
    /// many videos), counting from 1. `None` for a page with one video.
    #[serde(default)]
    pub item: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub title: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    /// Name of the site handler yt-dlp used (e.g. "youtube", "generic").
    pub extractor: Option<String>,
    pub is_live: bool,
    pub choices: Vec<MediaChoice>,
}

/// A format from yt-dlp's JSON, keeping only what the menu needs.
#[derive(Debug, Clone)]
struct Format {
    id: String,
    ext: String,
    height: Option<u32>,
    fps: Option<f64>,
    has_video: bool,
    has_audio: bool,
    size: Option<u64>,
    tbr: f64,
}

fn codec_present(v: Option<&Value>) -> Option<bool> {
    match v.and_then(Value::as_str) {
        Some("none") => Some(false),
        Some(_) => Some(true),
        None => None,
    }
}

fn parse(f: &Value) -> Option<Format> {
    let id = f.get("format_id")?.as_str()?.to_string();
    let ext = f
        .get("ext")
        .and_then(Value::as_str)
        .unwrap_or("mp4")
        .to_string();
    let height = f.get("height").and_then(Value::as_u64).map(|h| h as u32);
    let vcodec = codec_present(f.get("vcodec"));
    let acodec = codec_present(f.get("acodec"));
    // Unknown codecs: guess from what else we know (HLS playlists and plain files often
    // omit them). A video container with no other clues is taken to hold picture and sound.
    let video_ext = matches!(
        ext.as_str(),
        "mp4" | "m4v" | "webm" | "mkv" | "mov" | "flv" | "3gp" | "ts" | "ogv"
    );
    let has_video = vcodec.unwrap_or(
        height.is_some()
            || f.get("width").is_some_and(|w| !w.is_null())
            || (acodec.is_none() && video_ext),
    );
    let has_audio = acodec.unwrap_or(
        vcodec != Some(true) || f.get("audio_ext").and_then(Value::as_str) != Some("none"),
    );
    // Storyboards and thumbnails are formats too; skip them.
    if ext == "mhtml"
        || f.get("format_note")
            .and_then(Value::as_str)
            .is_some_and(|n| n.contains("storyboard"))
    {
        return None;
    }
    let size = f
        .get("filesize")
        .and_then(Value::as_u64)
        .or_else(|| f.get("filesize_approx").and_then(Value::as_u64));
    Some(Format {
        id,
        ext,
        height,
        fps: f.get("fps").and_then(Value::as_f64),
        has_video,
        has_audio,
        size,
        tbr: f.get("tbr").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

fn ext_rank(ext: &str) -> u8 {
    match ext {
        "mp4" | "m4a" => 2,
        "webm" => 1,
        _ => 0,
    }
}

pub(crate) fn from_info(info: &Value) -> MediaInfo {
    let formats: Vec<Format> = match info.get("formats").and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(parse).collect(),
        // A single file has no "formats" list, just its own fields.
        None => parse(info).into_iter().collect(),
    };
    MediaInfo {
        title: info
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("video")
            .to_string(),
        duration: info.get("duration").and_then(Value::as_f64),
        thumbnail: info
            .get("thumbnail")
            .and_then(Value::as_str)
            .map(str::to_string),
        extractor: info
            .get("extractor_key")
            .and_then(Value::as_str)
            .map(|s| s.to_lowercase()),
        is_live: info
            .get("is_live")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        choices: if formats.is_empty() {
            from_entries(info)
        } else {
            build_choices_from(&formats)
        },
    }
}

/// Public for tests and other callers that already have yt-dlp's JSON.
/// A post with several videos comes as a list: one set of choices per video, each
/// labelled with its number ("Video 2 · 720p MP4").
fn from_entries(info: &Value) -> Vec<MediaChoice> {
    let Some(entries) = info.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let videos: Vec<(u32, Vec<MediaChoice>)> = entries
        .iter()
        .enumerate()
        .take(20)
        .map(|(i, e)| (i as u32 + 1, from_info(e).choices))
        .filter(|(_, c)| !c.is_empty())
        .collect();
    let several = videos.len() > 1;
    videos
        .into_iter()
        .flat_map(|(n, choices)| {
            choices.into_iter().map(move |mut c| {
                c.item = Some(n);
                if several {
                    c.label = format!("Video {n} · {}", c.label);
                }
                c
            })
        })
        .collect()
}

pub fn build_choices(info: &Value) -> Vec<MediaChoice> {
    from_info(info).choices
}

fn build_choices_from(formats: &[Format]) -> Vec<MediaChoice> {
    let best_audio = formats
        .iter()
        .filter(|f| f.has_audio && !f.has_video)
        .max_by(|a, b| {
            ext_rank(&a.ext)
                .cmp(&ext_rank(&b.ext))
                .then(a.tbr.total_cmp(&b.tbr))
        });

    let mut heights: Vec<u32> = formats
        .iter()
        .filter(|f| f.has_video)
        .filter_map(|f| f.height)
        .collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();

    let mut out = Vec::new();
    for h in heights.into_iter().take(8) {
        // At each height, prefer a file that already has sound, then MP4, then the higher bitrate.
        let Some(v) = formats
            .iter()
            .filter(|f| f.has_video && f.height == Some(h))
            .max_by(|a, b| {
                (
                    a.has_audio as u8,
                    ext_rank(&a.ext),
                    (a.fps.unwrap_or(0.0) > 30.0) as u8,
                )
                    .cmp(&(
                        b.has_audio as u8,
                        ext_rank(&b.ext),
                        (b.fps.unwrap_or(0.0) > 30.0) as u8,
                    ))
                    .then(a.tbr.total_cmp(&b.tbr))
            })
        else {
            continue;
        };
        let fps = v
            .fps
            .filter(|f| *f > 30.5)
            .map(|f| format!("{}", f.round() as u32))
            .unwrap_or_default();
        let (format, size, ext) = match (v.has_audio, best_audio) {
            (false, Some(a)) => {
                let ext = if v.ext == "mp4" && a.ext == "m4a" {
                    "mp4"
                } else if v.ext == "webm" {
                    "webm"
                } else {
                    "mkv"
                };
                (
                    format!("{}+{}/{}", v.id, a.id, same_height(h)),
                    v.size.zip(a.size).map(|(x, y)| x + y),
                    ext.to_string(),
                )
            }
            _ => (
                format!("{}/{}", v.id, same_height(h)),
                v.size,
                v.ext.clone(),
            ),
        };
        out.push(MediaChoice {
            format,
            label: format!("{h}p{fps} {}", ext.to_uppercase()),
            kind: "video".into(),
            ext,
            height: Some(h),
            size,
            item: None,
        });
    }
    // Plain files (no height known) still deserve an entry.
    if out.is_empty()
        && let Some(v) = formats
            .iter()
            .filter(|f| f.has_video)
            .max_by(|a, b| a.tbr.total_cmp(&b.tbr))
    {
        out.push(MediaChoice {
            format: format!("{}/bv*+ba/b", v.id),
            label: format!("Video {}", v.ext.to_uppercase()),
            kind: "video".into(),
            ext: v.ext.clone(),
            height: None,
            size: v.size,
            item: None,
        });
    }
    if let Some(a) = best_audio {
        out.push(MediaChoice {
            format: format!("{}/ba/b", a.id),
            label: format!("Audio only {}", a.ext.to_uppercase()),
            kind: "audio".into(),
            ext: a.ext.clone(),
            height: None,
            size: a.size,
            item: None,
        });
    }
    out
}

/// After the exact format, what to fall back to: the same height, then the best there
/// is. Format ids can differ between the listing and the download (sites hand out
/// different sets), and a missing one must not fail the download.
fn same_height(h: u32) -> String {
    format!("bv*[height<={h}]+ba/b[height<={h}]/bv*+ba/b")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_post_with_several_videos_lists_each() {
        let video = |id: &str| {
            json!({"title": "Video", "formats": [
                {"format_id": id, "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "mp4a", "tbr": 900}
            ]})
        };
        let info = json!({"_type": "playlist", "title": "Post by someone", "entries": [video("a"), json!({"title": "photo"}), video("b")]});
        let c = from_info(&info).choices;
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].label, "Video 1 · 720p MP4");
        assert_eq!(c[0].item, Some(1));
        assert_eq!(c[1].label, "Video 3 · 720p MP4");
        assert_eq!(
            c[1].item,
            Some(3),
            "numbered as in the post, photos counted"
        );

        let one = json!({"_type": "playlist", "title": "Post", "entries": [video("a")]});
        let c = from_info(&one).choices;
        assert_eq!(c[0].label, "720p MP4", "no number for a single video");
        assert_eq!(c[0].item, Some(1));
    }

    #[test]
    fn one_entry_per_height_with_merged_audio() {
        let info = json!({
            "title": "Talk", "extractor_key": "Youtube", "duration": 60.0,
            "formats": [
                {"format_id": "sb0", "ext": "mhtml", "format_note": "storyboard"},
                {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "filesize": 1000, "tbr": 128},
                {"format_id": "251", "ext": "webm", "vcodec": "none", "acodec": "opus", "filesize": 900, "tbr": 140},
                {"format_id": "137", "ext": "mp4", "height": 1080, "fps": 30, "vcodec": "avc1", "acodec": "none", "filesize": 50000, "tbr": 4000},
                {"format_id": "248", "ext": "webm", "height": 1080, "fps": 30, "vcodec": "vp9", "acodec": "none", "filesize": 40000, "tbr": 3000},
                {"format_id": "299", "ext": "mp4", "height": 1080, "fps": 60, "vcodec": "avc1", "acodec": "none", "filesize": 80000, "tbr": 6000},
                {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a", "filesize": 5000, "tbr": 500}
            ]
        });
        let info = from_info(&info);
        assert_eq!(info.extractor.as_deref(), Some("youtube"));
        let c = info.choices;
        assert_eq!(c[0].label, "1080p60 MP4");
        assert_eq!(
            c[0].format,
            "299+140/bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b"
        );
        assert_eq!(c[0].size, Some(81000));
        assert_eq!(c[1].label, "360p MP4");
        assert_eq!(
            c[1].format, "18/bv*[height<=360]+ba/b[height<=360]/bv*+ba/b",
            "a file with sound already needs no merging"
        );
        assert_eq!(c[2].label, "Audio only M4A");
        assert_eq!(c[2].kind, "audio");
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn hls_variants_without_codecs() {
        let info = json!({"title": "Stream", "formats": [
            {"format_id": "hls-800", "ext": "mp4", "height": 360, "width": 640, "tbr": 800},
            {"format_id": "hls-2400", "ext": "mp4", "height": 720, "width": 1280, "tbr": 2400}
        ]});
        let c = build_choices(&info);
        assert_eq!(
            c.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["720p MP4", "360p MP4"]
        );
        assert!(c[0].format.starts_with("hls-2400/"));
    }

    #[test]
    fn plain_video_file_without_codec_details_is_video() {
        let c = build_choices(
            &json!({"title": "page", "formats": [{"format_id": "mp4", "ext": "mp4"}]}),
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].label, "Video MP4");
        assert_eq!(c[0].kind, "video");
        // An audio file stays audio.
        let a = build_choices(
            &json!({"formats": [{"format_id": "0", "ext": "mp3", "vcodec": "none"}]}),
        );
        assert_eq!(a[0].kind, "audio");
    }

    #[test]
    fn single_file_without_format_list() {
        let c = build_choices(
            &json!({"title": "clip", "format_id": "0", "ext": "webm", "vcodec": "vp8", "acodec": "vorbis"}),
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].label, "Video WEBM");
    }
}
