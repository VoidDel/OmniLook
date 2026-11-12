//! Omni Look's previewers (`docs/development-plan.md` §5): which kind of preview a file gets,
//! and the preview of each kind. Platform independent; the windows and processes are elsewhere.

pub mod archive;
pub mod audio;
pub mod folder;
pub mod font;
pub mod hex;
pub mod image;
pub mod kind;
pub mod markdown;
pub mod office;
pub mod records;
pub mod text;

pub use kind::{Kind, detect};
