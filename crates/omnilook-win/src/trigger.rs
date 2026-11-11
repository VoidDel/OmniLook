//! What a key press means for the preview (`docs/development-plan.md` §4.1). Pure, so the rules
//! are tested without a keyboard: the hook gathers a [`KeyContext`] and acts on the [`Decision`].

/// The preview hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hotkey {
    /// Ctrl+Space (default).
    #[default]
    CtrlSpace,
    /// Space alone.
    Space,
}

/// The window that has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Explorer,
    Desktop,
    Other,
}

/// A key the hook cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Space,
    Escape,
    /// Arrow keys, Home / End, Page Up / Down: they move Explorer's selection.
    Move,
    /// Letters and digits: Explorer's type-to-select.
    Typing,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyContext {
    pub surface: Surface,
    /// The focus is in the file list (not the address bar, search box or a rename box).
    pub in_file_view: bool,
    pub mods: Modifiers,
    /// Milliseconds since the last letter or digit typed into Explorer.
    pub since_typing_ms: Option<u64>,
    pub preview_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Let the key through untouched.
    Pass,
    /// Swallow the key and open the preview, or close it when open.
    Toggle,
    /// Swallow the key and close the preview.
    Close,
    /// Let the key through; the selection may change, so look again.
    Follow,
}

/// Type-to-select uses Space inside names; Space alone waits this long after typing.
pub const TYPING_PAUSE_MS: u64 = 1000;

pub fn decide(hotkey: Hotkey, key: Key, cx: &KeyContext) -> Decision {
    if cx.surface == Surface::Other || !cx.in_file_view {
        return Decision::Pass;
    }
    match key {
        Key::Space => {
            let m = cx.mods;
            let mods_match = match hotkey {
                Hotkey::CtrlSpace => m.ctrl && !m.alt && !m.shift && !m.win,
                Hotkey::Space => !m.ctrl && !m.alt && !m.shift && !m.win,
            };
            let typing = hotkey == Hotkey::Space
                && !cx.preview_open
                && cx.since_typing_ms.is_some_and(|t| t < TYPING_PAUSE_MS);
            if mods_match && !typing {
                Decision::Toggle
            } else {
                Decision::Pass
            }
        }
        Key::Escape if cx.preview_open => Decision::Close,
        Key::Move if cx.preview_open => Decision::Follow,
        _ => Decision::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx(surface: Surface, in_file_view: bool, ctrl: bool) -> KeyContext {
        KeyContext {
            surface,
            in_file_view,
            mods: Modifiers {
                ctrl,
                ..Default::default()
            },
            since_typing_ms: None,
            preview_open: false,
        }
    }

    #[test]
    fn ctrl_space_only_in_file_views() {
        let h = Hotkey::CtrlSpace;
        assert_eq!(
            decide(h, Key::Space, &cx(Surface::Explorer, true, true)),
            Decision::Toggle
        );
        assert_eq!(
            decide(h, Key::Space, &cx(Surface::Desktop, true, true)),
            Decision::Toggle
        );
        // Plain Space, the address bar or a rename box, other programs (the IME's Ctrl+Space).
        assert_eq!(
            decide(h, Key::Space, &cx(Surface::Explorer, true, false)),
            Decision::Pass
        );
        assert_eq!(
            decide(h, Key::Space, &cx(Surface::Explorer, false, true)),
            Decision::Pass
        );
        assert_eq!(
            decide(h, Key::Space, &cx(Surface::Other, true, true)),
            Decision::Pass
        );
        let mut c = cx(Surface::Explorer, true, true);
        c.mods.shift = true;
        assert_eq!(decide(h, Key::Space, &c), Decision::Pass);
    }

    #[test]
    fn space_alone_waits_after_typing() {
        let h = Hotkey::Space;
        let mut c = cx(Surface::Explorer, true, false);
        assert_eq!(decide(h, Key::Space, &c), Decision::Toggle);
        c.since_typing_ms = Some(300);
        assert_eq!(decide(h, Key::Space, &c), Decision::Pass, "type-to-select");
        c.since_typing_ms = Some(TYPING_PAUSE_MS + 1);
        assert_eq!(decide(h, Key::Space, &c), Decision::Toggle);
        // Closing never waits.
        c.since_typing_ms = Some(10);
        c.preview_open = true;
        assert_eq!(decide(h, Key::Space, &c), Decision::Toggle);
        c.mods.ctrl = true;
        c.preview_open = false;
        c.since_typing_ms = None;
        assert_eq!(decide(h, Key::Space, &c), Decision::Pass, "Ctrl+Space toggles selection");
    }

    #[test]
    fn escape_and_moves_only_while_open() {
        let h = Hotkey::CtrlSpace;
        let mut c = cx(Surface::Explorer, true, false);
        assert_eq!(decide(h, Key::Escape, &c), Decision::Pass);
        assert_eq!(decide(h, Key::Move, &c), Decision::Pass);
        c.preview_open = true;
        assert_eq!(decide(h, Key::Escape, &c), Decision::Close);
        assert_eq!(decide(h, Key::Move, &c), Decision::Follow);
        // Escape in a rename box cancels the rename, not the preview.
        c.in_file_view = false;
        assert_eq!(decide(h, Key::Escape, &c), Decision::Pass);
    }
}
