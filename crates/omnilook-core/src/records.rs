//! Firmware record files: Intel HEX (`.hex`, `.ihx`…) and Motorola S-records (`.s19`, `.srec`,
//! `.mot`…). They are text, shown by the text preview; this reads the whole file (not only the part
//! the text view shows) for what it holds — data size, address ranges, entry point, S0 header —
//! and checks every record's length and checksum. The page colours each record's fields itself
//! (`apps/desktop/src/lib/records.ts`), so only the lines on screen cost anything.

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    IntelHex,
    SRecord,
}

impl Format {
    /// Display name, the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Format::IntelHex => "Intel HEX",
            Format::SRecord => "Motorola S-record",
        }
    }
}

const INTEL: &[&str] = &["hex", "ihx", "ihex", "h86", "hxl", "hxh", "mcs", "a43"];
const SREC: &[&str] = &[
    "s19", "s28", "s37", "srec", "srecord", "mot", "mhx", "sx", "s1", "s2", "s3",
];

/// The record format of a file named `path` whose text starts with `head`: the extension says
/// which, and the first record confirms it (a `.hex` can also be a plain hex dump).
pub fn detect(path: &Path, head: &str) -> Option<Format> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let format = if INTEL.contains(&ext.as_str()) {
        Format::IntelHex
    } else if SREC.contains(&ext.as_str()) {
        Format::SRecord
    } else {
        return None;
    };
    let first = head
        .trim_start_matches('\u{FEFF}')
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    let hex_after =
        |n: usize| first.len() >= n + 8 && first.as_bytes()[n..].iter().all(u8::is_ascii_hexdigit);
    let ok = match format {
        Format::IntelHex => first.starts_with(':') && hex_after(1),
        Format::SRecord => {
            first.starts_with('S') && first.as_bytes()[1].is_ascii_digit() && hex_after(2)
        }
    };
    ok.then_some(format)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Problem {
    /// The checksum byte is wrong (`expected` holds the right one).
    Checksum,
    /// The byte count does not match the record's length.
    Length,
    /// Not a record: wrong start, characters that are not hex digits, too short, unknown type.
    Syntax,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BadLine {
    /// Line index, from 0, counting `\n`s (as the text view does).
    pub line: u32,
    pub problem: Problem,
    pub expected: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub format: Format,
    pub records: u64,
    /// Bytes in data records.
    pub data_bytes: u64,
    /// Address ranges the data covers, `[start, end)`, merged and sorted; at most [`MAX_RANGES`].
    pub ranges: Vec<[u64; 2]>,
    pub range_count: u64,
    /// From the first byte of data to past the last, `[start, end)`.
    pub span: Option<[u64; 2]>,
    /// Execution start address (Intel 03/05, S7/S8/S9).
    pub entry: Option<u64>,
    /// The S0 header's text.
    pub header: Option<String>,
    /// Intel HEX: the end-of-file record was there (a cut-off download lacks it).
    pub ended: bool,
    /// Problems, in line order; at most [`MAX_BAD`].
    pub bad: Vec<BadLine>,
    pub bad_count: u64,
    /// The file was larger than [`MAX_SCAN`]; the rest was not read.
    pub partial: bool,
}

pub const MAX_RANGES: usize = 64;
pub const MAX_BAD: usize = 1000;
/// Bytes read for the summary; firmware is far smaller.
pub const MAX_SCAN: u64 = 512 << 20;

/// Reads `path` as `format` records.
pub fn scan(path: &Path, format: Format) -> std::io::Result<Summary> {
    let mut reader = BufReader::with_capacity(1 << 16, File::open(path)?);
    let mut s = Scanner::new(format);
    let mut line = Vec::with_capacity(128);
    let mut read = 0u64;
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        read += n as u64;
        if read > MAX_SCAN {
            s.partial = true;
            break;
        }
        s.line(&line);
    }
    Ok(s.finish())
}

/// Reads records from text already in memory.
pub fn scan_text(text: &str, format: Format) -> Summary {
    let mut s = Scanner::new(format);
    for line in text.split('\n') {
        s.line(line.as_bytes());
    }
    s.finish()
}

struct Scanner {
    format: Format,
    index: u32,
    records: u64,
    data_bytes: u64,
    ranges: Vec<[u64; 2]>,
    entry: Option<u64>,
    header: Option<String>,
    ended: bool,
    bad: Vec<BadLine>,
    bad_count: u64,
    partial: bool,
    /// Intel HEX: the base added to data addresses (record 02 or 04).
    base: u64,
    bytes: Vec<u8>,
}

impl Scanner {
    fn new(format: Format) -> Self {
        Scanner {
            format,
            index: 0,
            records: 0,
            data_bytes: 0,
            ranges: Vec::new(),
            entry: None,
            header: None,
            ended: false,
            bad: Vec::new(),
            bad_count: 0,
            partial: false,
            base: 0,
            bytes: Vec::with_capacity(64),
        }
    }

    fn line(&mut self, raw: &[u8]) {
        let at = self.index;
        self.index += 1;
        let mut l = raw.trim_ascii();
        if at == 0 && l.starts_with("\u{FEFF}".as_bytes()) {
            l = &l[3..];
        }
        // DOS tools end files with Ctrl+Z.
        while let [rest @ .., 0x1A] = l {
            l = rest.trim_ascii_end();
        }
        if l.is_empty() {
            return;
        }
        let result = match self.format {
            Format::IntelHex => self.intel(l),
            Format::SRecord => self.srec(l),
        };
        match result {
            Ok(()) => self.records += 1,
            Err((problem, expected)) => {
                self.bad_count += 1;
                if self.bad.len() < MAX_BAD {
                    self.bad.push(BadLine {
                        line: at,
                        problem,
                        expected,
                    });
                }
            }
        }
    }

    /// Hex digit pairs after the `skip` leading characters, into `self.bytes`.
    fn decode(&mut self, l: &[u8], skip: usize) -> Result<(), (Problem, Option<u8>)> {
        let digits = &l[skip..];
        let (pairs, odd) = digits.as_chunks::<2>();
        if !odd.is_empty() {
            return Err((Problem::Syntax, None));
        }
        self.bytes.clear();
        for &[a, b] in pairs {
            let hi = hex(a).ok_or((Problem::Syntax, None))?;
            let lo = hex(b).ok_or((Problem::Syntax, None))?;
            self.bytes.push(hi << 4 | lo);
        }
        Ok(())
    }

    // :LL AAAA TT DD… CC — CC makes the sum of all bytes 0.
    fn intel(&mut self, l: &[u8]) -> Result<(), (Problem, Option<u8>)> {
        if l[0] != b':' || l.len() < 11 {
            return Err((Problem::Syntax, None));
        }
        self.decode(l, 1)?;
        let b = &self.bytes;
        let count = b[0] as usize;
        if b.len() != count + 5 {
            return Err((Problem::Length, None));
        }
        let sum = b[..b.len() - 1].iter().fold(0u8, |a, &x| a.wrapping_add(x));
        let expected = sum.wrapping_neg();
        if b[b.len() - 1] != expected {
            return Err((Problem::Checksum, Some(expected)));
        }
        let addr = u64::from(u16::from_be_bytes([b[1], b[2]]));
        let data = &b[4..4 + count];
        match b[3] {
            0x00 => {
                let start = self.base + addr;
                self.add_range(start, count as u64);
            }
            0x01 => self.ended = true,
            0x02 if count == 2 => {
                self.base = u64::from(u16::from_be_bytes([data[0], data[1]])) << 4
            }
            0x04 if count == 2 => {
                self.base = u64::from(u16::from_be_bytes([data[0], data[1]])) << 16
            }
            0x03 if count == 4 => {
                let cs = u64::from(u16::from_be_bytes([data[0], data[1]]));
                let ip = u64::from(u16::from_be_bytes([data[2], data[3]]));
                self.entry = Some((cs << 4) + ip);
            }
            0x05 if count == 4 => {
                self.entry = Some(u64::from(u32::from_be_bytes([
                    data[0], data[1], data[2], data[3],
                ])))
            }
            0x02..=0x05 => return Err((Problem::Length, None)),
            _ => return Err((Problem::Syntax, None)),
        }
        Ok(())
    }

    // Sn LL AA… DD… CC — LL counts address, data and checksum; CC is the ones' complement of
    // the low byte of the sum of LL, address and data.
    fn srec(&mut self, l: &[u8]) -> Result<(), (Problem, Option<u8>)> {
        if l[0] != b'S' || l.len() < 10 {
            return Err((Problem::Syntax, None));
        }
        let alen = match l[1] {
            b'0' | b'1' | b'5' | b'9' => 2,
            b'2' | b'6' | b'8' => 3,
            b'3' | b'7' => 4,
            _ => return Err((Problem::Syntax, None)),
        };
        self.decode(l, 2)?;
        let b = &self.bytes;
        let count = b[0] as usize;
        if b.len() != count + 1 || count < alen + 1 {
            return Err((Problem::Length, None));
        }
        let sum = b[..b.len() - 1].iter().fold(0u8, |a, &x| a.wrapping_add(x));
        let expected = !sum;
        if b[b.len() - 1] != expected {
            return Err((Problem::Checksum, Some(expected)));
        }
        let addr = b[1..1 + alen]
            .iter()
            .fold(0u64, |a, &x| a << 8 | u64::from(x));
        let data = &b[1 + alen..b.len() - 1];
        match l[1] {
            b'1' | b'2' | b'3' => {
                let n = data.len() as u64;
                self.add_range(addr, n);
            }
            b'0' if self.header.is_none() => {
                let text: String = data
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| {
                        if c.is_ascii_graphic() || c == b' ' {
                            c as char
                        } else {
                            '·'
                        }
                    })
                    .collect();
                let text = text.trim().to_owned();
                if !text.is_empty() {
                    self.header = Some(text);
                }
            }
            b'7' | b'8' | b'9' => {
                self.entry = Some(addr);
                self.ended = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn add_range(&mut self, start: u64, n: u64) {
        self.data_bytes += n;
        if n == 0 {
            return;
        }
        let end = start + n;
        if let Some(last) = self.ranges.last_mut()
            && last[1] == start
        {
            last[1] = end;
            return;
        }
        self.ranges.push([start, end]);
        if self.ranges.len() >= 1 << 16 {
            merge(&mut self.ranges);
        }
    }

    fn finish(mut self) -> Summary {
        merge(&mut self.ranges);
        let range_count = self.ranges.len() as u64;
        let span = match (self.ranges.first(), self.ranges.last()) {
            (Some(a), Some(b)) => Some([a[0], b[1]]),
            _ => None,
        };
        self.ranges.truncate(MAX_RANGES);
        Summary {
            format: self.format,
            records: self.records,
            data_bytes: self.data_bytes,
            ranges: self.ranges,
            range_count,
            span,
            entry: self.entry,
            header: self.header,
            ended: self.ended,
            bad: self.bad,
            bad_count: self.bad_count,
            partial: self.partial,
        }
    }
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Sorts ranges and joins the ones that touch or overlap.
fn merge(ranges: &mut Vec<[u64; 2]>) {
    ranges.sort_unstable();
    let mut out: Vec<[u64; 2]> = Vec::with_capacity(ranges.len());
    for r in ranges.drain(..) {
        match out.last_mut() {
            Some(last) if r[0] <= last[1] => last[1] = last[1].max(r[1]),
            _ => out.push(r),
        }
    }
    *ranges = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_and_first_record() {
        let intel = ":020000040800F2\n";
        assert_eq!(detect(Path::new("a.HEX"), intel), Some(Format::IntelHex));
        assert_eq!(detect(Path::new("a.ihx"), intel), Some(Format::IntelHex));
        // A .hex that is a plain dump is left alone.
        assert_eq!(detect(Path::new("a.hex"), "00 01 02 03\n"), None);
        let srec = "\u{FEFF}\nS00F000068656C6C6F202020202000003C\n";
        assert_eq!(detect(Path::new("a.s19"), srec), Some(Format::SRecord));
        assert_eq!(detect(Path::new("a.mot"), srec), Some(Format::SRecord));
        assert_eq!(detect(Path::new("a.txt"), srec), None);
        assert_eq!(detect(Path::new("a.s19"), "Some notes"), None);
    }

    #[test]
    fn intel_hex_summary() {
        // Extended linear address 0x0800, 16 + 4 bytes, start address, end.
        let text = "\
:020000040800F2\r
:10000000000102030405060708090A0B0C0D0E0F78\r
:0400100010111213A6\r
:0400000508000131BD\r
:00000001FF\r
";
        let s = scan_text(text, Format::IntelHex);
        assert_eq!(s.bad_count, 0, "{:?}", s.bad);
        assert_eq!(s.records, 5);
        assert_eq!(s.data_bytes, 20);
        assert_eq!(s.ranges, vec![[0x0800_0000, 0x0800_0014]]);
        assert_eq!(s.entry, Some(0x0800_0131));
        assert!(s.ended);
    }

    #[test]
    fn intel_hex_problems() {
        let text = "\
:10000000000102030405060708090A0B0C0D0E0F00
:0400100010111213
:04001000101112ZZA6
:00000001FF
";
        let s = scan_text(text, Format::IntelHex);
        let got: Vec<_> = s
            .bad
            .iter()
            .map(|b| (b.line, b.problem, b.expected))
            .collect();
        assert_eq!(
            got,
            vec![
                (0, Problem::Checksum, Some(0x78)),
                (1, Problem::Length, None),
                (2, Problem::Syntax, None),
            ]
        );
        assert_eq!(s.records, 1);
        // A download cut off before the end record.
        let s = scan_text(":0400100010111213A6\n", Format::IntelHex);
        assert!(!s.ended);
        // The DOS end-of-file mark is not a record.
        let s = scan_text(":00000001FF\r\n\x1A", Format::IntelHex);
        assert_eq!((s.records, s.bad_count), (1, 0));
    }

    #[test]
    fn srecord_summary() {
        // Header "hello", two S1 data records with a gap, S5 count, S9 entry.
        let text = "\
S00F000068656C6C6F202020202000003C
S1130000285F245F2212226A000424290008237C2A
S11300100002000800082629001853812341001813
S10B10000001020304050607C8
S5030003F9
S9030000FC
";
        let s = scan_text(text, Format::SRecord);
        assert_eq!(s.bad_count, 0, "{:?}", s.bad);
        assert_eq!(s.header.as_deref(), Some("hello"));
        assert_eq!(s.data_bytes, 16 + 16 + 8);
        assert_eq!(s.ranges, vec![[0, 0x20], [0x1000, 0x1008]]);
        assert_eq!(s.range_count, 2);
        assert_eq!(s.span, Some([0, 0x1008]));
        assert_eq!(s.entry, Some(0));
        assert!(s.ended);
    }

    #[test]
    fn srecord_problems() {
        let text = "\
S1130000285F245F2212226A000424290008237C2B
S41300000000
S10B10000001020304050607
";
        let s = scan_text(text, Format::SRecord);
        let got: Vec<_> = s
            .bad
            .iter()
            .map(|b| (b.line, b.problem, b.expected))
            .collect();
        assert_eq!(
            got,
            vec![
                (0, Problem::Checksum, Some(0x2A)),
                (1, Problem::Syntax, None),
                (2, Problem::Length, None),
            ]
        );
    }

    #[test]
    fn ranges_merge_out_of_order() {
        let mut r = vec![[10, 20], [0, 10], [30, 40], [15, 25]];
        merge(&mut r);
        assert_eq!(r, vec![[0, 25], [30, 40]]);
    }
}
