//! Messages between Omni Look's processes, one JSON line each way per connection
//! (`docs/development-plan.md` §3.1, §6.2).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// To the agent (pipe `omnilook-agent-<SID>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Request {
    /// Preview a file (the command line, OmniZip); the same file again closes the preview.
    Preview {
        path: PathBuf,
        #[serde(default)]
        title: Option<String>,
        /// The caller's pipe for the previous / next file (OmniZip inside an archive, protocol
        /// v2, `docs/development-plan.md` §6.3).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nav: Option<String>,
    },
    /// A preview process is ready for files on `pipe`.
    ViewReady {
        pid: u32,
        pipe: String,
    },
    /// The user closed the preview window.
    ViewClosed {
        pid: u32,
    },
    /// An arrow key in the preview window: the next (`1`) or previous (`-1`) file.
    Nav {
        pid: u32,
        step: i32,
    },
    /// The settings changed on disk.
    Reload,
    Quit,
    Ping,
}

/// To a preview process (pipe `omnilook-view-<pid>-<SID>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ViewCommand {
    Show {
        path: PathBuf,
        #[serde(default)]
        title: Option<String>,
    },
    Close,
}

/// To the caller's navigation pipe (§6.3): the file next to `path`, the one it gave last.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavAsk {
    /// `next` or `prev`.
    pub op: String,
    pub path: PathBuf,
}

/// Its answer: the file and its title, or `end` when there is none that way.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NavAnswer {
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub end: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    pub fn err(e: impl std::fmt::Display) -> Self {
        Self {
            ok: false,
            error: Some(e.to_string()),
        }
    }

    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"ok":false}"#.into())
    }

    pub fn parse(line: &str) -> Self {
        serde_json::from_str(line).unwrap_or_else(Self::err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let r = Request::Preview {
            path: PathBuf::from(r"C:\a.txt"),
            title: Some("t".into()),
            nav: None,
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"op":"preview","path":"C:\\a.txt","title":"t"}"#
        );
        let r: Request = serde_json::from_str(r#"{"op":"viewReady","pid":7,"pipe":"p"}"#).unwrap();
        assert!(matches!(r, Request::ViewReady { pid: 7, .. }));
        // OmniZip may leave the title out.
        let r: Request = serde_json::from_str(r#"{"op":"preview","path":"x"}"#).unwrap();
        assert!(matches!(r, Request::Preview { title: None, .. }));
        assert!(!Reply::parse("garbage").ok);
        // The caller's navigation answers (§6.3).
        let a: NavAnswer =
            serde_json::from_str(r#"{"path":"C:\\b.txt","title":"x › b.txt"}"#).unwrap();
        assert_eq!(a.path, Some(PathBuf::from(r"C:\b.txt")));
        assert_eq!(a.title.as_deref(), Some("x › b.txt"));
        let a: NavAnswer = serde_json::from_str(r#"{"end":true}"#).unwrap();
        assert!(a.end && a.path.is_none());
    }
}
