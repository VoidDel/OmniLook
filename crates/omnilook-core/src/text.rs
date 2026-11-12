//! Text and code (`docs/development-plan.md` §5.1): the encoding is detected (byte order marks,
//! UTF-8, else a legacy code page such as GBK or Shift_JIS), and code is split into token classes
//! by `syntect` with bat's syntax set (`two-face`). The page gets plain text plus class ranges and
//! renders them as text, so nothing in the file can turn into markup.

use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

use serde::Serialize;
use syntect::parsing::{
    ParseState, Scope, ScopeStack, SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder,
};

use crate::records;

/// Bytes read for a text preview.
pub const MAX_BYTES: u64 = 4 << 20;
/// Text longer than this is not highlighted (it would take too long to be worth waiting for).
pub const MAX_HIGHLIGHT: usize = 512 << 10;
/// Highlighting stops after this long; what is done is kept.
const HIGHLIGHT_BUDGET: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    pub text: String,
    /// Only the start of the file was read.
    pub truncated: bool,
    pub encoding: String,
    /// The syntax found for the file (its display name), if any but plain text.
    pub language: Option<String>,
    /// Firmware records (Intel HEX, S-records): what the whole file holds, and its bad records.
    /// Only from [`preview`].
    pub records: Option<records::Summary>,
}

fn decode(bytes: &[u8]) -> (String, &'static str) {
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        let (s, _, _) = enc.decode(&bytes[bom..]);
        return (s.into_owned(), enc.name());
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), "UTF-8"),
        // A character cut by the read limit is still UTF-8.
        Err(e) if e.error_len().is_none() => (
            String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned(),
            "UTF-8",
        ),
        Err(_) => {
            let mut d = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
            d.feed(bytes, true);
            let enc = d.guess(None, chardetng::Utf8Detection::Allow);
            let (s, _, _) = enc.decode(bytes);
            (s.into_owned(), enc.name())
        }
    }
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// Syntaxes bat lacks, kept in a set of their own (adding them to bat's would link its few hundred
/// syntaxes again at every start): GNU ld linker scripts and IAR ILINK configuration files.
fn own_syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut b = SyntaxSetBuilder::new();
        for src in [
            include_str!("../syntaxes/linker-script.sublime-syntax"),
            include_str!("../syntaxes/iar-icf.sublime-syntax"),
        ] {
            match SyntaxDefinition::load_from_str(src, true, None) {
                Ok(d) => b.add(d),
                Err(e) => debug_assert!(false, "{e}"),
            }
        }
        b.build()
    })
}

/// The syntax for  and the set it belongs to: ours first, then bat's.
fn syntax_for(path: &Path, text: &str) -> Option<(&'static SyntaxSet, &'static SyntaxReference)> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let own = own_syntaxes();
    if let Some(s) = own.find_syntax_by_extension(&ext) {
        return Some((own, s));
    }
    let set = syntaxes();
    syntax_in(set, path, text).map(|s| (set, s))
}

/// The syntax for `lang` (a code fence's language) and its set.
fn syntax_by_token(lang: &str) -> Option<(&'static SyntaxSet, &'static SyntaxReference)> {
    let lower = lang.to_ascii_lowercase();
    [own_syntaxes(), syntaxes()].into_iter().find_map(|set| {
        set.find_syntax_by_token(lang)
            .or_else(|| set.find_syntax_by_token(&lower))
            .filter(|s| s.name != "Plain Text")
            .map(|s| (set, s))
    })
}

fn syntax_in<'a>(set: &'a SyntaxSet, path: &Path, text: &str) -> Option<&'a SyntaxReference> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    set.find_syntax_by_extension(&name)
        .or_else(|| set.find_syntax_by_extension(&name.to_ascii_lowercase()))
        .or_else(|| set.find_syntax_by_extension(&ext))
        .or_else(|| set.find_syntax_by_extension(&ext.to_ascii_lowercase()))
        .or_else(|| set.find_syntax_by_first_line(text.lines().next().unwrap_or_default()))
        .filter(|s| s.name != "Plain Text")
}

pub fn read(path: &Path) -> std::io::Result<TextPreview> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut buf = Vec::with_capacity(len.min(MAX_BYTES) as usize);
    f.by_ref().take(MAX_BYTES).read_to_end(&mut buf)?;
    let (text, encoding) = decode(&buf);
    let language = match records::detect(path, &text) {
        Some(f) => Some(f.name().to_owned()),
        None => syntax_for(path, &text).map(|(_, s)| s.name.clone()),
    };
    Ok(TextPreview {
        text,
        truncated: len > MAX_BYTES,
        encoding: encoding.to_owned(),
        language,
        records: None,
    })
}

/// [`read`], plus the summary of firmware records, which reads the whole file.
pub fn preview(path: &Path) -> std::io::Result<TextPreview> {
    let mut t = read(path)?;
    if let Some(format) = records::detect(path, &t.text) {
        t.records = records::scan(path, format).ok();
    }
    Ok(t)
}

/// Token classes the page colours (`.tk-<letter>` in its styles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Class {
    Comment = 1,
    String = 2,
    Number = 3,
    Keyword = 4,
    Function = 5,
    Type = 6,
    Constant = 7,
    Tag = 8,
    Attribute = 9,
    Heading = 10,
}

/// Scope prefixes and their class, most specific first.
fn classes() -> &'static [(Scope, Class)] {
    static TABLE: OnceLock<Vec<(Scope, Class)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        [
            ("comment", Class::Comment),
            ("string", Class::String),
            ("constant.character.escape", Class::String),
            ("constant.numeric", Class::Number),
            ("constant", Class::Constant),
            ("keyword", Class::Keyword),
            ("storage", Class::Keyword),
            ("meta.preprocessor", Class::Keyword),
            ("entity.name.function", Class::Function),
            ("support.function", Class::Function),
            ("variable.function", Class::Function),
            ("entity.name.type", Class::Type),
            ("entity.name.class", Class::Type),
            ("entity.name.struct", Class::Type),
            ("entity.name.enum", Class::Type),
            ("entity.other.inherited-class", Class::Type),
            ("support.type", Class::Type),
            ("support.class", Class::Type),
            ("entity.name.tag", Class::Tag),
            ("entity.other.attribute-name", Class::Attribute),
            ("support.type.property-name", Class::Attribute),
            ("markup.heading", Class::Heading),
            ("entity.name.section", Class::Heading),
        ]
        .into_iter()
        .filter_map(|(s, c)| Scope::new(s).ok().map(|s| (s, c)))
        .collect()
    })
}

/// The class of the innermost scope that has one.
fn class_of(stack: &ScopeStack) -> Option<Class> {
    for scope in stack.as_slice().iter().rev() {
        for (prefix, class) in classes() {
            if prefix.is_prefix_of(*scope) {
                return Some(*class);
            }
        }
    }
    None
}

/// Class ranges of `text` as flat `[start, length, class]` triples, in UTF-16 code units (the
/// page's string indices). Ranges never cross a line break. Empty when the text is too long or no
/// syntax is known; partial when the time budget runs out.
pub fn highlight(path: &Path, text: &str) -> Vec<u32> {
    if text.len() > MAX_HIGHLIGHT {
        return Vec::new();
    }
    // Firmware records are coloured by the page, line by line.
    if records::detect(path, text).is_some() {
        return Vec::new();
    }
    let Some((set, syntax)) = syntax_for(path, text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut at = 0u32; // UTF-16 offset of the piece
    walk(set, syntax, text, |piece, class| {
        let n = piece.encode_utf16().count() as u32;
        // Line breaks get no class.
        let vn = piece.trim_end_matches(['\n', '\r']).encode_utf16().count() as u32;
        if vn > 0
            && let Some(c) = class
        {
            out.extend_from_slice(&[at, vn, c as u32]);
        }
        at += n;
    });
    merge(out)
}

/// Hands `text` to `f` piece by piece with each piece's class, line by line, until the time budget
/// runs out. Returns how many bytes were covered.
fn walk(
    set: &SyntaxSet,
    syntax: &SyntaxReference,
    text: &str,
    mut f: impl FnMut(&str, Option<Class>),
) -> usize {
    let start = Instant::now();
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut covered = 0;
    for line in text.split_inclusive('\n') {
        if start.elapsed() > HIGHLIGHT_BUDGET {
            break;
        }
        let Ok(ops) = state.parse_line(line, set) else {
            break;
        };
        let mut pos = 0usize; // byte offset in the line
        for (at, op) in ops {
            if at > pos {
                f(&line[pos..at], class_of(&stack));
                pos = at;
            }
            let _ = stack.apply(&op);
        }
        if pos < line.len() {
            f(&line[pos..], class_of(&stack));
        }
        covered += line.len();
    }
    covered
}

fn escape_html(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// `code` as escaped HTML with `<span class="tkN">` around each coloured piece, for a code block
/// whose fence names `lang` (`rust`, `ts`, `toml`…). Plain escaped text when the language is not
/// known or the code is too long.
pub fn highlight_html(lang: &str, code: &str) -> String {
    let mut out = String::with_capacity(code.len() + code.len() / 2);
    let found = (!lang.is_empty() && code.len() <= MAX_HIGHLIGHT)
        .then(|| syntax_by_token(lang))
        .flatten();
    let Some((set, syntax)) = found else {
        escape_html(code, &mut out);
        return out;
    };
    let covered = walk(set, syntax, code, |piece, class| match class {
        Some(c) if !piece.trim().is_empty() => {
            out.push_str(&format!("<span class=\"tk{}\">", c as u8));
            escape_html(piece, &mut out);
            out.push_str("</span>");
        }
        _ => escape_html(piece, &mut out),
    });
    // The budget ran out: the rest uncoloured.
    escape_html(&code[covered..], &mut out);
    out
}

/// Joins neighbouring ranges of the same class.
fn merge(flat: Vec<u32>) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(flat.len());
    for t in flat.as_chunks::<3>().0 {
        let n = out.len();
        if n >= 3 && out[n - 1] == t[2] && out[n - 3] + out[n - 2] == t[0] {
            out[n - 2] += t[1];
        } else {
            out.extend_from_slice(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_legacy_and_bom() {
        let gbk = encoding_rs::GBK
            .encode("这是一段很长的中文说明文字，用来测试编码识别是否正确。")
            .0
            .into_owned();
        let (s, enc) = decode(&gbk);
        assert!(s.starts_with("这是"), "{enc}");
        let (s, enc) = decode(b"\xFF\xFEh\0i\0");
        assert_eq!((s.as_str(), enc), ("hi", "UTF-16LE"));
        // A UTF-8 character cut in the middle by the read limit.
        let (s, _) = decode(&"中文".as_bytes()[..5]);
        assert_eq!(s, "中");
    }

    fn ranges(path: &str, text: &str) -> Vec<(String, u32)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        highlight(Path::new(path), text)
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let s = String::from_utf16_lossy(&units[t[0] as usize..(t[0] + t[1]) as usize]);
                (s, t[2])
            })
            .collect()
    }

    #[test]
    fn highlights_by_extension() {
        let r = ranges("main.rs", "// 注释\nfn main() { let s = \"字\"; }\n");
        assert!(
            r.contains(&("// 注释".into(), Class::Comment as u32)),
            "{r:?}"
        );
        assert!(r.contains(&("fn".into(), Class::Keyword as u32)), "{r:?}");
        assert!(
            r.iter()
                .any(|(s, c)| s.contains('字') && *c == Class::String as u32),
            "{r:?}"
        );
        // Languages bat adds to syntect's own set.
        assert!(!ranges("a.ts", "const x: number = 1;").is_empty());
        assert!(!ranges("Cargo.toml", "[package]\nname = \"x\"\n").is_empty());
        // No syntax, no ranges.
        assert!(ranges("notes.txt", "plain words").is_empty());
    }

    #[test]
    fn linker_scripts() {
        let ld = "/* Entry */\nENTRY(Reset_Handler)\n_estack = ORIGIN(RAM) + LENGTH(RAM);\n\
                  MEMORY\n{\n  FLASH (rx) : ORIGIN = 0x8000000, LENGTH = 2048K\n}\n\
                  SECTIONS\n{\n  .text :\n  {\n    KEEP(*(.isr_vector))\n  } >FLASH\n}\n";
        let r = ranges("STM32H743XX_FLASH.ld", ld);
        for (piece, class) in [
            ("/* Entry */", Class::Comment),
            ("ENTRY", Class::Keyword),
            ("ORIGIN", Class::Function),
            ("SECTIONS", Class::Keyword),
            ("0x8000000", Class::Number),
            ("2048K", Class::Number),
            (".text", Class::Type),
            (".isr_vector", Class::Type),
            ("FLASH", Class::Attribute),
        ] {
            assert!(r.contains(&(piece.into(), class as u32)), "{piece}: {r:?}");
        }
        let icf = "define symbol __ICFEDIT_intvec_start__ = 0x08000000;\n\
                   place in ROM_region   { readonly };\n// done\n";
        let r = ranges("stm32g431xx_flash.icf", icf);
        for (piece, class) in [
            ("define", Class::Keyword),
            ("0x08000000", Class::Number),
            ("readonly", Class::Keyword),
            ("// done", Class::Comment),
        ] {
            assert!(r.contains(&(piece.into(), class as u32)), "{piece}: {r:?}");
        }
        assert_eq!(
            syntax_by_token("ld").map(|(_, s)| s.name.as_str()),
            Some("Linker Script")
        );
    }

    #[test]
    fn firmware_records_are_left_to_the_page() {
        let dir = std::env::temp_dir().join(format!("omnilook-records-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("app.hex");
        std::fs::write(
            &file,
            ":0400100010111213A6\n:0400100010111213A7\n:00000001FF\n",
        )
        .unwrap();
        let t = preview(&file).unwrap();
        assert_eq!(t.language.as_deref(), Some("Intel HEX"));
        let s = t.records.unwrap();
        assert_eq!((s.records, s.bad_count, s.data_bytes), (2, 1, 4));
        assert!(highlight(&file, &t.text).is_empty());
        // read() leaves the summary out.
        assert!(read(&file).unwrap().records.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ranges_count_utf16_units() {
        // An emoji is two UTF-16 units: the comment after it starts at 4, not 3.
        let r = highlight(Path::new("a.py"), "x=\"😀\"#c\n");
        let comment = r
            .as_chunks::<3>()
            .0
            .iter()
            .find(|t| t[2] == Class::Comment as u32)
            .unwrap();
        assert_eq!(comment[0], 6, "{r:?}");
    }
}
