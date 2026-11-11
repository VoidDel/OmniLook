//! Windows integration for Omni Look: the preview hotkey and the Explorer / desktop selection
//! (`docs/development-plan.md` §4).

pub mod foreground;
pub mod trigger;

#[cfg(windows)]
pub mod events;
#[cfg(windows)]
pub mod explorer;
#[cfg(windows)]
pub mod hook;
#[cfg(windows)]
pub mod pipe;
