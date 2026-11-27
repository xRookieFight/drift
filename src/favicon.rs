//! Favicons arrive as raw pixels, the sidebar wants something it can put in an <img src>.

use std::io::Cursor;

use base64::Engine;
use image::{ImageFormat, RgbaImage};
use servo::{Image, PixelFormat};

/// Encodes a favicon as a PNG data URL. Returns None for empty or malformed images.
pub fn to_data_url(icon: &Image) -> Option<String> {
    let rgba = to_rgba(icon.format, icon.data(), icon.width, icon.height)?;
    let image = RgbaImage::from_raw(icon.width, icon.height, rgba)?;
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// Turns one source pixel into RGBA.
type Convert = fn(&[u8]) -> [u8; 4];

fn to_rgba(format: PixelFormat, data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixels = (width as usize).checked_mul(height as usize)?;
    if pixels == 0 {
        return None;
    }
    let (stride, convert): (usize, Convert) = match format {
        PixelFormat::K8 => (1, |p| [p[0], p[0], p[0], 255]),
        PixelFormat::KA8 => (2, |p| [p[0], p[0], p[0], p[1]]),
        PixelFormat::RGB8 => (3, |p| [p[0], p[1], p[2], 255]),
        PixelFormat::RGBA8 => (4, |p| [p[0], p[1], p[2], p[3]]),
        PixelFormat::BGRA8 => (4, |p| [p[2], p[1], p[0], p[3]]),
    };
    let data = data.get(..pixels * stride)?;
    Some(data.chunks_exact(stride).flat_map(convert).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bgra() {
        let out = to_rgba(PixelFormat::BGRA8, &[1, 2, 3, 4], 1, 1).unwrap();
        assert_eq!(out, vec![3, 2, 1, 4]);
    }

    #[test]
    fn rejects_short_buffers_and_empty_images() {
        assert!(to_rgba(PixelFormat::RGBA8, &[0; 7], 2, 1).is_none());
        assert!(to_rgba(PixelFormat::RGBA8, &[], 0, 0).is_none());
    }
}
