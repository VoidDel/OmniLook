//! A folder (`docs/development-plan.md` §5.1): what is directly in it, and how much is in it
//! altogether as far as a short time budget allows to count.

use std::{
    fs,
    path::Path,
    time::{Duration, Instant, SystemTime},
};

use serde::Serialize;

/// Items listed (the rest are counted only).
pub const MAX_ITEMS: usize = 500;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderPreview {
    /// Directly inside, folders first, then by name; at most [`MAX_ITEMS`].
    pub items: Vec<Item>,
    /// Everything below, counted until the budget ran out.
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    /// The counts are final (the whole tree was walked in time).
    pub complete: bool,
}

fn millis(t: SystemTime) -> Option<u64> {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

pub fn read(path: &Path, budget: Duration) -> std::io::Result<FolderPreview> {
    let start = Instant::now();
    let mut items = Vec::new();
    for e in fs::read_dir(path)?.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        items.push(Item {
            name: e.file_name().to_string_lossy().into_owned(),
            dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: meta.modified().ok().and_then(millis),
        });
    }
    items.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    // Counted breadth first, without following links (a junction can loop).
    let (mut files, mut folders, mut bytes) = (0u64, 0u64, 0u64);
    let mut queue = std::collections::VecDeque::from([path.to_path_buf()]);
    let mut complete = true;
    while let Some(dir) = queue.pop_front() {
        if start.elapsed() > budget {
            complete = false;
            break;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                folders += 1;
                queue.push_back(e.path());
            } else {
                files += 1;
                bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    items.truncate(MAX_ITEMS);
    Ok(FolderPreview {
        items,
        files,
        folders,
        bytes,
        complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_order() {
        let d = std::env::temp_dir().join(format!("omnilook-folder-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("Sub/deeper")).unwrap();
        fs::write(d.join("b.txt"), "12345").unwrap();
        fs::write(d.join("a.txt"), "1").unwrap();
        fs::write(d.join("Sub/deeper/c.bin"), [0u8; 10]).unwrap();
        let p = read(&d, Duration::from_secs(5)).unwrap();
        let names: Vec<_> = p.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Sub", "a.txt", "b.txt"]);
        assert_eq!((p.files, p.folders, p.bytes, p.complete), (3, 2, 16, true));
        fs::remove_dir_all(&d).unwrap();
    }
}
