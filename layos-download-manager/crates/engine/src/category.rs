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

    /// The categories sorted by file extension, in the order they're checked.
    pub const BY_EXTENSION: [Category; 5] = [
        Self::Video,
        Self::Music,
        Self::Compressed,
        Self::Documents,
        Self::Programs,
    ];

    /// The extensions that belong here unless the user changed the list.
    pub fn default_extensions(self) -> &'static [&'static str] {
        match self {
            Self::Video => VIDEO,
            Self::Music => MUSIC,
            Self::Compressed => COMPRESSED,
            Self::Documents => DOCUMENTS,
            Self::Programs => PROGRAMS,
            Self::General | Self::Torrents => &[],
        }
    }

    /// Like `from_file_name`, but with the user's own lists for some categories. An
    /// extension the user put in a category always goes there; the rest follow the
    /// built-in lists (minus what the user took out of an edited category).
    pub fn from_file_name_with(
        name: &str,
        lists: &std::collections::HashMap<Category, Vec<String>>,
    ) -> Self {
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext.is_empty() {
            return Self::General;
        }
        for c in Self::BY_EXTENSION {
            if let Some(list) = lists.get(&c)
                && list
                    .iter()
                    .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
            {
                return c;
            }
        }
        for c in Self::BY_EXTENSION {
            if !lists.contains_key(&c) && c.default_extensions().contains(&ext.as_str()) {
                return c;
            }
        }
        Self::General
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

    #[test]
    fn the_users_lists_win() {
        let mut lists = std::collections::HashMap::new();
        // Disk images moved from Compressed to Programs; .xyz added to Video.
        lists.insert(Category::Programs, vec!["exe".into(), "iso".into()]);
        lists.insert(Category::Video, vec!["mp4".into(), ".XYZ".into()]);
        let of = |n: &str| Category::from_file_name_with(n, &lists);
        assert_eq!(of("ubuntu.iso"), Category::Programs);
        assert_eq!(of("clip.xyz"), Category::Video);
        assert_eq!(
            of("film.mkv"),
            Category::General,
            "taken out of the edited Video list"
        );
        assert_eq!(
            of("setup.msi"),
            Category::General,
            "Programs is now just exe and iso"
        );
        assert_eq!(
            of("song.mp3"),
            Category::Music,
            "untouched lists keep their defaults"
        );
        assert_eq!(of("a.zip"), Category::Compressed);
        assert_eq!(of("noext"), Category::General);
        // No lists: same as the built-in sorting.
        let none = std::collections::HashMap::new();
        for n in ["a.mkv", "b.flac", "c.7z", "d.pdf", "e.exe", "f.bin"] {
            assert_eq!(
                Category::from_file_name_with(n, &none),
                Category::from_file_name(n)
            );
        }
    }
}
