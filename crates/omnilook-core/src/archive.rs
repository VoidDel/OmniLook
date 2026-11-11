//! Archives (`docs/development-plan.md` §5.1): the list of what is in them, read with OmniZip's
//! formats (the same code OmniZip opens them with). Nothing is extracted.

use std::path::Path;

use omnizip_formats::omnizip_core::{EntryKind, ErrorCode, JobCtx, OpenOptions};
use serde::Serialize;

/// Entries listed (the rest are counted only).
pub const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    /// `/`-separated, as stored.
    pub path: String,
    pub dir: bool,
    pub size: u64,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePreview {
    pub format: String,
    pub entries: Vec<ArchiveEntry>,
    pub files: u64,
    pub folders: u64,
    /// Uncompressed size of all files.
    pub size: u64,
    pub encrypted: bool,
    pub truncated: bool,
    /// Why the list could not be read: `passwordRequired` (encrypted names), `corrupt`,
    /// `unsupported` or another OmniZip error code.
    pub error: Option<String>,
}

pub fn read(path: &Path, locale: Option<&str>) -> ArchivePreview {
    let opts = OpenOptions {
        locale: locale.map(str::to_owned),
        ..Default::default()
    };
    let reader = match omnizip_formats::shared().open(path, &opts, &JobCtx::default()) {
        Ok(r) => r,
        Err(e) => {
            let code = match e.code() {
                ErrorCode::PasswordRequired | ErrorCode::WrongPassword => "passwordRequired".into(),
                ErrorCode::UnsupportedFormat => "unsupported".into(),
                c => format!("{c:?}"),
            };
            return ArchivePreview {
                error: Some(code),
                ..Default::default()
            };
        }
    };
    let info = reader.info();
    let mut p = ArchivePreview {
        format: info.format.to_string(),
        encrypted: info.encrypted || info.header_encrypted,
        ..Default::default()
    };
    // Folders: those stored, and those only implied by paths (many archivers store no folder
    // entries).
    let mut folders = std::collections::HashSet::new();
    for e in reader.entries() {
        let dir = e.kind == EntryKind::Dir;
        let trimmed = e.path.trim_end_matches('/');
        // A folder goes in with its parents; one already in has its parents in already.
        let mut chain = if dir {
            Some(trimmed)
        } else {
            trimmed.rsplit_once('/').map(|(parent, _)| parent)
        };
        while let Some(f) = chain {
            if f.is_empty() || !folders.insert(f.to_owned()) {
                break;
            }
            chain = f.rsplit_once('/').map(|(parent, _)| parent);
        }
        if !dir {
            p.files += 1;
            p.size += e.size;
        }
        if p.entries.len() < MAX_ENTRIES {
            p.entries.push(ArchiveEntry {
                path: e.path.clone(),
                dir,
                size: e.size,
                encrypted: e.encrypted,
            });
        } else {
            p.truncated = true;
        }
    }
    p.folders = folders.len() as u64;
    p
}

#[cfg(test)]
mod tests {
    use std::fs;

    use omnizip_formats::omnizip_core::{CollectOptions, FormatId, WriteOptions, source};

    use super::*;

    #[test]
    fn lists_a_zip_and_refuses_garbage() {
        let d = std::env::temp_dir().join(format!("omnilook-archive-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let src = d.join("相册");
        fs::create_dir_all(src.join("2026")).unwrap();
        fs::write(src.join("2026/a.txt"), "alpha").unwrap();
        fs::write(src.join("b.txt"), "beta!").unwrap();
        let ctx = JobCtx::default();
        let sources = source::collect(&[src], &CollectOptions::default(), &ctx).unwrap();
        let zip = d.join("x.zip");
        let opts = WriteOptions {
            format: FormatId::Zip,
            ..Default::default()
        };
        omnizip_formats::shared()
            .create(&zip, &sources, &opts, &ctx)
            .unwrap();
        let p = read(&zip, Some("zh-CN"));
        assert_eq!(p.error, None);
        assert_eq!(p.format, "zip");
        assert_eq!((p.files, p.folders, p.size), (2, 2, 10));
        assert!(
            p.entries
                .iter()
                .any(|e| e.path == "相册/2026/a.txt" && !e.dir)
        );
        fs::write(d.join("bad.zip"), b"PK\x03\x04 not really").unwrap();
        assert!(read(&d.join("bad.zip"), None).error.is_some());
        fs::remove_dir_all(&d).unwrap();
    }
}
