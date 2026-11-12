//! Which preview a file gets (`docs/development-plan.md` §5.1): by extension first, then by its
//! first bytes when the extension says nothing or lies (a renamed archive, an extension-less
//! text file).

use std::{fs::File, io::Read, path::Path};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Text,
    /// Rendered (with a source view).
    Markdown,
    Image,
    Svg,
    Pdf,
    Video,
    Audio,
    Archive,
    Folder,
    /// A type the system has a preview handler for (Office and the like).
    Handler,
    /// Word, Excel or PowerPoint read without Office (`office`), when no handler shows it.
    Office,
    /// TrueType, OpenType, WOFF: samples drawn with the font (`font`).
    Font,
    /// DWG, DXF: the drawing (`cad`).
    Cad,
    /// 3D models: meshes (STL, OBJ, glTF…) and CAD solids (STEP, BREP), read and drawn by the
    /// page (three.js, Open CASCADE in WebAssembly).
    Model,
    /// SolidWorks and Inventor files: the picture saved in them (`thumbnail`).
    Native,
    Binary,
}

impl Kind {
    /// Previews that need the GPU: video (H.265 has no software decoder), CAD drawings
    /// (millions of lines, drawn with WebGL) and 3D models.
    pub fn wants_gpu(self) -> bool {
        matches!(self, Kind::Video | Kind::Cad | Kind::Model)
    }
}

/// 3D model formats known by their extension alone.
const MODEL: &[&str] = &[
    "stl", "ply", "gltf", "glb", "3mf", "fbx", "dae", "3ds", "amf", "brep",
];

/// Extensions shared with other files, a model only when the content says so: `.obj` is also a
/// compiler's object file, `.stp` a SystemTap script.
fn model_by_content(ext: &str, head: &[u8]) -> bool {
    match ext {
        // Wavefront OBJ: text with vertex, face or object lines.
        "obj" => {
            looks_like_text(head)
                && head.split(|&b| b == b'\n').any(|l| {
                    let l = l.trim_ascii_start();
                    ["v ", "vn ", "vt ", "f ", "o ", "g ", "mtllib "]
                        .iter()
                        .any(|p| l.starts_with(p.as_bytes()))
                })
        }
        // STEP (ISO 10303-21).
        "step" | "stp" => head.trim_ascii_start().starts_with(b"ISO-10303-21"),
        _ => false,
    }
}

const TEXT: &[&str] = &[
    "txt",
    "rst",
    "log",
    "csv",
    "tsv",
    "ini",
    "cfg",
    "conf",
    "toml",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "json5",
    "xml",
    "html",
    "htm",
    "xhtml",
    "css",
    "scss",
    "sass",
    "less",
    "js",
    "mjs",
    "cjs",
    "ts",
    "mts",
    "cts",
    "tsx",
    "jsx",
    "vue",
    "svelte",
    "rs",
    "go",
    "py",
    "pyw",
    "rb",
    "java",
    "kt",
    "kts",
    "scala",
    "swift",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "hh",
    "hxx",
    "cs",
    "fs",
    "vb",
    "php",
    "pl",
    "pm",
    "lua",
    "r",
    "dart",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "psm1",
    "bat",
    "cmd",
    "sql",
    "gradle",
    "properties",
    "env",
    "gitignore",
    "gitattributes",
    "editorconfig",
    "dockerfile",
    "makefile",
    "cmake",
    "srt",
    "ass",
    "vtt",
    "tex",
    "bib",
    "lock",
    "license",
    "readme",
    "ftl",
    "proto",
    "graphql",
    "gql",
    "nim",
    "zig",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "clj",
    "lisp",
    "el",
    "vim",
    "diff",
    "patch",
    "nsi",
    "nsh",
    "reg",
    "inf",
    "manifest",
    "plist",
    "sln",
    "csproj",
    "vcxproj",
    "props",
    "targets",
];
const MARKDOWN: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn"];
const IMAGE: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "gif", "webp", "bmp", "ico", "tif", "tiff", "avif", "heic",
    "heif", "hif",
];
const VIDEO: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "ogv", "ts", "m2ts", "mts", "3gp", "flv",
];
const AUDIO: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "oga", "opus", "m4a", "aac", "wma",
];
const ARCHIVE: &[&str] = &[
    "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "zst", "tzst",
    "lz4", "lzma", "cab", "iso", "lzh", "lha", "arj", "cpio", "rpm", "deb", "ar", "xar", "pkg",
    "jar", "war", "apk", "xpi", "crx", "vsix", "nupkg", "whl", "epub",
];
const HANDLER: &[&str] = &[
    "doc", "docx", "docm", "dot", "dotx", "rtf", "xls", "xlsx", "xlsm", "xlsb", "ppt", "pptx",
    "pptm", "pps", "ppsx", "vsd", "vsdx", "msg", "eml", "ttc", "fon",
];

fn ext_of(path: &Path) -> String {
    match path.extension() {
        Some(e) => e.to_string_lossy().to_ascii_lowercase(),
        // README, Makefile, LICENSE, Dockerfile.
        None => path
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default(),
    }
}

/// What the first bytes say, when they say something definite.
fn sniff(head: &[u8]) -> Option<Kind> {
    let starts = |m: &[u8]| head.starts_with(m);
    if starts(b"%PDF-") {
        return Some(Kind::Pdf);
    }
    if starts(b"\x89PNG\r\n\x1a\n")
        || starts(b"\xFF\xD8\xFF")
        || starts(b"GIF87a")
        || starts(b"GIF89a")
        || (starts(b"RIFF") && head.get(8..12) == Some(b"WEBP"))
        || starts(b"BM")
    {
        return Some(Kind::Image);
    }
    if starts(b"PK\x03\x04")
        || starts(b"7z\xBC\xAF\x27\x1C")
        || starts(b"Rar!\x1A\x07")
        || starts(b"\x1F\x8B")
        || starts(b"BZh")
        || starts(b"\xFD7zXZ\x00")
        || starts(b"\x28\xB5\x2F\xFD")
        || starts(b"MSCF")
    {
        return Some(Kind::Archive);
    }
    None
}

/// Text: no NUL in the sample, unless it starts with a UTF-16 byte order mark.
pub fn looks_like_text(head: &[u8]) -> bool {
    head.starts_with(b"\xFF\xFE") || head.starts_with(b"\xFE\xFF") || !head.contains(&0)
}

/// Bytes read to decide.
const SNIFF: usize = 8192;

pub fn detect(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    let ext = ext_of(path);
    let e = ext.as_str();
    let by_ext = if TEXT.contains(&e) {
        Some(Kind::Text)
    } else if MARKDOWN.contains(&e) {
        Some(Kind::Markdown)
    } else if e == "svg" {
        Some(Kind::Svg)
    } else if e == "pdf" {
        Some(Kind::Pdf)
    } else if IMAGE.contains(&e) {
        Some(Kind::Image)
    } else if VIDEO.contains(&e) {
        Some(Kind::Video)
    } else if AUDIO.contains(&e) {
        Some(Kind::Audio)
    } else if ARCHIVE.contains(&e) {
        Some(Kind::Archive)
    } else if matches!(e, "ttf" | "otf" | "woff" | "woff2") {
        Some(Kind::Font)
    } else if e == "dwg" || e == "dxf" {
        Some(Kind::Cad)
    } else if MODEL.contains(&e) {
        Some(Kind::Model)
    } else if HANDLER.contains(&e) {
        Some(Kind::Handler)
    } else {
        None
    };
    // Media, documents and archives by name: their own previewers check the content.
    if let Some(k) = by_ext
        && k != Kind::Text
        && k != Kind::Markdown
    {
        return k;
    }
    let mut head = vec![0u8; SNIFF];
    let n = File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    head.truncate(n);
    if model_by_content(e, &head) {
        return Kind::Model;
    }
    if crate::thumbnail::handles(path, &head) {
        return Kind::Native;
    }
    if let Some(k) = sniff(&head) {
        return k;
    }
    if by_ext == Some(Kind::Markdown) && looks_like_text(&head) {
        Kind::Markdown
    } else if by_ext == Some(Kind::Text) || looks_like_text(&head) {
        Kind::Text
    } else {
        Kind::Binary
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn by_extension_and_content() {
        let d = std::env::temp_dir().join(format!("omnilook-kind-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let w = |name: &str, data: &[u8]| {
            let p = d.join(name);
            fs::write(&p, data).unwrap();
            p
        };
        assert_eq!(detect(&w("a.rs", b"fn main() {}")), Kind::Text);
        assert_eq!(detect(&w("README", b"hello")), Kind::Text);
        assert_eq!(detect(&w("README.md", b"# hi")), Kind::Markdown);
        assert_eq!(detect(&w("plan.DWG", b"AC1032")), Kind::Cad);
        assert_eq!(detect(&w("plan.dxf", b"  0\nSECTION\n")), Kind::Cad);
        // 3D models; .obj and .stp by their content.
        assert_eq!(detect(&w("part.STL", b"solid x\n")), Kind::Model);
        assert_eq!(
            detect(&w("m.obj", b"# Blender\nmtllib m.mtl\nv 0 0 0\n")),
            Kind::Model
        );
        assert_eq!(
            detect(&w("main.obj", b"\x64\x86\x05\x00rest")),
            Kind::Binary
        );
        assert_eq!(
            detect(&w("a.step", b"ISO-10303-21;\nHEADER;\n")),
            Kind::Model
        );
        assert_eq!(detect(&w("trace.stp", b"probe begin { }\n")), Kind::Text);
        assert_eq!(detect(&w("x.brep", b"DBRep_DrawableShape\n")), Kind::Model);
        assert!(Kind::Model.wants_gpu());
        // SolidWorks and Inventor by their containers; an .ipt of another program is text.
        assert_eq!(
            detect(&w("a.SLDPRT", b"\x72\xd0\x25\x0f\0\0\0\x04")),
            Kind::Native
        );
        assert_eq!(
            detect(&w("b.ipt", b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1")),
            Kind::Native
        );
        assert_eq!(detect(&w("input.ipt", b"!InputFile\n")), Kind::Text);
        assert_eq!(detect(&w("zip.md", b"PKrest")), Kind::Archive);
        assert_eq!(detect(&w("x.mp4", b"\0\0\0 ftyp")), Kind::Video);
        assert_eq!(detect(&w("x.docx", b"PK\x03\x04")), Kind::Handler);
        // A renamed archive, an unknown binary, a PDF without its extension.
        assert_eq!(detect(&w("x.dat", b"PK\x03\x04rest")), Kind::Archive);
        assert_eq!(detect(&w("x.bin", b"\x00\x01\x02")), Kind::Binary);
        assert_eq!(detect(&w("x", b"%PDF-1.7\n")), Kind::Pdf);
        assert_eq!(detect(&w("utf16.txt", b"\xFF\xFEa\0b\0")), Kind::Text);
        assert_eq!(detect(&d), Kind::Folder);
        assert!(Kind::Video.wants_gpu() && Kind::Cad.wants_gpu() && !Kind::Pdf.wants_gpu());
        fs::remove_dir_all(&d).unwrap();
    }
}
