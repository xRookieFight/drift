//! Tab list, selection and split view state. Knows nothing about Servo, the app keeps the
//! WebView for each tab in a map keyed by TabId and creates them lazily when a tab is shown.

use serde::Serialize;

use crate::layout::clamp_ratio;

pub type TabId = u64;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// data: URL of the favicon, ready for an <img src>.
    pub favicon: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Split {
    pub left: TabId,
    pub right: TabId,
    pub ratio: f32,
    pub focused: Side,
}

impl Split {
    fn focused_tab(&self) -> TabId {
        match self.focused {
            Side::Left => self.left,
            Side::Right => self.right,
        }
    }

    fn contains(&self, id: TabId) -> bool {
        self.left == id || self.right == id
    }
}

#[derive(Debug, Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    split: Option<Split>,
    next_id: TabId,
}

impl Tabs {
    pub fn all(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn split(&self) -> Option<Split> {
        self.split
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// The tab that gets keyboard input and whose URL the sidebar shows.
    pub fn focused(&self) -> Option<TabId> {
        match self.split {
            Some(split) => Some(split.focused_tab()),
            None => self.active,
        }
    }

    /// Tabs on screen, left to right. One normally, two in split view.
    pub fn visible(&self) -> Vec<TabId> {
        match self.split {
            Some(split) => vec![split.left, split.right],
            None => self.active.into_iter().collect(),
        }
    }

    /// Adds a tab right after the focused one and focuses it.
    pub fn open(&mut self, url: impl Into<String>) -> TabId {
        let id = self.push_after_focused(url.into());
        self.activate(id);
        id
    }

    /// Adds a tab without touching what is on screen, for restores and background opens.
    pub fn open_in_background(&mut self, url: impl Into<String>) -> TabId {
        self.push_after_focused(url.into())
    }

    fn push_after_focused(&mut self, url: String) -> TabId {
        self.next_id += 1;
        let id = self.next_id;
        let tab = Tab {
            id,
            title: String::new(),
            url,
            loading: false,
            favicon: None,
        };
        let at = self
            .focused()
            .and_then(|f| self.index_of(f))
            .map(|i| i + 1)
            .unwrap_or(self.tabs.len());
        self.tabs.insert(at, tab);
        id
    }

    /// Shows a tab. Picking one half of the split just moves focus there, picking any other
    /// tab leaves split view, the same way Zen does it.
    pub fn activate(&mut self, id: TabId) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        if let Some(split) = &mut self.split {
            if split.contains(id) {
                split.focused = if split.left == id {
                    Side::Left
                } else {
                    Side::Right
                };
                self.active = Some(id);
                return true;
            }
            self.split = None;
        }
        self.active = Some(id);
        true
    }

    /// Closes a tab and returns it. Focus moves to the neighbour, like every other browser.
    pub fn close(&mut self, id: TabId) -> Option<Tab> {
        let index = self.index_of(id)?;
        let was_focused = self.focused() == Some(id);

        if let Some(split) = self.split.filter(|s| s.contains(id)) {
            let other = if split.left == id {
                split.right
            } else {
                split.left
            };
            self.split = None;
            self.active = Some(other);
        }

        let tab = self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = None;
        } else if was_focused && self.split.is_none() && self.active == Some(id) {
            let next = index.min(self.tabs.len() - 1);
            self.active = Some(self.tabs[next].id);
        }
        Some(tab)
    }

    /// Ctrl+Tab and friends. Wraps around.
    pub fn cycle(&mut self, forward: bool) {
        let Some(current) = self.focused().and_then(|f| self.index_of(f)) else {
            return;
        };
        let n = self.tabs.len();
        let next = if forward {
            (current + 1) % n
        } else {
            (current + n - 1) % n
        };
        let id = self.tabs[next].id;
        self.activate(id);
    }

    /// Puts `other` next to the focused tab. The focused tab stays on the left.
    pub fn split_with(&mut self, other: TabId) -> bool {
        let Some(focused) = self.focused() else {
            return false;
        };
        if focused == other || self.get(other).is_none() {
            return false;
        }
        let ratio = self.split.map(|s| s.ratio).unwrap_or(0.5);
        self.split = Some(Split {
            left: focused,
            right: other,
            ratio,
            focused: Side::Right,
        });
        self.active = Some(other);
        true
    }

    /// Leaves split view, keeping the focused pane on screen.
    pub fn unsplit(&mut self) {
        if let Some(split) = self.split.take() {
            self.active = Some(split.focused_tab());
        }
    }

    /// Best partner for a fresh split: the next tab, or the previous one at the end of the list.
    pub fn split_candidate(&self) -> Option<TabId> {
        let focused = self.focused()?;
        let i = self.index_of(focused)?;
        self.tabs
            .get(i + 1)
            .or_else(|| i.checked_sub(1).and_then(|p| self.tabs.get(p)))
            .map(|t| t.id)
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        if let Some(split) = &mut self.split {
            split.ratio = clamp_ratio(ratio);
        }
    }

    pub fn focus_side(&mut self, side: Side) {
        if let Some(split) = &mut self.split {
            split.focused = side;
            self.active = Some(split.focused_tab());
        }
    }

    /// Moves a tab to a new position in the list, for drag and drop in the sidebar.
    pub fn move_tab(&mut self, id: TabId, to: usize) {
        if let Some(from) = self.index_of(id) {
            let tab = self.tabs.remove(from);
            let to = to.min(self.tabs.len());
            self.tabs.insert(to, tab);
        }
    }

    /// Restores a split by list positions, used by the session loader.
    pub fn restore_split(&mut self, left: usize, right: usize, ratio: f32) {
        let (Some(l), Some(r)) = (self.tabs.get(left), self.tabs.get(right)) else {
            return;
        };
        if l.id == r.id {
            return;
        }
        self.split = Some(Split {
            left: l.id,
            right: r.id,
            ratio: clamp_ratio(ratio),
            focused: Side::Left,
        });
        self.active = Some(l.id);
    }

    pub fn position(&self, id: TabId) -> Option<usize> {
        self.index_of(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three() -> (Tabs, TabId, TabId, TabId) {
        let mut t = Tabs::default();
        let a = t.open("a");
        let b = t.open("b");
        let c = t.open("c");
        (t, a, b, c)
    }

    #[test]
    fn open_inserts_after_focused() {
        let (mut t, a, b, c) = three();
        t.activate(a);
        let d = t.open("d");
        let ids: Vec<_> = t.all().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![a, d, b, c]);
        assert_eq!(t.focused(), Some(d));
    }

    #[test]
    fn close_moves_focus_to_neighbour() {
        let (mut t, a, b, c) = three();
        t.activate(b);
        t.close(b);
        assert_eq!(t.focused(), Some(c));
        t.close(c);
        assert_eq!(t.focused(), Some(a));
        t.close(a);
        assert_eq!(t.focused(), None);
        assert!(t.is_empty());
    }

    #[test]
    fn closing_background_tab_keeps_focus() {
        let (mut t, a, _b, c) = three();
        t.activate(c);
        t.close(a);
        assert_eq!(t.focused(), Some(c));
    }

    #[test]
    fn split_and_focus() {
        let (mut t, a, b, _c) = three();
        t.activate(a);
        assert!(t.split_with(b));
        assert_eq!(t.visible(), vec![a, b]);
        assert_eq!(t.focused(), Some(b));
        t.activate(a);
        assert_eq!(t.focused(), Some(a));
        assert!(t.split().is_some());
    }

    #[test]
    fn activating_outside_split_leaves_it() {
        let (mut t, a, b, c) = three();
        t.activate(a);
        t.split_with(b);
        t.activate(c);
        assert!(t.split().is_none());
        assert_eq!(t.visible(), vec![c]);
    }

    #[test]
    fn closing_split_half_keeps_other() {
        let (mut t, a, b, _c) = three();
        t.activate(a);
        t.split_with(b);
        t.close(b);
        assert!(t.split().is_none());
        assert_eq!(t.focused(), Some(a));
    }

    #[test]
    fn unsplit_keeps_focused_pane() {
        let (mut t, a, b, _c) = three();
        t.activate(a);
        t.split_with(b);
        t.focus_side(Side::Left);
        t.unsplit();
        assert_eq!(t.visible(), vec![a]);
    }

    #[test]
    fn cannot_split_with_self_or_unknown() {
        let (mut t, a, _b, _c) = three();
        t.activate(a);
        assert!(!t.split_with(a));
        assert!(!t.split_with(999));
    }

    #[test]
    fn cycle_wraps() {
        let (mut t, a, _b, c) = three();
        t.activate(c);
        t.cycle(true);
        assert_eq!(t.focused(), Some(a));
        t.cycle(false);
        assert_eq!(t.focused(), Some(c));
    }

    #[test]
    fn split_candidate_prefers_next() {
        let (mut t, a, b, c) = three();
        t.activate(a);
        assert_eq!(t.split_candidate(), Some(b));
        t.activate(c);
        assert_eq!(t.split_candidate(), Some(b));
    }

    #[test]
    fn move_tab_reorders() {
        let (mut t, a, b, c) = three();
        t.move_tab(c, 0);
        let ids: Vec<_> = t.all().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![c, a, b]);
        t.move_tab(c, 99);
        let ids: Vec<_> = t.all().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![a, b, c]);
    }
}
