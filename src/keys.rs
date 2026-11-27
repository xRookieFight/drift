//! Browser shortcuts. They are checked before a key reaches any webview, so pages cannot
//! swallow them.

use servo::{Code, KeyState, KeyboardEvent, Modifiers, NamedKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewTab,
    CloseTab,
    FocusAddress,
    NextTab,
    PrevTab,
    ToggleSplit,
    Back,
    Forward,
    Reload,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Fullscreen,
    ReopenTab,
    /// Ctrl+1 to Ctrl+9, 9 meaning the last tab.
    GoToTab(usize),
    ShowHistory,
}

/// Matches on the physical key (`Code`), so shortcuts stay put on non-QWERTY layouts.
pub fn shortcut(event: &KeyboardEvent) -> Option<Action> {
    let e = &event.event;
    if e.state != KeyState::Down {
        return None;
    }
    let ctrl = e.modifiers.contains(Modifiers::CONTROL);
    let shift = e.modifiers.contains(Modifiers::SHIFT);
    let alt = e.modifiers.contains(Modifiers::ALT);

    let action = match (ctrl, shift, alt, e.code) {
        (true, false, false, Code::KeyT) => Action::NewTab,
        (true, false, false, Code::KeyW) => Action::CloseTab,
        (true, false, false, Code::KeyL) => Action::FocusAddress,
        (true, false, false, Code::KeyR) => Action::Reload,
        (true, false, false, Code::Tab) => Action::NextTab,
        (true, true, false, Code::Tab) => Action::PrevTab,
        (true, true, false, Code::KeyS) => Action::ToggleSplit,
        (true, true, false, Code::KeyT) => Action::ReopenTab,
        (true, false, false, Code::KeyH) => Action::ShowHistory,
        (true, false, false, Code::Digit1) => Action::GoToTab(1),
        (true, false, false, Code::Digit2) => Action::GoToTab(2),
        (true, false, false, Code::Digit3) => Action::GoToTab(3),
        (true, false, false, Code::Digit4) => Action::GoToTab(4),
        (true, false, false, Code::Digit5) => Action::GoToTab(5),
        (true, false, false, Code::Digit6) => Action::GoToTab(6),
        (true, false, false, Code::Digit7) => Action::GoToTab(7),
        (true, false, false, Code::Digit8) => Action::GoToTab(8),
        (true, false, false, Code::Digit9) => Action::GoToTab(9),
        (true, _, false, Code::Equal | Code::NumpadAdd) => Action::ZoomIn,
        (true, false, false, Code::Minus | Code::NumpadSubtract) => Action::ZoomOut,
        (true, false, false, Code::Digit0 | Code::Numpad0) => Action::ZoomReset,
        (false, false, true, Code::ArrowLeft) => Action::Back,
        (false, false, true, Code::ArrowRight) => Action::Forward,
        _ => match e.key {
            servo::Key::Named(NamedKey::F5) if !ctrl && !alt => Action::Reload,
            servo::Key::Named(NamedKey::F6) if !ctrl && !alt => Action::FocusAddress,
            servo::Key::Named(NamedKey::F11) if !ctrl && !alt => Action::Fullscreen,
            servo::Key::Named(NamedKey::BrowserBack) => Action::Back,
            servo::Key::Named(NamedKey::BrowserForward) => Action::Forward,
            _ => return None,
        },
    };
    Some(action)
}

#[cfg(test)]
mod tests {
    use servo::{Key, Location};

    use super::*;

    fn ev(code: Code, key: Key, mods: Modifiers, state: KeyState) -> KeyboardEvent {
        KeyboardEvent::new_without_event(state, key, code, Location::Standard, mods, false, false)
    }

    fn down(code: Code, mods: Modifiers) -> KeyboardEvent {
        ev(code, Key::Character("x".into()), mods, KeyState::Down)
    }

    #[test]
    fn basic_shortcuts() {
        assert_eq!(
            shortcut(&down(Code::KeyT, Modifiers::CONTROL)),
            Some(Action::NewTab)
        );
        assert_eq!(
            shortcut(&down(Code::KeyW, Modifiers::CONTROL)),
            Some(Action::CloseTab)
        );
        assert_eq!(
            shortcut(&down(Code::KeyS, Modifiers::CONTROL | Modifiers::SHIFT)),
            Some(Action::ToggleSplit)
        );
        assert_eq!(
            shortcut(&down(Code::Tab, Modifiers::CONTROL | Modifiers::SHIFT)),
            Some(Action::PrevTab)
        );
        assert_eq!(
            shortcut(&down(Code::ArrowLeft, Modifiers::ALT)),
            Some(Action::Back)
        );
    }

    #[test]
    fn tab_shortcuts() {
        assert_eq!(
            shortcut(&down(Code::KeyT, Modifiers::CONTROL | Modifiers::SHIFT)),
            Some(Action::ReopenTab)
        );
        assert_eq!(
            shortcut(&down(Code::Digit3, Modifiers::CONTROL)),
            Some(Action::GoToTab(3))
        );
        assert_eq!(
            shortcut(&down(Code::KeyH, Modifiers::CONTROL)),
            Some(Action::ShowHistory)
        );
    }

    #[test]
    fn plain_typing_passes_through() {
        assert_eq!(shortcut(&down(Code::KeyT, Modifiers::empty())), None);
        assert_eq!(shortcut(&down(Code::KeyS, Modifiers::CONTROL)), None);
        assert_eq!(shortcut(&down(Code::ArrowLeft, Modifiers::empty())), None);
    }

    #[test]
    fn key_up_is_ignored() {
        let e = ev(
            Code::KeyT,
            Key::Character("t".into()),
            Modifiers::CONTROL,
            KeyState::Up,
        );
        assert_eq!(shortcut(&e), None);
    }

    #[test]
    fn function_keys() {
        let e = ev(
            Code::F5,
            Key::Named(NamedKey::F5),
            Modifiers::empty(),
            KeyState::Down,
        );
        assert_eq!(shortcut(&e), Some(Action::Reload));
    }
}
