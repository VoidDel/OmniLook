//! Word, Excel and PowerPoint files without Office (`docs/development-plan.md` §5.1, M3): their
//! text read from the XML inside the ZIP. Word: paragraphs, headings, list items, bold and
//! italic, tables. Excel: the first rows and columns of each sheet (values as stored: dates stay
//! numbers). PowerPoint: the text of each slide. Nothing is laid out as Office would; it is to
//! see what is in the file.
//!
//! The page gets data, not markup: nothing in the file can turn into HTML.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use roxmltree::{Document, Node};
use serde::Serialize;

/// Largest XML part read (uncompressed): a bigger one is a bomb or not worth waiting for.
const MAX_PART: u64 = 32 << 20;
const MAX_BLOCKS: usize = 5000;
const MAX_ROWS: usize = 200;
const MAX_COLS: usize = 30;
const MAX_SHEETS: usize = 12;
const MAX_SLIDES: usize = 300;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    /// `level` 0 is body text, 1–6 headings.
    Para {
        level: u8,
        list: bool,
        runs: Vec<Run>,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    /// More rows or columns than shown.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OfficePreview {
    Word {
        blocks: Vec<Block>,
        truncated: bool,
    },
    Excel {
        sheets: Vec<Sheet>,
    },
    PowerPoint {
        slides: Vec<Vec<String>>,
        truncated: bool,
    },
}

/// The extensions read here.
pub fn handles(path: &Path) -> bool {
    matches!(
        ext(path).as_str(),
        "docx" | "docm" | "dotx" | "xlsx" | "xlsm" | "xltx" | "pptx" | "pptm" | "ppsx"
    )
}

fn ext(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

type Zip = zip::ZipArchive<BufReader<File>>;

fn part(z: &mut Zip, name: &str) -> Result<Option<String>, String> {
    let mut f = match z.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if f.size() > MAX_PART {
        return Err(format!("{name} is too large"));
    }
    let mut s = String::with_capacity(f.size() as usize);
    f.by_ref()
        .take(MAX_PART)
        .read_to_string(&mut s)
        .map_err(|e| e.to_string())?;
    Ok(Some(s))
}

fn need(z: &mut Zip, name: &str) -> Result<String, String> {
    part(z, name)?.ok_or_else(|| format!("no {name}"))
}

fn parse(xml: &str) -> Result<Document<'_>, String> {
    // DTDs are refused by default: no entity expansion.
    Document::parse(xml).map_err(|e| e.to_string())
}

fn is(n: &Node, ns: &str, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(ns)
}

fn child<'a, 'i>(n: &Node<'a, 'i>, ns: &str, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is(c, ns, name))
}

/// A `w:b` / `w:i` that is on (absent `w:val`, or one that is not off).
fn toggle(n: Option<Node>) -> bool {
    n.is_some_and(|n| !matches!(n.attribute((W, "val")), Some("0" | "false" | "off")))
}

/// Relationship ids of a part's `.rels` and the parts they name, relative to `dir`.
fn rels(z: &mut Zip, rels_name: &str, dir: &str) -> Result<HashMap<String, String>, String> {
    let Some(xml) = part(z, rels_name)? else {
        return Ok(HashMap::new());
    };
    let doc = parse(&xml)?;
    Ok(doc
        .descendants()
        .filter(|n| is(n, PKG_REL, "Relationship"))
        .filter_map(|n| {
            let target = n.attribute("Target")?;
            let full = match target.strip_prefix('/') {
                Some(abs) => abs.to_owned(),
                None => format!("{dir}{target}"),
            };
            Some((n.attribute("Id")?.to_owned(), full))
        })
        .collect())
}

// --- Word ------------------------------------------------------------------------------------

fn runs_of(p: Node) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for r in p.descendants().filter(|n| is(n, W, "r")) {
        let props = child(&r, W, "rPr");
        let bold = toggle(props.and_then(|p| child(&p, W, "b")));
        let italic = toggle(props.and_then(|p| child(&p, W, "i")));
        let mut text = String::new();
        for c in r.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "t" => text.push_str(c.text().unwrap_or_default()),
                "tab" => text.push('\t'),
                "br" | "cr" => text.push('\n'),
                _ => {}
            }
        }
        if text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.bold == bold && last.italic == italic => last.text.push_str(&text),
            _ => out.push(Run { text, bold, italic }),
        }
    }
    out
}

fn para(p: Node) -> Block {
    let props = child(&p, W, "pPr");
    let style = props
        .and_then(|pp| child(&pp, W, "pStyle"))
        .and_then(|s| s.attribute((W, "val")))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let level = if style == "title" {
        1
    } else {
        style
            .strip_prefix("heading")
            .and_then(|n| n.trim().parse::<u8>().ok())
            .filter(|n| (1..=6).contains(n))
            .unwrap_or(0)
    };
    let list = props.is_some_and(|pp| child(&pp, W, "numPr").is_some());
    Block::Para {
        level,
        list,
        runs: runs_of(p),
    }
}

fn cell_text(tc: Node) -> String {
    tc.children()
        .filter(|n| is(n, W, "p"))
        .map(|p| runs_of(p).into_iter().map(|r| r.text).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn blocks(parent: Node, out: &mut Vec<Block>) {
    for n in parent.children().filter(|n| n.is_element()) {
        if out.len() >= MAX_BLOCKS {
            return;
        }
        if is(&n, W, "p") {
            out.push(para(n));
        } else if is(&n, W, "tbl") {
            let rows = n
                .children()
                .filter(|r| is(r, W, "tr"))
                .take(MAX_ROWS)
                .map(|tr| {
                    tr.children()
                        .filter(|c| is(c, W, "tc"))
                        .map(cell_text)
                        .collect()
                })
                .collect();
            out.push(Block::Table { rows });
        } else if is(&n, W, "sdt")
            && let Some(content) = child(&n, W, "sdtContent")
        {
            blocks(content, out);
        }
    }
}

fn word(z: &mut Zip) -> Result<OfficePreview, String> {
    let xml = need(z, "word/document.xml")?;
    let doc = parse(&xml)?;
    let body = doc
        .descendants()
        .find(|n| is(n, W, "body"))
        .ok_or("no document body")?;
    let mut out = Vec::new();
    blocks(body, &mut out);
    // Empty paragraphs between others are spacing in Word; one is enough here.
    out.dedup_by(|b, a| {
        matches!((a, b), (Block::Para { runs: x, .. }, Block::Para { runs: y, .. }) if x.is_empty() && y.is_empty())
    });
    Ok(OfficePreview::Word {
        truncated: out.len() >= MAX_BLOCKS,
        blocks: out,
    })
}

// --- Excel -----------------------------------------------------------------------------------

/// `B12` → column 1 (zero based).
fn column(reference: &str) -> Option<usize> {
    let letters = reference.bytes().take_while(u8::is_ascii_alphabetic);
    let mut n = 0usize;
    let mut any = false;
    for b in letters {
        any = true;
        n = n * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize;
    }
    any.then(|| n - 1)
}

fn all_text(n: Node) -> String {
    n.descendants()
        .filter(|d| d.is_element() && d.tag_name().name() == "t")
        .filter_map(|t| t.text())
        .collect()
}

fn excel(z: &mut Zip) -> Result<OfficePreview, String> {
    let shared: Vec<String> = match part(z, "xl/sharedStrings.xml")? {
        Some(xml) => {
            let doc = parse(&xml)?;
            doc.descendants()
                .filter(|n| is(n, S, "si"))
                .map(all_text)
                .collect()
        }
        None => Vec::new(),
    };
    let book = need(z, "xl/workbook.xml")?;
    let book = parse(&book)?;
    let targets = rels(z, "xl/_rels/workbook.xml.rels", "xl/")?;
    let named: Vec<(String, String)> = book
        .descendants()
        .filter(|n| is(n, S, "sheet"))
        .filter_map(|n| {
            let id = n.attribute((R, "id"))?;
            Some((n.attribute("name")?.to_owned(), targets.get(id)?.clone()))
        })
        .collect();
    let mut sheets = Vec::new();
    for (name, target) in named.into_iter().take(MAX_SHEETS) {
        let Some(xml) = part(z, &target)? else {
            continue;
        };
        let doc = parse(&xml)?;
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut truncated = false;
        for row in doc.descendants().filter(|n| is(n, S, "row")) {
            // Rows by their number: empty ones in between are kept, so line numbers match.
            let at = row
                .attribute("r")
                .and_then(|r| r.parse::<usize>().ok())
                .map_or(rows.len(), |r| r.saturating_sub(1));
            if at >= MAX_ROWS {
                truncated = true;
                break;
            }
            while rows.len() < at {
                rows.push(Vec::new());
            }
            let mut cells: Vec<String> = Vec::new();
            for (i, c) in row.children().filter(|c| is(c, S, "c")).enumerate() {
                let col = c.attribute("r").and_then(column).unwrap_or(i);
                if col >= MAX_COLS {
                    truncated = true;
                    continue;
                }
                let v = child(&c, S, "v").and_then(|v| v.text()).unwrap_or_default();
                let value = match c.attribute("t") {
                    Some("s") => v
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| shared.get(i).cloned())
                        .unwrap_or_default(),
                    Some("inlineStr") => child(&c, S, "is").map(all_text).unwrap_or_default(),
                    Some("b") => if v == "1" { "TRUE" } else { "FALSE" }.to_owned(),
                    _ => v.to_owned(),
                };
                while cells.len() < col {
                    cells.push(String::new());
                }
                cells.push(value);
            }
            rows.push(cells);
        }
        sheets.push(Sheet {
            name,
            rows,
            truncated,
        });
    }
    Ok(OfficePreview::Excel { sheets })
}

// --- PowerPoint ------------------------------------------------------------------------------

fn powerpoint(z: &mut Zip) -> Result<OfficePreview, String> {
    let pres = need(z, "ppt/presentation.xml")?;
    let pres = parse(&pres)?;
    let targets = rels(z, "ppt/_rels/presentation.xml.rels", "ppt/")?;
    let order: Vec<String> = pres
        .descendants()
        .filter(|n| is(n, P, "sldId"))
        .filter_map(|n| targets.get(n.attribute((R, "id"))?).cloned())
        .collect();
    let truncated = order.len() > MAX_SLIDES;
    let mut slides = Vec::new();
    for target in order.into_iter().take(MAX_SLIDES) {
        let Some(xml) = part(z, &target)? else {
            continue;
        };
        let doc = parse(&xml)?;
        let paras: Vec<String> = doc
            .descendants()
            .filter(|n| is(n, A, "p"))
            .map(|p| {
                p.descendants()
                    .filter(|t| is(t, A, "t"))
                    .filter_map(|t| t.text())
                    .collect::<String>()
            })
            .filter(|s| !s.trim().is_empty())
            .collect();
        slides.push(paras);
    }
    Ok(OfficePreview::PowerPoint { slides, truncated })
}

pub fn read(path: &Path) -> Result<OfficePreview, String> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(BufReader::new(f)).map_err(|e| e.to_string())?;
    match ext(path).as_str() {
        "xlsx" | "xlsm" | "xltx" => excel(&mut z),
        "pptx" | "pptm" | "ppsx" => powerpoint(&mut z),
        _ => word(&mut z),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn make(name: &str, parts: &[(&str, &str)]) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("omnilook-office-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        for (n, body) in parts {
            z.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        p
    }

    #[test]
    fn word_paragraphs_headings_lists_tables() {
        let doc = format!(
            r#"<w:document xmlns:w="{W}"><w:body>
              <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p>
              <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>粗</w:t></w:r><w:r><w:t xml:space="preserve"> 正常</w:t></w:r></w:p>
              <w:p/><w:p/>
              <w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>项</w:t></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            </w:body></w:document>"#
        );
        let p = make("a.docx", &[("word/document.xml", &doc)]);
        let OfficePreview::Word { blocks, .. } = read(&p).unwrap() else {
            panic!("not word")
        };
        assert_eq!(
            blocks[0],
            Block::Para {
                level: 1,
                list: false,
                runs: vec![Run {
                    text: "标题".into(),
                    bold: false,
                    italic: false
                }]
            }
        );
        let Block::Para { runs, .. } = &blocks[1] else {
            panic!()
        };
        assert!(runs[0].bold && runs[1].text == " 正常" && !runs[1].bold);
        // Two empty paragraphs became one.
        assert!(matches!(&blocks[2], Block::Para { runs, .. } if runs.is_empty()));
        assert!(matches!(&blocks[3], Block::Para { list: true, .. }));
        assert_eq!(
            blocks[4],
            Block::Table {
                rows: vec![vec!["a".into(), "b".into()]]
            }
        );
    }

    #[test]
    fn excel_shared_strings_and_gaps() {
        let book = format!(
            r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets><sheet name="数据" sheetId="1" r:id="rId1"/></sheets></workbook>"#
        );
        let rels = format!(
            r#"<Relationships xmlns="{PKG_REL}"><Relationship Id="rId1" Target="worksheets/sheet1.xml" Type="x"/></Relationships>"#
        );
        let strings = format!(
            r#"<sst xmlns="{S}"><si><t>名称</t></si><si><r><t>富</t></r><r><t>文本</t></r></si></sst>"#
        );
        let sheet = format!(
            r#"<worksheet xmlns="{S}"><sheetData>
              <row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1"><v>3.5</v></c></row>
              <row r="3"><c r="B3" t="s"><v>1</v></c><c r="C3" t="b"><v>1</v></c></row>
            </sheetData></worksheet>"#
        );
        let p = make(
            "b.xlsx",
            &[
                ("xl/workbook.xml", &book),
                ("xl/_rels/workbook.xml.rels", &rels),
                ("xl/sharedStrings.xml", &strings),
                ("xl/worksheets/sheet1.xml", &sheet),
            ],
        );
        let OfficePreview::Excel { sheets } = read(&p).unwrap() else {
            panic!("not excel")
        };
        assert_eq!(sheets[0].name, "数据");
        assert_eq!(
            sheets[0].rows,
            vec![
                vec!["名称".to_owned(), String::new(), "3.5".into()],
                vec![],
                vec![String::new(), "富文本".into(), "TRUE".into()],
            ]
        );
        assert_eq!(column("AA10"), Some(26));
    }

    #[test]
    fn powerpoint_slides_in_order() {
        let pres = format!(
            r#"<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="2" r:id="rB"/><p:sldId id="1" r:id="rA"/></p:sldIdLst></p:presentation>"#
        );
        let rels = format!(
            r#"<Relationships xmlns="{PKG_REL}"><Relationship Id="rA" Target="slides/slide1.xml"/><Relationship Id="rB" Target="slides/slide2.xml"/></Relationships>"#
        );
        let slide = |t: &str| {
            format!(
                r#"<p:sld xmlns:p="{P}" xmlns:a="{A}"><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#
            )
        };
        let p = make(
            "c.pptx",
            &[
                ("ppt/presentation.xml", &pres),
                ("ppt/_rels/presentation.xml.rels", &rels),
                ("ppt/slides/slide1.xml", &slide("一")),
                ("ppt/slides/slide2.xml", &slide("二")),
            ],
        );
        let OfficePreview::PowerPoint { slides, .. } = read(&p).unwrap() else {
            panic!("not powerpoint")
        };
        // The presentation's order, not the file names'.
        assert_eq!(slides, vec![vec!["二".to_owned()], vec!["一".to_owned()]]);
    }

    #[test]
    fn not_a_package() {
        let d = std::env::temp_dir().join(format!("omnilook-office-bad-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("x.docx");
        std::fs::write(&p, b"PK\x03\x04 broken").unwrap();
        assert!(read(&p).is_err());
        assert!(handles(&p) && !handles(Path::new("x.doc")));
    }
}
