//! Checks and fingerprints for copied content.

use sha2::{Digest, Sha256};

/// Text bigger than this (in bytes) stays on this PC only.
pub const MAX_TEXT_BYTES: usize = 100 * 1024;

/// Why a copy isn't synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    Empty,
    TooBig,
}

/// Checks copied text. Text is synced exactly as copied (no trimming), so pasting it on
/// another PC gives the same result.
pub fn check_text(text: &str) -> Result<(), Skip> {
    if text.trim().is_empty() {
        Err(Skip::Empty)
    } else if text.len() > MAX_TEXT_BYTES {
        Err(Skip::TooBig)
    } else {
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

/// The fingerprint of a text clip: copying the same text again matches it.
pub fn text_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"text\0");
    h.update(text.as_bytes());
    hex(&h.finalize())
}

/// The fingerprint of an image clip, from its pixels (so the same picture copied from two
/// apps in two formats still matches).
pub fn image_hash(rgba: &[u8], width: u32, height: u32) -> String {
    let mut h = Sha256::new();
    h.update(b"image\0");
    h.update(width.to_le_bytes());
    h.update(height.to_le_bytes());
    h.update(rgba);
    hex(&h.finalize())
}

/// The start of a text, on one line, for lists: runs of whitespace become one space.
pub fn excerpt(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            if count + 1 >= max_chars {
                out.push('…');
                return out;
            }
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= max_chars {
            out.push('…');
            return out;
        }
        out.push(c);
        count += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_checks() {
        assert_eq!(check_text("  \n\t"), Err(Skip::Empty));
        assert_eq!(check_text("hi"), Ok(()));
        assert_eq!(check_text(&"a".repeat(MAX_TEXT_BYTES)), Ok(()));
        assert_eq!(check_text(&"a".repeat(MAX_TEXT_BYTES + 1)), Err(Skip::TooBig));
    }

    #[test]
    fn hashes_are_stable_hex_and_distinct() {
        let a = text_hash("hello");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(a, text_hash("hello"));
        assert_ne!(a, text_hash("hello "));
        assert_ne!(image_hash(&[0, 0, 0, 255], 1, 1), image_hash(&[1, 0, 0, 255], 1, 1));
        assert_ne!(image_hash(&[0; 8], 2, 1), image_hash(&[0; 8], 1, 2));
    }

    #[test]
    fn excerpts() {
        assert_eq!(excerpt("  hello\n\n  world  ", 50), "hello world");
        assert_eq!(excerpt("abcdef", 3), "abc…");
        assert_eq!(excerpt("ab cd", 3), "ab…");
        assert_eq!(excerpt("", 3), "");
        assert_eq!(excerpt("héllo wörld", 20), "héllo wörld");
    }
}
