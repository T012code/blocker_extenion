//! Images: making them small enough to sync, thumbnails for the lists, and converting to
//! and from the Windows clipboard formats ("PNG" and CF_DIB). Pure Rust, so it's tested
//! on any computer.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::imageops::{self, FilterType};
use image::{ExtendedColorType, ImageEncoder, RgbImage, RgbaImage};

/// Synced images are at most this many pixels on their longest side...
pub const MAX_SIDE: u32 = 1920;
/// ...and at most this many bytes (the storage bucket enforces the same limit).
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// Thumbnails fit in this box (twice the size they're shown at, for sharp high-DPI screens).
const THUMB_W: u32 = 224;
const THUMB_H: u32 = 128;
/// The database allows thumbnails up to 24,000 base64 characters.
const THUMB_MAX_B64: usize = 24_000;

/// An image ready to upload.
#[derive(Debug, Clone)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
    /// Base64 JPEG.
    pub thumb_b64: String,
}

fn to_image(rgba: &[u8], width: u32, height: u32) -> Option<RgbaImage> {
    if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
        return None;
    }
    RgbaImage::from_raw(width, height, rgba.to_vec())
}

/// The size that fits within `max_w` × `max_h`, keeping the shape (never enlarged).
fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w <= max_w && h <= max_h {
        return (w, h);
    }
    let scale = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
    (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1))
}

/// Puts transparent areas on white (for JPEG, which has no transparency).
fn flatten(img: &RgbaImage) -> RgbImage {
    let mut out = RgbImage::new(img.width(), img.height());
    for (o, p) in out.pixels_mut().zip(img.pixels()) {
        let a = p[3] as u32;
        for c in 0..3 {
            o[c] = ((p[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
    }
    out
}

fn jpeg(img: &RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = JpegEncoder::new_with_quality(&mut out, quality);
    if enc.write_image(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgb8).is_err() {
        return Vec::new();
    }
    out
}

fn webp_lossless(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = WebPEncoder::new_lossless(&mut out);
    if enc.encode(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgba8).is_err() {
        return Vec::new();
    }
    out
}

fn thumbnail(img: &RgbaImage) -> String {
    let (tw, th) = fit(img.width(), img.height(), THUMB_W, THUMB_H);
    let small = imageops::resize(img, tw, th, FilterType::Triangle);
    let flat = flatten(&small);
    for q in [72u8, 55, 40] {
        let b64 = crate::model::b64(&jpeg(&flat, q));
        if b64.len() <= THUMB_MAX_B64 {
            return b64;
        }
    }
    // A thumbnail this stubborn is noise; a tiny one still shows the colours.
    let tiny = imageops::resize(&flat, (tw / 3).max(1), (th / 3).max(1), FilterType::Triangle);
    crate::model::b64(&jpeg(&tiny, 40))
}

/// Shrinks a copied image for syncing: at most 1920 px on its longest side and 2 MB.
/// Screenshots and pictures with transparency are kept sharp as lossless WebP when they
/// fit; photos fall back to JPEG. Returns None for an image that can't be made to fit.
pub fn encode_for_sync(rgba: &[u8], width: u32, height: u32) -> Option<Encoded> {
    let original = to_image(rgba, width, height)?;
    let (w, h) = fit(width, height, MAX_SIDE, MAX_SIDE);
    let mut img =
        if (w, h) == (width, height) { original } else { imageops::resize(&original, w, h, FilterType::Triangle) };
    let thumb_b64 = thumbnail(&img);

    let webp = webp_lossless(&img);
    if !webp.is_empty() && webp.len() <= MAX_BYTES {
        return Some(Encoded {
            bytes: webp,
            mime: "image/webp",
            ext: "webp",
            width: img.width(),
            height: img.height(),
            thumb_b64,
        });
    }
    // A transparent picture that only fits as JPEG loses its transparency (it's put on
    // white): better than not syncing it at all.
    for _ in 0..4 {
        let flat = flatten(&img);
        for q in [85u8, 72] {
            let bytes = jpeg(&flat, q);
            if !bytes.is_empty() && bytes.len() <= MAX_BYTES {
                return Some(Encoded {
                    bytes,
                    mime: "image/jpeg",
                    ext: "jpg",
                    width: img.width(),
                    height: img.height(),
                    thumb_b64,
                });
            }
        }
        let (nw, nh) = fit(img.width(), img.height(), img.width() * 3 / 4, img.height() * 3 / 4);
        if (nw, nh) == (img.width(), img.height()) {
            break;
        }
        img = imageops::resize(&img, nw, nh, FilterType::Triangle);
    }
    None
}

/// Opens a synced image (WebP or JPEG) or a PNG.
pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    image::load_from_memory(bytes).ok().map(|i| i.to_rgba8())
}

/// The clipboard's "PNG" format: keeps transparency for the apps that read it.
pub fn to_png(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Adaptive);
    if enc.write_image(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgba8).is_err() {
        return Vec::new();
    }
    out
}

/// CF_DIB: a 24-bit bottom-up bitmap (BITMAPINFOHEADER + pixels), the format every
/// Windows app can paste. Transparency is put on white.
pub fn to_dib(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = (img.width(), img.height());
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let size = stride * h as usize;
    let mut out = Vec::with_capacity(40 + size);
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(w as i32).to_le_bytes()); // biWidth
    out.extend_from_slice(&(h as i32).to_le_bytes()); // biHeight: positive = bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&(size as u32).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 DPI
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
    let flat = flatten(img);
    for y in (0..h).rev() {
        let start = out.len();
        for x in 0..w {
            let p = flat.get_pixel(x, y);
            out.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        out.resize(start + stride, 0);
    }
    out
}

/// Reads CF_DIB / CF_DIBV5 data (a bitmap without its file header).
pub fn from_dib(dib: &[u8]) -> Option<RgbaImage> {
    if dib.len() < 40 {
        return None;
    }
    let u32_at = |i: usize| u32::from_le_bytes([dib[i], dib[i + 1], dib[i + 2], dib[i + 3]]);
    let header = u32_at(0) as usize;
    if !(40..=124).contains(&header) || dib.len() < header {
        return None;
    }
    let bits = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let compression = u32_at(16);
    let colours_used = u32_at(32) as usize;
    // BI_BITFIELDS (3) and BI_ALPHABITFIELDS (6) put their colour masks after a short header.
    let masks = match (header, compression) {
        (40, 3) => 12,
        (40, 6) => 16,
        _ => 0,
    };
    let palette = match bits {
        1 | 4 | 8 => 4 * if colours_used > 0 { colours_used } else { 1usize << bits },
        _ => 0,
    };
    let offset = 14 + header + masks + palette;
    // Add the 14-byte file header the BMP reader expects.
    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(offset as u32).to_le_bytes());
    bmp.extend_from_slice(dib);
    let mut img = image::load(Cursor::new(bmp), image::ImageFormat::Bmp).ok()?.to_rgba8();
    // Many apps leave the alpha byte of 32-bit bitmaps at 0 and mean "opaque".
    if bits == 32 && img.pixels().all(|p| p[3] == 0) {
        for p in img.pixels_mut() {
            p[3] = 255;
        }
    }
    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, alpha: u8) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 90, alpha])
        })
    }

    /// Pseudo-random pixels: the worst case for compression.
    fn noise(w: u32, h: u32) -> RgbaImage {
        let mut seed = 0x2545_f491_u32;
        RgbaImage::from_fn(w, h, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let b = seed.to_le_bytes();
            image::Rgba([b[0], b[1], b[2], 255])
        })
    }

    #[test]
    fn fits() {
        assert_eq!(fit(100, 50, 1920, 1920), (100, 50));
        assert_eq!(fit(3840, 2160, 1920, 1920), (1920, 1080));
        assert_eq!(fit(1000, 4000, 1920, 1920), (480, 1920));
        assert_eq!(fit(5000, 1, 224, 128), (224, 1));
    }

    #[test]
    fn small_screenshot_stays_lossless_webp() {
        let img = gradient(640, 360, 255);
        let e = encode_for_sync(img.as_raw(), 640, 360).unwrap();
        assert_eq!((e.mime, e.ext, e.width, e.height), ("image/webp", "webp", 640, 360));
        assert!(e.bytes.len() <= MAX_BYTES);
        let back = decode(&e.bytes).unwrap();
        assert_eq!(back, img, "lossless round trip");
        assert!(!e.thumb_b64.is_empty() && e.thumb_b64.len() <= THUMB_MAX_B64);
    }

    #[test]
    fn big_images_shrink_to_the_limits() {
        let img = noise(2600, 1800);
        let e = encode_for_sync(img.as_raw(), 2600, 1800).unwrap();
        assert!(e.width <= MAX_SIDE && e.height <= MAX_SIDE);
        assert!(e.bytes.len() <= MAX_BYTES, "{} bytes", e.bytes.len());
        assert_eq!(e.mime, "image/jpeg");
        assert!(decode(&e.bytes).is_some());
        assert!(e.thumb_b64.len() <= THUMB_MAX_B64);
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(encode_for_sync(&[0; 3], 1, 1).is_none());
        assert!(encode_for_sync(&[], 0, 0).is_none());
    }

    #[test]
    fn dib_round_trip() {
        let img = gradient(37, 11, 255); // an odd width checks the row padding
        let dib = to_dib(&img);
        assert_eq!(dib.len(), 40 + 112 * 11);
        assert_eq!(from_dib(&dib).unwrap(), img);
    }

    #[test]
    fn transparent_pixels_go_on_white_in_a_dib() {
        let img = gradient(4, 4, 0);
        let back = from_dib(&to_dib(&img)).unwrap();
        assert!(back.pixels().all(|p| p.0 == [255, 255, 255, 255]));
    }

    #[test]
    fn reads_32_bit_dibs_with_empty_alpha() {
        // A 2×1 32-bit BI_RGB bitmap, as many apps put on the clipboard: alpha bytes are 0.
        let mut dib = Vec::new();
        for v in [40u32, 2, 1] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        for v in [0u32, 8, 0, 0, 0, 0] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&[255, 0, 0, 0, 0, 0, 255, 0]); // blue, red (BGRA)
        let img = from_dib(&dib).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn png_round_trip_keeps_alpha() {
        let img = gradient(20, 10, 128);
        assert_eq!(decode(&to_png(&img)).unwrap(), img);
    }

    #[test]
    fn short_dibs_are_refused() {
        assert!(from_dib(&[0; 10]).is_none());
        assert!(from_dib(&[0; 40]).is_none());
    }
}
