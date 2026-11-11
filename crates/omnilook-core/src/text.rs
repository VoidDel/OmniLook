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
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

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

fn syntax_for<'a>(set: &'a SyntaxSet, path: &Path, text: &str) -> Option<&'a SyntaxReference> {
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
    let language = syntax_for(syntaxes(), path, &text).map(|s| s.name.clone());
    Ok(TextPreview {
        text,
        truncated: len > MAX_BYTES,
        encoding: encoding.to_owned(),
        language,
    })
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
    let set = syntaxes();
    let Some(syntax) = syntax_for(set, path, text) else {
        return Vec::new();
    };
    let start = Instant::now();
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out = Vec::new();
    let mut base = 0u32; // UTF-16 offset of the line
    for line in text.split_inclusive('\n') {
        if start.elapsed() > HIGHLIGHT_BUDGET {
            break;
        }
        let Ok(ops) = state.parse_line(line, set) else {
            break;
        };
        let mut pos = 0usize; // byte offset in the line
        let mut upos = 0u32; // UTF-16 offset in the line
        let mut emit = |from: usize, to: usize, upos: &mut u32, stack: &ScopeStack| {
            let piece = &line[from..to];
            let n = piece.encode_utf16().count() as u32;
            // Line breaks get no class.
            let visible = piece.trim_end_matches(['\n', '\r']);
            let vn = visible.encode_utf16().count() as u32;
            if vn > 0
                && let Some(c) = class_of(stack)
            {
                out.extend_from_slice(&[base + *upos, vn, c as u32]);
            }
            *upos += n;
        };
        for (at, op) in ops {
            if at > pos {
                emit(pos, at, &mut upos, &stack);
                pos = at;
            }
            let _ = stack.apply(&op);
        }
        if pos < line.len() {
            emit(pos, line.len(), &mut upos, &stack);
        }
        base += upos;
    }
    merge(out)
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
