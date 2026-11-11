//! Any other file (`docs/development-plan.md` §5.1): its first bytes as a hex dump.

use std::{fs::File, io::Read, path::Path};

use serde::Serialize;

/// Bytes shown.
pub const MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HexPreview {
    /// One line per 16 bytes: offset, bytes, printable ASCII.
    pub dump: String,
    pub truncated: bool,
}

pub fn dump(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 5);
    for (i, chunk) in bytes.chunks(16).enumerate() {
        out.push_str(&format!("{:08X}  ", i * 16));
        for j in 0..16 {
            match chunk.get(j) {
                Some(b) => out.push_str(&format!("{b:02X} ")),
                None => out.push_str("   "),
            }
            if j == 7 {
                out.push(' ');
            }
        }
        out.push(' ');
        out.extend(chunk.iter().map(|&b| {
            if (0x20..0x7F).contains(&b) {
                b as char
            } else {
                '.'
            }
        }));
        out.push('\n');
    }
    out
}

pub fn read(path: &Path) -> std::io::Result<HexPreview> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut buf = Vec::with_capacity(MAX_BYTES);
    f.by_ref().take(MAX_BYTES as u64).read_to_end(&mut buf)?;
    Ok(HexPreview {
        dump: dump(&buf),
        truncated: len > MAX_BYTES as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let d = dump(b"MZ\x90\x00 hello, world!\x01");
        let first = d.lines().next().unwrap();
        assert!(first.starts_with("00000000  4D 5A 90 00 20 68"), "{first}");
        assert!(first.ends_with("MZ.. hello, worl"), "{first}");
        assert_eq!(d.lines().count(), 2);
    }
}
