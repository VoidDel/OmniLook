//! Settings (`docs/development-plan.md` §1.2): `%APPDATA%\OmniLook\settings.json`. The agent reads
//! the hotkey; the settings window reads and writes everything; the appearance belongs to the page
//! and is kept as it comes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Hotkey {
    #[default]
    CtrlSpace,
    Space,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkey: Hotkey,
    /// Start the agent when signing in to Windows.
    pub autostart: bool,
    /// Show the notification-area icon (off by default: starting Omni Look again opens the
    /// settings).
    pub tray: bool,
    /// Close the preview when another window comes to the front (anything but the preview and the
    /// window it was opened from).
    pub close_on_blur: bool,
    /// Look for a new version once a day (the agent) and when the settings open.
    pub check_updates: bool,
    /// `auto` or a language tag.
    pub language: String,
    /// The page's appearance settings (mode, themes, accent), opaque here.
    pub appearance: serde_json::Value,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: Hotkey::default(),
            autostart: true,
            tray: false,
            close_on_blur: true,
            check_updates: true,
            language: "auto".into(),
            appearance: serde_json::Value::Null,
        }
    }
}

pub fn path() -> PathBuf {
    paths::config_dir().join("settings.json")
}

impl Settings {
    /// The saved settings, or the defaults when there are none or they do not parse.
    pub fn load() -> Self {
        std::fs::read(path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Writes through a temporary file, so a crash never leaves half a file.
    pub fn save(&self) -> std::io::Result<()> {
        let p = path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_files() {
        let s: Settings = serde_json::from_str(r#"{"hotkey":"space"}"#).unwrap();
        assert_eq!(s.hotkey, Hotkey::Space);
        assert!(s.autostart, "missing fields take their defaults");
        assert!(s.close_on_blur);
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Settings::default());
    }
}
