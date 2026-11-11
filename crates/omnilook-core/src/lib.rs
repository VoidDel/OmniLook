//! Omni Look's previewers (`docs/development-plan.md` §5): which kind of preview a file gets,
//! and the preview of each kind. Platform independent; the windows and processes are elsewhere.

pub mod kind;

pub use kind::{Kind, detect};
