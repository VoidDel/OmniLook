//! CAD solids (STEP, BREP) read into glTF, kept between previews: reading a large assembly takes
//! half a minute, showing its glTF a moment. Kept by the file's path, size and date, so a changed
//! file is read again; past `CAP` the least recently shown go.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::paths;

/// The most the kept glTF files take together.
const CAP: u64 = 1 << 30;

fn dir() -> PathBuf {
    paths::local_dir().join("cache").join("solids")
}

/// FNV-1a: a stable hash (the standard library's may change between Rust versions).
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in *p {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The cache file for `path` as it is now.
fn entry(path: &Path) -> Option<PathBuf> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let name = path.to_string_lossy();
    let h = fnv(&[
        name.as_bytes(),
        &meta.len().to_le_bytes(),
        &modified.to_le_bytes(),
    ]);
    Some(dir().join(format!("{h:016x}.glb")))
}

/// The glTF kept for `path`, if any (and marked as just used).
pub fn get(path: &Path) -> Option<Vec<u8>> {
    let e = entry(path)?;
    let data = fs::read(&e).ok()?;
    if let Ok(f) = fs::File::options().write(true).open(&e) {
        let _ = f.set_modified(SystemTime::now());
    }
    Some(data)
}

/// Keeps `glb` for `path`, then trims the cache to `CAP`.
pub fn put(path: &Path, glb: &[u8]) -> io::Result<()> {
    let e = entry(path).ok_or_else(|| io::Error::other("no such file"))?;
    fs::create_dir_all(dir())?;
    // Written whole, then moved in place: a half-written file is never read.
    let tmp = e.with_extension("tmp");
    fs::File::create(&tmp)?.write_all(glb)?;
    fs::rename(&tmp, &e)?;
    trim(&dir(), CAP);
    Ok(())
}

/// Removes the least recently used files of `dir` until they take at most `cap` bytes.
fn trim(dir: &Path, cap: u64) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| {
                (
                    m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    m.len(),
                    e.path(),
                )
            })
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, len, p) in files {
        if total <= cap {
            break;
        }
        if fs::remove_file(&p).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_within_the_cap() {
        let d = std::env::temp_dir().join(format!("omnilook-cache-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        for (i, n) in ["a", "b", "c"].iter().enumerate() {
            let p = d.join(n);
            fs::write(&p, vec![0u8; 100]).unwrap();
            let f = fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(
                SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i as u64),
            )
            .unwrap();
        }
        trim(&d, 200);
        assert!(!d.join("a").exists());
        assert!(d.join("b").exists() && d.join("c").exists());
        fs::remove_dir_all(&d).unwrap();
        assert_ne!(fnv(&[b"ab", b"c"]), fnv(&[b"a", b"bc"]));
    }
}
