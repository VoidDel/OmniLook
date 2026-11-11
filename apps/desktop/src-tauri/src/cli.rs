//! The command line (`docs/development-plan.md` §6.2).

use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewArgs {
    pub path: PathBuf,
    pub title: Option<String>,
    /// The Explorer window to center on (a window handle).
    pub anchor: Option<isize>,
    /// WebView2 with GPU acceleration (video); the agent decides by the file's kind.
    pub gpu: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Agent,
    View(ViewArgs),
    Settings,
    Preview {
        path: PathBuf,
        title: Option<String>,
        /// The caller's pipe for the previous / next file (§6.3).
        nav: Option<String>,
    },
    Quit,
    Usage(String),
}

const USAGE: &str =
    "usage: omnilook [preview] <file> [--title <text>] [--nav <pipe>] | omnilook --quit";

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Mode {
    let args: Vec<OsString> = args.into_iter().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if flag("--agent") {
        return Mode::Agent;
    }
    if flag("--quit") {
        return Mode::Quit;
    }
    let title = value("--title").map(|t| t.to_string_lossy().into_owned());
    let nav = value("--nav")
        .map(|t| t.to_string_lossy().into_owned())
        // Only a local named pipe.
        .filter(|n| n.starts_with(r"\\.\pipe\"));
    if flag("--view") {
        let Some(path) = value("--path") else {
            return Mode::Usage("--view needs --path".into());
        };
        return Mode::View(ViewArgs {
            path: PathBuf::from(path),
            title,
            anchor: value("--anchor").and_then(|a| a.to_string_lossy().parse().ok()),
            gpu: flag("--gpu"),
        });
    }
    // The first argument that is neither a flag nor a flag's value.
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].to_string_lossy();
        // Flags with a value (`--source` names the caller; accepted, not used yet).
        if a == "--title" || a == "--source" || a == "--nav" {
            i += 2;
            continue;
        }
        if !a.starts_with("--") {
            positional.push(args[i].clone());
        }
        i += 1;
    }
    match positional.as_slice() {
        [] => Mode::Settings,
        [cmd, file] if cmd == "preview" => Mode::Preview {
            path: PathBuf::from(file),
            title,
            nav,
        },
        [file] if file != "preview" => Mode::Preview {
            path: PathBuf::from(file),
            title,
            nav,
        },
        _ => Mode::Usage(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &[&str]) -> Mode {
        parse(s.iter().map(OsString::from))
    }

    #[test]
    fn modes() {
        assert_eq!(p(&[]), Mode::Settings);
        assert_eq!(p(&["--agent"]), Mode::Agent);
        assert_eq!(p(&["--quit"]), Mode::Quit);
        assert_eq!(
            p(&["preview", r"C:\a b.txt", "--title", "x.zip › a b.txt"]),
            Mode::Preview {
                path: PathBuf::from(r"C:\a b.txt"),
                title: Some("x.zip › a b.txt".into()),
                nav: None
            }
        );
        assert_eq!(
            p(&["C:\\x.png"]),
            Mode::Preview {
                path: PathBuf::from("C:\\x.png"),
                title: None,
                nav: None
            }
        );
        assert_eq!(
            p(&["--view", "--path", "C:\\v.mp4", "--anchor", "1234", "--gpu"]),
            Mode::View(ViewArgs {
                path: PathBuf::from("C:\\v.mp4"),
                title: None,
                anchor: Some(1234),
                gpu: true
            })
        );
        assert_eq!(
            p(&["preview", "C:\\a.txt", "--source", "omnizip"]),
            Mode::Preview {
                path: PathBuf::from("C:\\a.txt"),
                title: None,
                nav: None
            }
        );
        assert_eq!(
            p(&["preview", r"C:\c.txt", "--nav", r"\\.\pipe\omnizip-nav-1"]),
            Mode::Preview {
                path: PathBuf::from(r"C:\c.txt"),
                title: None,
                nav: Some(r"\\.\pipe\omnizip-nav-1".into())
            }
        );
        // Only a named pipe.
        assert!(matches!(
            p(&["preview", r"C:\c.txt", "--nav", r"C:\x"]),
            Mode::Preview { nav: None, .. }
        ));
        assert!(matches!(p(&["preview"]), Mode::Usage(_)));
        assert!(matches!(p(&["a", "b"]), Mode::Usage(_)));
    }
}
