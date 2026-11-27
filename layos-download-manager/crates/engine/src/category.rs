//! Sorting files into categories by extension, each with its own save folder.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    General,
    Compressed,
    Documents,
    Music,
    Programs,
    Video,
    /// Torrents go to their own folder, whatever is inside them.
    Torrents,
}

const COMPRESSED: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "lz", "lzma", "cab",
    "arj", "iso", "img", "dmg", "z",
];
const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "txt", "csv",
    "epub", "mobi", "azw3", "djvu", "md", "pages", "numbers", "key", "xps",
];
const MUSIC: &[&str] = &[
    "mp3", "m4a", "aac", "flac", "wav", "ogg", "oga", "opus", "wma", "aif", "aiff", "alac", "ape",
    "mid", "midi",
];
const PROGRAMS: &[&str] = &[
    "exe",
    "msi",
    "msix",
    "msixbundle",
    "appx",
    "bat",
    "cmd",
    "apk",
    "jar",
    "deb",
    "rpm",
    "appimage",
    "pkg",
];
const VIDEO: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "avi", "mov", "wmv", "flv", "mpg", "mpeg", "3gp", "3g2", "ts",
    "m2ts", "mts", "vob", "ogv", "rm", "rmvb", "asf",
];

impl Category {
    pub const ALL: [Category; 7] = [
        Self::General,
        Self::Compressed,
        Self::Documents,
        Self::Music,
        Self::Programs,
        Self::Video,
        Self::Torrents,
    ];

    pub fn from_file_name(name: &str) -> Self {
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        Self::from_extension(&ext)
    }

    pub fn from_extension(ext: &str) -> Self {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        let ext = ext.as_str();
        if VIDEO.contains(&ext) {
            Self::Video
        } else if MUSIC.contains(&ext) {
            Self::Music
        } else if COMPRESSED.contains(&ext) {
            Self::Compressed
        } else if DOCUMENTS.contains(&ext) {
            Self::Documents
        } else if PROGRAMS.contains(&ext) {
            Self::Programs
        } else {
            Self::General
        }
    }

    /// The sub-folder of the main download folder used by default.
    pub fn folder_name(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Compressed => "Compressed",
            Self::Documents => "Documents",
            Self::Music => "Music",
            Self::Programs => "Programs",
            Self::Video => "Video",
            Self::Torrents => "Torrents",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_extension() {
        assert_eq!(Category::from_file_name("movie.MKV"), Category::Video);
        assert_eq!(Category::from_file_name("song.flac"), Category::Music);
        assert_eq!(Category::from_file_name("setup.exe"), Category::Programs);
        assert_eq!(Category::from_file_name("a.tar.gz"), Category::Compressed);
        assert_eq!(Category::from_file_name("report.pdf"), Category::Documents);
        assert_eq!(Category::from_file_name("noextension"), Category::General);
    }
}
