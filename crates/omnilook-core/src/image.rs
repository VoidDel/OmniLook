//! Images the WebView cannot show itself (TIFF): decoded here and handed over as PNG. Formats it
//! shows (PNG, JPEG, GIF, WebP, BMP, ICO, AVIF, SVG as an image) go to it directly; it decodes
//! them in its sandboxed renderer, as it does PDF and media (`docs/m0-results.md` §2.6).

use std::{io::Cursor, path::Path};

/// Larger images are scaled down to this side first.
pub const MAX_SIDE: u32 = 4096;
/// Images claiming more than this per side are refused before decoding.
const LIMIT_SIDE: u32 = 32_768;

/// Whether the WebView needs the image converted.
pub fn needs_conversion(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| e == "tif" || e == "tiff")
}

pub fn to_png(path: &Path) -> Result<Vec<u8>, String> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(LIMIT_SIDE);
    limits.max_image_height = Some(LIMIT_SIDE);
    limits.max_alloc = Some(1 << 30);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img.thumbnail(MAX_SIDE, MAX_SIDE)
    } else {
        img
    };
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png)
}
