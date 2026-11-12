//! SolidWorks and Inventor files: closed formats, shown by the picture the program saved in them.
//!
//! - SolidWorks up to 2014, and Inventor: OLE compound files. SolidWorks keeps a `PreviewPNG`
//!   stream; Inventor a PNG in one of its property sets (the thumbnail property).
//! - SolidWorks 2015 and later: its own container, a run of sections, each `14 00 06 00 08 00`,
//!   five little-endian u32 (two unknown, the compressed and the full size, the name's length), the
//!   name with each byte's halves swapped, and the data, raw deflate. `PreviewPNG` is a PNG;
//!   `Preview` a Windows DIB, used when there is no PNG.

use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use serde::Serialize;

/// What a file is, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeKind {
    SolidWorksPart,
    SolidWorksAssembly,
    SolidWorksDrawing,
    InventorPart,
    InventorAssembly,
    InventorDrawing,
    InventorPresentation,
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn kind_of(path: &Path) -> Option<NativeKind> {
    Some(match ext_of(path).as_str() {
        "sldprt" => NativeKind::SolidWorksPart,
        "sldasm" => NativeKind::SolidWorksAssembly,
        "slddrw" => NativeKind::SolidWorksDrawing,
        "ipt" => NativeKind::InventorPart,
        "iam" => NativeKind::InventorAssembly,
        "idw" => NativeKind::InventorDrawing,
        "ipn" => NativeKind::InventorPresentation,
        _ => return None,
    })
}

const OLE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Whether the first bytes fit the extension: SolidWorks' two containers, Inventor's one (an
/// `.ipt` may be anything else, a chemistry program's input for one).
pub fn handles(path: &Path, head: &[u8]) -> bool {
    match kind_of(path) {
        None => false,
        Some(
            NativeKind::SolidWorksPart
            | NativeKind::SolidWorksAssembly
            | NativeKind::SolidWorksDrawing,
        ) => head.starts_with(OLE) || head.get(4..8) == Some(&[0, 0, 0, 4]),
        Some(_) => head.starts_with(OLE),
    }
}

/// The PNG starting at `b`, up to and with its `IEND` chunk.
fn png_at(b: &[u8]) -> Option<&[u8]> {
    if !b.starts_with(PNG) {
        return None;
    }
    let mut at = PNG.len();
    while at + 12 <= b.len() {
        let len = u32::from_be_bytes(b[at..at + 4].try_into().ok()?) as usize;
        let end = at.checked_add(12)?.checked_add(len)?;
        if end > b.len() {
            return None;
        }
        if &b[at + 4..at + 8] == b"IEND" {
            return Some(&b[..end]);
        }
        at = end;
    }
    None
}

/// Every whole PNG inside `b`.
fn pngs_in(b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = b[from..].windows(PNG.len()).position(|w| w == PNG) {
        let at = from + i;
        match png_at(&b[at..]) {
            Some(p) => {
                out.push(p);
                from = at + p.len();
            }
            None => from = at + 1,
        }
    }
    out
}

/// A Windows DIB (`BITMAPINFOHEADER` onwards) as PNG.
fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 || u32::from_le_bytes(dib[..4].try_into().ok()?) != 40 {
        return None;
    }
    let bpp = u16::from_le_bytes(dib[14..16].try_into().ok()?);
    let colors = u32::from_le_bytes(dib[32..36].try_into().ok()?);
    let palette = if bpp <= 8 {
        4 * if colors == 0 { 1u32 << bpp } else { colors }
    } else {
        0
    };
    // A file header in front, and it is a BMP.
    let offset = 14 + 40 + palette;
    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(14 + dib.len() as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&offset.to_le_bytes());
    bmp.extend_from_slice(dib);
    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()?;
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// SolidWorks 2015 and later: the sections named `…PreviewPNG` (the file's own first, then a
/// configuration's), else `…Preview` as a DIB.
fn solidworks_sections(b: &[u8]) -> Option<Vec<u8>> {
    const MARK: &[u8] = &[0x14, 0x00, 0x06, 0x00, 0x08, 0x00];
    let u32_at =
        |i: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?)) };
    let mut pngs: Vec<(bool, Vec<u8>)> = Vec::new();
    let mut dibs: Vec<Vec<u8>> = Vec::new();
    let mut from = 0;
    while let Some(i) = b[from..].windows(MARK.len()).position(|w| w == MARK) {
        let h = from + i + MARK.len();
        from = from + i + 1;
        let (Some(csize), Some(usize_), Some(len)) =
            (u32_at(h + 8), u32_at(h + 12), u32_at(h + 16))
        else {
            break;
        };
        let (csize, usize_, len) = (csize as usize, usize_ as usize, len as usize);
        if len == 0 || len > 256 || csize == 0 || usize_ > 64 << 20 {
            continue;
        }
        let Some(name) = b.get(h + 20..h + 20 + len) else {
            continue;
        };
        let name: Vec<u8> = name.iter().map(|x| x.rotate_left(4)).collect();
        let Ok(name) = std::str::from_utf8(&name) else {
            continue;
        };
        let wanted_png = name.ends_with("PreviewPNG");
        if !wanted_png && !name.ends_with("Preview") {
            continue;
        }
        let Some(data) = b.get(h + 20 + len..h + 20 + len + csize) else {
            continue;
        };
        let mut out = Vec::with_capacity(usize_);
        if flate2::read::DeflateDecoder::new(data)
            .take(usize_ as u64 + 1)
            .read_to_end(&mut out)
            .is_err()
        {
            continue;
        }
        if wanted_png {
            if let Some(p) = png_at(&out) {
                pngs.push((name == "PreviewPNG", p.to_vec()));
            }
        } else if out.starts_with(&40u32.to_le_bytes()) {
            dibs.push(out);
        }
    }
    // The file's own preview, else the largest.
    pngs.sort_by_key(|(own, p)| (!*own, std::cmp::Reverse(p.len())));
    if let Some((_, p)) = pngs.into_iter().next() {
        return Some(p);
    }
    dibs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    dibs.iter().find_map(|d| dib_to_png(d))
}

/// SolidWorks up to 2014 and Inventor (OLE): a `PreviewPNG` stream, else the largest PNG in any
/// stream (Inventor's thumbnail property), else a `Preview` DIB.
fn ole(path: &Path) -> Option<Vec<u8>> {
    let mut cf = cfb::open(path).ok()?;
    let streams: Vec<(String, std::path::PathBuf, u64)> = cf
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| (e.name().to_owned(), e.path().to_path_buf(), e.len()))
        .collect();
    let mut read = |p: &Path| -> Option<Vec<u8>> {
        let mut s = cf.open_stream(p).ok()?;
        let mut v = Vec::new();
        s.read_to_end(&mut v).ok()?;
        Some(v)
    };
    if let Some((_, p, _)) = streams.iter().find(|(n, _, _)| n == "PreviewPNG")
        && let Some(v) = read(p)
        && let Some(png) = png_at(&v)
    {
        return Some(png.to_vec());
    }
    let mut best: Option<Vec<u8>> = None;
    for (_, p, len) in &streams {
        // Thumbnails are small; the model's own streams can be hundreds of MB.
        if *len > 16 << 20 {
            continue;
        }
        let Some(v) = read(p) else { continue };
        for png in pngs_in(&v) {
            if best.as_ref().is_none_or(|b| png.len() > b.len()) {
                best = Some(png.to_vec());
            }
        }
    }
    if best.is_some() {
        return best;
    }
    let (_, p, _) = streams.iter().find(|(n, _, _)| n == "Preview")?;
    dib_to_png(&read(p)?)
}

/// The picture saved in a SolidWorks or Inventor file, as PNG.
pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut head = [0u8; 8];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| e.to_string())?;
    let png = if head.starts_with(OLE) {
        ole(path)
    } else {
        // Previews can sit anywhere in the file (an assembly's near its end).
        let b = std::fs::read(path).map_err(|e| e.to_string())?;
        solidworks_sections(&b)
    };
    png.ok_or_else(|| "no preview in the file".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1 × 1 PNG.
    fn tiny_png() -> Vec<u8> {
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    #[test]
    fn finds_whole_pngs() {
        let p = tiny_png();
        let mut b = b"junk\x89PNG not one".to_vec();
        b.extend_from_slice(&p);
        b.extend_from_slice(b"tail");
        let found = pngs_in(&b);
        assert_eq!(found, vec![p.as_slice()]);
        // Cut short: none.
        assert!(png_at(&p[..p.len() - 4]).is_none());
    }

    #[test]
    fn reads_solidworks_sections() {
        use std::io::Write;
        let p = tiny_png();
        let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&p).unwrap();
        let data = z.finish().unwrap();
        let name: Vec<u8> = b"PreviewPNG".iter().map(|x| x.rotate_left(4)).collect();
        let mut file = vec![0x72, 0xd0, 0x25, 0x0f, 0, 0, 0, 4, 1, 2, 3];
        file.extend_from_slice(&[0x14, 0x00, 0x06, 0x00, 0x08, 0x00]);
        for v in [
            0u32,
            0,
            data.len() as u32,
            p.len() as u32,
            name.len() as u32,
        ] {
            file.extend_from_slice(&v.to_le_bytes());
        }
        file.extend_from_slice(&name);
        file.extend_from_slice(&data);
        assert_eq!(solidworks_sections(&file), Some(p));
        assert!(handles(Path::new("a.SLDPRT"), &file));
        assert!(!handles(Path::new("input.ipt"), b"!Input"));
        assert!(handles(Path::new("a.ipt"), OLE));
    }

    #[test]
    fn dibs_become_pngs() {
        // A 2 × 1, 24-bit DIB: blue, red (rows padded to 4 bytes).
        let mut dib = Vec::new();
        for v in [40u32, 2, 1] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0; 24]);
        dib.extend_from_slice(&[255, 0, 0, 0, 0, 255, 0, 0]);
        let png = dib_to_png(&dib).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 255]);
        assert_eq!(img.get_pixel(1, 0).0, [255, 0, 0]);
    }

    /// Reads the files listed in `OMNILOOK_THUMBS` (one path per line).
    #[test]
    #[ignore]
    fn real_files() {
        let Ok(list) = std::env::var("OMNILOOK_THUMBS") else {
            return;
        };
        let list = std::fs::read_to_string(&list).unwrap_or(list);
        for path in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let t = std::time::Instant::now();
            match read(Path::new(path)) {
                Ok(png) => {
                    let img = image::load_from_memory(&png).map(|i| (i.width(), i.height()));
                    println!("ok {:>5}ms {:?} {path}", t.elapsed().as_millis(), img);
                }
                Err(e) => println!("ERR {path}: {e}"),
            }
        }
    }
}
