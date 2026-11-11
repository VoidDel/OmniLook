//! Fonts (`docs/development-plan.md` §5.1, M3): what a font file says about itself (names,
//! version, glyph count) and which scripts it covers, read with `ttf-parser`. The page draws the
//! samples with the font itself (loaded through `look://`), and shows only the scripts it has.
//!
//! TrueType and OpenType files; WOFF and WOFF2 are compressed, so only the page reads them (it
//! gets no names from here). Collections (`.ttc`) go to the system's font previewer.

use std::path::Path;

use serde::Serialize;
use ttf_parser::{Face, name_id};

/// Largest font read here (the page still loads bigger ones).
const MAX_BYTES: u64 = 64 << 20;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FontInfo {
    pub family: Option<String>,
    /// The family in English when `family` is in another language.
    pub family_en: Option<String>,
    pub style: Option<String>,
    pub version: Option<String>,
    pub designer: Option<String>,
    pub glyphs: Option<u16>,
    pub monospace: bool,
    /// Scripts it has letters for; all `true` when unknown (WOFF), so the page shows them all.
    pub latin: bool,
    pub cjk: bool,
    pub kana: bool,
    pub hangul: bool,
}

/// Windows name-table language ids for the UI language (`zh-CN`, `ja-JP`…), best first.
fn languages(locale: &str) -> &'static [u16] {
    let l = locale.to_ascii_lowercase();
    if l.starts_with("zh") && (l.contains("tw") || l.contains("hk") || l.contains("hant")) {
        &[0x0404, 0x0C04, 0x0804]
    } else if l.starts_with("zh") {
        &[0x0804, 0x1004, 0x0404]
    } else if l.starts_with("ja") {
        &[0x0411]
    } else {
        &[]
    }
}

const ENGLISH: u16 = 0x0409;

fn name(face: &Face, ids: &[u16], langs: &[u16]) -> Option<String> {
    let names: Vec<_> = face
        .names()
        .into_iter()
        .filter(|n| n.is_unicode())
        .collect();
    for id in ids {
        let of_id: Vec<_> = names.iter().filter(|n| n.name_id == *id).collect();
        let found = langs
            .iter()
            .chain(std::iter::once(&ENGLISH))
            .find_map(|l| of_id.iter().find(|n| n.language_id == *l))
            .or_else(|| of_id.first())
            .and_then(|n| n.to_string())
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        if found.is_some() {
            return found;
        }
    }
    None
}

pub fn is_font(path: &Path) -> bool {
    matches!(
        path.extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .as_deref(),
        Some("ttf" | "otf" | "woff" | "woff2")
    )
}

pub fn read(path: &Path, locale: &str) -> FontInfo {
    let every = FontInfo {
        latin: true,
        cjk: true,
        kana: true,
        hangul: true,
        ..Default::default()
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return every;
    };
    if meta.len() > MAX_BYTES {
        return every;
    }
    let Ok(data) = std::fs::read(path) else {
        return every;
    };
    let Ok(face) = Face::parse(&data, 0) else {
        // WOFF / WOFF2, or not a font this reads.
        return every;
    };
    let langs = languages(locale);
    let family_ids = [name_id::TYPOGRAPHIC_FAMILY, name_id::FAMILY];
    let family = name(&face, &family_ids, langs);
    let family_en = name(&face, &family_ids, &[]).filter(|en| Some(en) != family.as_ref());
    let has = |cs: &str| cs.chars().all(|c| face.glyph_index(c).is_some());
    FontInfo {
        family,
        family_en,
        style: name(
            &face,
            &[name_id::TYPOGRAPHIC_SUBFAMILY, name_id::SUBFAMILY],
            langs,
        ),
        version: name(&face, &[name_id::VERSION], &[]),
        designer: name(&face, &[name_id::DESIGNER, name_id::MANUFACTURER], langs),
        glyphs: Some(face.number_of_glyphs()),
        monospace: face.is_monospaced(),
        latin: has("Aaz"),
        cjk: has("永字"),
        kana: has("あア"),
        hangul: has("한글"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_font_and_not_a_font() {
        // Arial ships with every Windows; elsewhere the test only checks the fallback.
        let arial = Path::new(r"C:\Windows\Fonts\arial.ttf");
        if arial.is_file() {
            let f = read(arial, "en-US");
            assert_eq!(f.family.as_deref(), Some("Arial"));
            assert!(f.latin && !f.cjk && f.glyphs.unwrap_or(0) > 100);
        }
        let d = std::env::temp_dir().join(format!("omnilook-font-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("x.woff2");
        std::fs::write(&p, b"wOF2....").unwrap();
        let f = read(&p, "zh-CN");
        assert!(f.family.is_none() && f.cjk && f.latin);
        assert!(is_font(&p) && !is_font(Path::new("x.ttc")));
    }
}
