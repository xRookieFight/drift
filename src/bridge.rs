//! Messages between the sidebar UI and the core.
//!
//! UI to core: `fetch("drift://ui/bridge?" + encodeURIComponent(JSON.stringify(cmd)))`, where
//! `cmd` is one of the [`Command`] shapes, e.g. `{"cmd":"activate","id":3}`.
//! Core to UI: `drift.update(state)` with a [`UiState`], evaluated in the UI webview.

use serde::{Deserialize, Serialize};

use crate::dialogs::DialogView;
use crate::layout::CssRect;
use crate::tabs::{Side, Tab, TabId};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// UI finished loading and wants the first state push.
    Ready,
    NewTab,
    Close {
        id: TabId,
    },
    Activate {
        id: TabId,
    },
    Navigate {
        input: String,
    },
    Back,
    Forward,
    Reload,
    ToggleSplit,
    SplitWith {
        id: TabId,
    },
    SetRatio {
        ratio: f32,
    },
    FocusPane {
        side: Side,
    },
    MoveTab {
        id: TabId,
        to: usize,
    },
    /// Hand keyboard focus back to the focused page, e.g. after Escape in the URL field.
    FocusContent,
    /// Answer to a dialog or picker opened by a page, see [`crate::dialogs`].
    DialogResult {
        id: u64,
        ok: bool,
        #[serde(default)]
        value: Option<serde_json::Value>,
    },
}

impl Command {
    /// Parses the query part of a bridge URL (percent encoded JSON).
    pub fn from_query(query: &str) -> Result<Self, String> {
        let decoded = percent_decode(query);
        serde_json::from_str(&decoded).map_err(|e| e.to_string())
    }
}

fn percent_decode(s: &str) -> String {
    // form_urlencoded would turn '+' into spaces, encodeURIComponent never emits a bare '+'
    // for a space but does keep literal '+' as %2B, so a plain percent decode is the right one
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
pub struct SplitView {
    pub left: TabId,
    pub right: TabId,
    pub ratio: f32,
    pub focused: Side,
}

#[derive(Debug, Serialize)]
pub struct UiState<'a> {
    pub tabs: &'a [Tab],
    pub focused: Option<TabId>,
    pub split: Option<SplitView>,
    /// URL shown in the address field, empty for the new tab page.
    pub url: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub loading: bool,
    /// Pane rects in CSS pixels, so the UI can draw card frames exactly where pages are.
    pub panes: Vec<CssRect>,
    /// Content card rect in CSS pixels.
    pub card: CssRect,
    /// Page went fullscreen, the UI hides its chrome.
    pub immersive: bool,
    /// The dialog or picker the UI should show on top of everything, if any.
    pub dialog: Option<DialogView>,
}

impl UiState<'_> {
    pub fn to_script(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_else(|_| "null".into());
        format!("window.drift && window.drift.update({json})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(
            Command::from_query("%7B%22cmd%22%3A%22ready%22%7D"),
            Ok(Command::Ready)
        );
        assert_eq!(
            Command::from_query("%7B%22cmd%22%3A%22activate%22%2C%22id%22%3A3%7D"),
            Ok(Command::Activate { id: 3 })
        );
        assert_eq!(
            Command::from_query(r#"{"cmd":"focus_pane","side":"right"}"#),
            Ok(Command::FocusPane { side: Side::Right })
        );
    }

    #[test]
    fn keeps_plus_and_unicode() {
        let q = "%7B%22cmd%22%3A%22navigate%22%2C%22input%22%3A%22c%2B%2B%20%C3%A7ay%22%7D";
        assert_eq!(
            Command::from_query(q),
            Ok(Command::Navigate {
                input: "c++ çay".into()
            })
        );
    }

    #[test]
    fn rejects_unknown() {
        assert!(Command::from_query(r#"{"cmd":"rm_rf"}"#).is_err());
        assert!(Command::from_query("garbage").is_err());
        assert!(Command::from_query("%ZZ").is_err());
    }
}
