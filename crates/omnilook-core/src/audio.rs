//! Audio and video details (`docs/development-plan.md` §5.1): tags, length and format of a sound
//! file read by `lofty` (MP3, FLAC, Ogg, Opus, M4A/AAC, WAV, AIFF, WMA, APE…), and its cover
//! picture. The page plays the file itself; this is what it shows beside the player, and what it
//! shows instead when the format cannot be played in the window.

use std::path::Path;

use lofty::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    /// kbit/s.
    pub bitrate: Option<u32>,
    /// Hz.
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub has_cover: bool,
}

/// Video and sound formats WebView2 cannot play (M0, `docs/m0-results.md`): the window shows the
/// file's details and "open" instead of a player that fails.
pub const UNPLAYABLE: &[&str] = &["avi", "wmv", "wma", "flv", "3gp", "ape"];

pub fn playable(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    !UNPLAYABLE.contains(&ext.as_str())
}

fn clean(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// What the tags and the stream say; empty when the file is not one `lofty` reads (a video).
pub fn read(path: &Path) -> AudioInfo {
    let Ok(f) = lofty::read_from_path(path) else {
        return AudioInfo::default();
    };
    let p = f.properties();
    let tag = f.primary_tag().or_else(|| f.first_tag());
    let ms = p.duration().as_millis() as u64;
    AudioInfo {
        title: tag.and_then(|t| clean(t.title())),
        artist: tag.and_then(|t| clean(t.artist())),
        album: tag.and_then(|t| clean(t.album())),
        duration_ms: (ms > 0).then_some(ms),
        bitrate: p.audio_bitrate().or(p.overall_bitrate()).filter(|&b| b > 0),
        sample_rate: p.sample_rate().filter(|&r| r > 0),
        channels: p.channels().filter(|&c| c > 0),
        has_cover: f.tags().iter().any(|t| !t.pictures().is_empty()),
    }
}

/// The cover picture (the front cover when marked, else the first) and its media type.
pub fn cover(path: &Path) -> Option<(Vec<u8>, String)> {
    let f = lofty::read_from_path(path).ok()?;
    let pics: Vec<_> = f.tags().iter().flat_map(|t| t.pictures()).collect();
    let pic = pics
        .iter()
        .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
        .or_else(|| pics.first())?;
    let mime = pic
        .mime_type()
        .map(|m| m.as_str().to_owned())
        .filter(|m| m.starts_with("image/"))
        .unwrap_or_else(|| "image/jpeg".into());
    Some((pic.data().to_vec(), mime))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal 16-bit PCM WAV of `ms` milliseconds of silence at 8 kHz mono.
    fn wav(ms: u32) -> Vec<u8> {
        let rate = 8000u32;
        let data = rate * ms / 1000 * 2;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 2).to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data.to_le_bytes());
        v.resize(v.len() + data as usize, 0);
        v
    }

    #[test]
    fn reads_stream_details() {
        let d = std::env::temp_dir().join(format!("omnilook-audio-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("a.wav");
        std::fs::write(&p, wav(1500)).unwrap();
        let i = read(&p);
        assert_eq!(i.duration_ms, Some(1500), "{i:?}");
        assert_eq!((i.sample_rate, i.channels), (Some(8000), Some(1)));
        assert!(!i.has_cover && i.title.is_none());
        assert!(cover(&p).is_none());
        // Not audio at all: nothing, no error.
        let t = d.join("x.mp3");
        std::fs::write(&t, b"not audio").unwrap();
        assert_eq!(read(&t), AudioInfo::default());
        assert!(playable(&p) && !playable(Path::new("x.AVI")));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
