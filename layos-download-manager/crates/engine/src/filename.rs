//! Working out a safe Windows file name for a download.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

const MAX_NAME_CHARS: usize = 200;
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Reads the file name from a `Content-Disposition` header.
/// `filename*` (RFC 5987, any charset we can read as UTF-8) wins over `filename`.
pub fn from_content_disposition(header: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for part in split_params(header) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "filename*" => {
                // charset'language'percent-encoded
                let mut it = value.trim_matches('"').splitn(3, '\'');
                let (_charset, _lang, encoded) = (it.next(), it.next(), it.next());
                if let Some(encoded) = encoded {
                    let decoded = percent_decode_str(encoded).decode_utf8_lossy().into_owned();
                    if !decoded.is_empty() {
                        extended = Some(decoded);
                    }
                }
            }
            "filename" => {
                let v = unquote(value);
                if !v.is_empty() {
                    // Some servers percent-encode plain filename too.
                    let v = if v.contains('%') {
                        percent_decode_str(&v).decode_utf8_lossy().into_owned()
                    } else {
                        v
                    };
                    plain = Some(v);
                }
            }
            _ => {}
        }
    }
    extended.or(plain).map(|n| base_name(&n).to_string())
}

/// Splits on `;` outside quotes.
fn split_params(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '\\' if in_quotes && !escaped => {
                escaped = true;
                continue;
            }
            '"' if !escaped => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        escaped = false;
    }
    out.push(&s[start..]);
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        v.to_string()
    }
}

/// Drops any directory part a server may have sent.
fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// The last path segment of a URL, percent-decoded.
pub fn from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.rfind(|s| !s.is_empty())?;
    let decoded = percent_decode_str(last).decode_utf8_lossy().into_owned();
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// Picks the best name from the server's headers and the URL, then makes it safe.
pub fn choose(
    content_disposition: Option<&str>,
    final_url: &str,
    original_url: &str,
    content_type: Option<&str>,
) -> String {
    let name = content_disposition
        .and_then(from_content_disposition)
        .or_else(|| from_url(final_url))
        .or_else(|| from_url(original_url))
        .unwrap_or_else(|| "download".to_string());
    let mut name = sanitize(&name);
    if Path::new(&name).extension().is_none()
        && let Some(ext) = content_type.and_then(extension_for_mime)
    {
        name.push('.');
        name.push_str(ext);
    }
    name
}

fn extension_for_mime(content_type: &str) -> Option<&'static str> {
    let mime = content_type.split(';').next()?.trim().to_ascii_lowercase();
    // Types where mime_guess's first pick is not the common one.
    let preferred = match mime.as_str() {
        "text/plain" => Some("txt"),
        "text/html" => Some("html"),
        "image/jpeg" => Some("jpg"),
        "video/mp4" => Some("mp4"),
        "audio/mpeg" => Some("mp3"),
        "application/octet-stream" | "binary/octet-stream" => return None,
        _ => None,
    };
    preferred.or_else(|| {
        mime_guess::get_mime_extensions_str(&mime).and_then(|exts| exts.first().copied())
    })
}

/// Makes a name valid on Windows: no forbidden characters, no reserved device names,
/// no trailing dots or spaces, and not too long.
pub fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out
        .trim()
        .trim_end_matches(['.', ' '])
        .trim_start_matches(' ')
        .to_string();
    if out.is_empty() || out.chars().all(|c| c == '.') {
        out = "download".to_string();
    }
    let stem = out
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    if out.chars().count() > MAX_NAME_CHARS {
        let path = Path::new(&out);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.chars().count() <= 16)
            .map(str::to_string);
        let keep = MAX_NAME_CHARS - ext.as_ref().map_or(0, |e| e.chars().count() + 1);
        let stem_src = match &ext {
            Some(e) => &out[..out.len() - e.len() - 1],
            None => out.as_str(),
        };
        let mut shortened: String = stem_src.chars().take(keep).collect();
        if let Some(e) = ext {
            shortened.push('.');
            shortened.push_str(&e);
        }
        out = shortened;
    }
    out
}

/// The part file a download writes to while it runs.
pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".ldmpart");
    PathBuf::from(s)
}

/// Returns `dir/name`, or `dir/name (1).ext`, `(2)`... if that file or its part file exists.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !taken(&candidate) {
        return candidate;
    }
    let p = Path::new(name);
    let (stem, ext) = match (
        p.file_stem().and_then(|s| s.to_str()),
        p.extension().and_then(|e| e.to_str()),
    ) {
        (Some(stem), Some(ext)) => (stem.to_string(), Some(ext.to_string())),
        _ => (name.to_string(), None),
    };
    for n in 1.. {
        let file = match &ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(file);
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn taken(path: &Path) -> bool {
    path.exists() || part_path(path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_forms() {
        assert_eq!(
            from_content_disposition("attachment; filename=\"a b.zip\"").as_deref(),
            Some("a b.zip")
        );
        assert_eq!(
            from_content_disposition("attachment; filename=plain.txt").as_deref(),
            Some("plain.txt")
        );
        assert_eq!(
            from_content_disposition(
                "attachment; filename=\"fallback.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"
            )
            .as_deref(),
            Some("résumé.pdf")
        );
        assert_eq!(
            from_content_disposition("attachment; filename=\"semi;colon.txt\"").as_deref(),
            Some("semi;colon.txt")
        );
        assert_eq!(
            from_content_disposition("attachment; filename=\"../../evil.exe\"").as_deref(),
            Some("evil.exe")
        );
        assert_eq!(from_content_disposition("inline"), None);
    }

    #[test]
    fn url_names() {
        assert_eq!(
            from_url("https://x.com/files/My%20File.iso?token=1").as_deref(),
            Some("My File.iso")
        );
        assert_eq!(from_url("https://x.com/dir/"), Some("dir".into()));
        assert_eq!(from_url("https://x.com/"), None);
    }

    #[test]
    fn sanitizes_for_windows() {
        assert_eq!(sanitize("a<b>c:d.txt"), "a_b_c_d.txt");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("name. . "), "name");
        assert_eq!(sanitize(""), "download");
        let long = format!("{}.mp4", "x".repeat(300));
        let s = sanitize(&long);
        assert_eq!(s.chars().count(), MAX_NAME_CHARS);
        assert!(s.ends_with(".mp4"));
    }

    #[test]
    fn adds_extension_from_type() {
        assert_eq!(
            choose(
                None,
                "https://x.com/get",
                "https://x.com/get",
                Some("application/pdf")
            ),
            "get.pdf"
        );
        assert_eq!(
            choose(
                None,
                "https://x.com/",
                "https://x.com/",
                Some("application/octet-stream")
            ),
            "download"
        );
    }

    #[test]
    fn unique_names() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unique_path(dir.path(), "a.zip"), dir.path().join("a.zip"));
        std::fs::write(dir.path().join("a.zip"), b"").unwrap();
        std::fs::write(dir.path().join("a (1).zip.ldmpart"), b"").unwrap();
        assert_eq!(
            unique_path(dir.path(), "a.zip"),
            dir.path().join("a (2).zip")
        );
    }
}
