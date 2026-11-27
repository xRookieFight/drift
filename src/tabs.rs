//! Tab list, workspaces, essentials, selection and split view state. Knows nothing about
//! Servo, the app keeps the WebView for each tab in a map keyed by TabId and creates them
//! lazily when a tab is shown.
//!
//! Every tab belongs to a workspace and only the current workspace's tabs are in view.
//! Pinned tabs are the exception: they are "essentials", shown in every workspace.

use std::collections::HashMap;

use serde::Serialize;

use crate::layout::clamp_ratio;

pub type TabId = u64;
pub type WorkspaceId = u64;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// data: URL of the favicon, ready for an <img src>.
    pub favicon: Option<String>,
    /// Essentials: shown as tiles above the tab list, in every workspace.
    pub pinned: bool,
    pub workspace: WorkspaceId,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    /// Name of one of the UI's workspace icons (ui/app.js WORKSPACE_ICONS). Unknown names
    /// fall back to the workspace's first letter.
    pub icon: String,
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

#[derive(Debug)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    split: Option<Split>,
    next_id: TabId,
    workspaces: Vec<Workspace>,
    current: WorkspaceId,
    /// What each workspace had in front when we left it.
    remembered: HashMap<WorkspaceId, TabId>,
}

impl Default for Tabs {
    fn default() -> Self {
        Tabs {
            tabs: Vec::new(),
            active: None,
            split: None,
            next_id: 0,
            workspaces: vec![Workspace {
                id: 1,
                name: "Home".into(),
                icon: "home".into(),
            }],
            current: 1,
            remembered: HashMap::new(),
        }
    }
}

impl Tabs {
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn current_workspace(&self) -> WorkspaceId {
        self.current
    }

    fn in_view(&self, tab: &Tab) -> bool {
        tab.pinned || tab.workspace == self.current
    }

    /// Ids of the tabs the sidebar shows right now, in list order.
    pub fn in_view_ids(&self) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|t| self.in_view(t))
            .map(|t| t.id)
            .collect()
    }

    /// Whether the current workspace has nothing open (essentials don't count).
    #[cfg(test)]
    pub fn workspace_is_empty(&self) -> bool {
        !self
            .tabs
            .iter()
            .any(|t| !t.pinned && t.workspace == self.current)
    }
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
            pinned: false,
            workspace: self.current,
        };
        // right after the focused tab, unless that is an essential: new tabs never land in
        // the essentials block
        let after_focused = self
            .focused()
            .and_then(|f| self.get(f).filter(|t| !t.pinned).map(|_| f))
            .and_then(|f| self.index_of(f))
            .map(|i| i + 1);
        let at = after_focused.unwrap_or(self.tabs.len());
        self.tabs.insert(at, tab);
        id
    }

    /// Shows a tab. Picking one half of the split just moves focus there, picking any other
    /// tab leaves split view, the same way Zen does it.
    pub fn activate(&mut self, id: TabId) -> bool {
        let Some(tab) = self.get(id) else {
            return false;
        };
        // a tab from another workspace (say, reopened or found in history) takes us there
        if !tab.pinned && tab.workspace != self.current {
            let ws = tab.workspace;
            self.leave_workspace();
            self.current = ws;
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
        // neighbours are judged by what the sidebar shows, not the hidden workspaces around
        let view = self.in_view_ids();
        let pos = view.iter().position(|v| *v == id);
        let neighbour = pos
            .and_then(|p| {
                view.get(p + 1)
                    .or_else(|| p.checked_sub(1).and_then(|q| view.get(q)))
            })
            .copied();
        self.remembered.retain(|_, t| *t != id);

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
        if was_focused && self.split.is_none() && self.active == Some(id) {
            self.active = neighbour;
        }
        Some(tab)
    }

    /// Ctrl+Tab and friends. Wraps around.
    pub fn cycle(&mut self, forward: bool) {
        let view = self.in_view_ids();
        let Some(current) = self
            .focused()
            .and_then(|f| view.iter().position(|v| *v == f))
        else {
            return;
        };
        let n = view.len();
        let next = if forward {
            (current + 1) % n
        } else {
            (current + n - 1) % n
        };
        self.activate(view[next]);
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
        let view = self.in_view_ids();
        let i = view.iter().position(|v| *v == focused)?;
        view.get(i + 1)
            .or_else(|| i.checked_sub(1).and_then(|p| view.get(p)))
            .copied()
    }

    // ---- workspaces -----------------------------------------------------------------------

    fn leave_workspace(&mut self) {
        self.split = None;
        if let Some(active) = self.active
            && self.get(active).is_some_and(|t| !t.pinned)
        {
            self.remembered.insert(self.current, active);
        }
        self.active = None;
    }

    /// Switches workspace and brings back what was in front there. Returns false for an
    /// unknown id. The workspace may come back empty, the caller decides what to show then.
    pub fn switch_workspace(&mut self, id: WorkspaceId) -> bool {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return false;
        }
        if id == self.current {
            return true;
        }
        self.leave_workspace();
        self.current = id;
        let back = self
            .remembered
            .get(&id)
            .copied()
            .filter(|t| self.get(*t).is_some())
            .or_else(|| {
                self.tabs
                    .iter()
                    .find(|t| !t.pinned && t.workspace == id)
                    .map(|t| t.id)
            });
        self.active = back;
        true
    }

    pub fn add_workspace(
        &mut self,
        name: impl Into<String>,
        icon: impl Into<String>,
    ) -> WorkspaceId {
        let id = self.workspaces.iter().map(|w| w.id).max().unwrap_or(0) + 1;
        self.workspaces.push(Workspace {
            id,
            name: name.into(),
            icon: icon.into(),
        });
        id
    }

    pub fn rename_workspace(&mut self, id: WorkspaceId, name: &str, icon: &str) {
        if let Some(w) = self.workspaces.iter_mut().find(|w| w.id == id) {
            if !name.trim().is_empty() {
                w.name = name.trim().to_owned();
            }
            if !icon.trim().is_empty() {
                w.icon = icon.trim().to_owned();
            }
        }
    }

    /// Deletes a workspace and returns the ids of its tabs, which are closed with it. The last
    /// workspace stays, there is always somewhere to be.
    pub fn remove_workspace(&mut self, id: WorkspaceId) -> Vec<TabId> {
        if self.workspaces.len() <= 1 || !self.workspaces.iter().any(|w| w.id == id) {
            return Vec::new();
        }
        let doomed: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| !t.pinned && t.workspace == id)
            .map(|t| t.id)
            .collect();
        if self.current == id {
            let other = self.workspaces.iter().find(|w| w.id != id).map(|w| w.id);
            if let Some(other) = other {
                self.switch_workspace(other);
            }
        }
        self.tabs.retain(|t| !doomed.contains(&t.id));
        self.workspaces.retain(|w| w.id != id);
        self.remembered.remove(&id);
        doomed
    }

    pub fn move_to_workspace(&mut self, tab: TabId, ws: WorkspaceId) {
        if !self.workspaces.iter().any(|w| w.id == ws) {
            return;
        }
        let leaving_view = ws != self.current && self.get(tab).is_some_and(|t| !t.pinned);
        if leaving_view && self.visible().contains(&tab) {
            let view = self.in_view_ids();
            let pos = view.iter().position(|v| *v == tab);
            let neighbour = pos
                .and_then(|p| {
                    view.get(p + 1)
                        .or_else(|| p.checked_sub(1).and_then(|q| view.get(q)))
                })
                .copied();
            self.split = None;
            self.active = neighbour;
        }
        if let Some(t) = self.get_mut(tab) {
            t.workspace = ws;
            t.pinned = false;
        }
    }

    // ---- essentials -----------------------------------------------------------------------

    /// Pins or unpins a tab. Essentials live in one block at the front of the list, so the
    /// sidebar can draw them as a grid; an unpinned tab joins the current workspace.
    pub fn set_pinned(&mut self, id: TabId, pinned: bool) {
        let Some(from) = self.index_of(id) else {
            return;
        };
        let mut tab = self.tabs.remove(from);
        tab.pinned = pinned;
        if !pinned {
            tab.workspace = self.current;
        }
        let block_end = self.tabs.iter().take_while(|t| t.pinned).count();
        self.tabs.insert(block_end, tab);
    }

    /// Restores workspaces from a session. Keeps the default if the list is empty.
    pub fn restore_workspaces(&mut self, workspaces: Vec<Workspace>, current: WorkspaceId) {
        if !workspaces.is_empty() {
            self.workspaces = workspaces;
        }
        if self.workspaces.iter().any(|w| w.id == current) {
            self.current = current;
        } else {
            self.current = self.workspaces[0].id;
        }
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

    /// Drag and drop in the sidebar. `to` is a position in the list the sidebar shows (this
    /// workspace's tabs, essentials excluded), not in the full list behind it.
    pub fn move_tab(&mut self, id: TabId, to: usize) {
        let Some(from) = self.index_of(id) else {
            return;
        };
        if self.tabs[from].pinned {
            return;
        }
        let tab = self.tabs.remove(from);
        let shown: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| !t.pinned && t.workspace == self.current)
            .map(|t| t.id)
            .collect();
        let at = match shown.get(to) {
            Some(before) => self.index_of(*before).unwrap_or(self.tabs.len()),
            None => shown
                .last()
                .and_then(|last| self.index_of(*last))
                .map_or(self.tabs.len(), |i| i + 1),
        };
        self.tabs.insert(at, tab);
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

    #[test]
    fn workspaces_keep_their_own_tabs() {
        let mut t = Tabs::default();
        let a = t.open("a");
        let work = t.add_workspace("Work", "briefcase");
        assert!(t.switch_workspace(work));
        assert!(t.workspace_is_empty());
        assert_eq!(t.focused(), None);
        let w = t.open("w");
        assert_eq!(t.in_view_ids(), vec![w]);

        t.switch_workspace(1);
        assert_eq!(t.focused(), Some(a));
        assert_eq!(t.in_view_ids(), vec![a]);
        t.switch_workspace(work);
        assert_eq!(t.focused(), Some(w));
    }

    #[test]
    fn essentials_show_everywhere_and_stay_in_front() {
        let mut t = Tabs::default();
        let a = t.open("a");
        let b = t.open("b");
        t.set_pinned(b, true);
        assert_eq!(t.all()[0].id, b);
        let work = t.add_workspace("Work", "briefcase");
        t.switch_workspace(work);
        assert_eq!(t.in_view_ids(), vec![b]);
        // a new tab opened while an essential is focused goes after the block, not into it
        t.activate(b);
        let c = t.open("c");
        assert!(!t.get(c).unwrap().pinned);
        assert_eq!(t.get(c).unwrap().workspace, work);
        t.set_pinned(b, false);
        assert_eq!(t.get(b).unwrap().workspace, work);
        let _ = a;
    }

    #[test]
    fn activating_a_tab_elsewhere_switches_workspace() {
        let mut t = Tabs::default();
        let a = t.open("a");
        let work = t.add_workspace("Work", "briefcase");
        t.switch_workspace(work);
        t.activate(a);
        assert_eq!(t.current_workspace(), 1);
        assert_eq!(t.focused(), Some(a));
    }

    #[test]
    fn closing_last_tab_in_workspace_leaves_nothing_focused() {
        let mut t = Tabs::default();
        let _a = t.open("a");
        let work = t.add_workspace("Work", "briefcase");
        t.switch_workspace(work);
        let w = t.open("w");
        t.close(w);
        // the other workspace's tab must not jump in here
        assert_eq!(t.focused(), None);
        assert!(t.workspace_is_empty());
    }

    #[test]
    fn removing_a_workspace_closes_its_tabs() {
        let mut t = Tabs::default();
        let a = t.open("a");
        let work = t.add_workspace("Work", "briefcase");
        t.switch_workspace(work);
        let w = t.open("w");
        assert_eq!(t.remove_workspace(work), vec![w]);
        assert_eq!(t.current_workspace(), 1);
        assert_eq!(t.focused(), Some(a));
        // the last one stays
        assert!(t.remove_workspace(1).is_empty());
        assert_eq!(t.workspaces().len(), 1);
    }

    #[test]
    fn move_to_workspace_hands_focus_to_a_neighbour() {
        let mut t = Tabs::default();
        let a = t.open("a");
        let b = t.open("b");
        let work = t.add_workspace("Work", "briefcase");
        t.move_to_workspace(b, work);
        assert_eq!(t.focused(), Some(a));
        assert_eq!(t.in_view_ids(), vec![a]);
    }

    #[test]
    fn move_tab_counts_only_what_the_sidebar_shows() {
        let mut t = Tabs::default();
        let a = t.open("a");
        let work = t.add_workspace("Work", "briefcase");
        t.switch_workspace(work);
        let x = t.open("x");
        let y = t.open("y");
        t.switch_workspace(1);
        let b = t.open("b");
        let c = t.open("c");
        // c to the top of this workspace's list, past nothing from Work
        t.move_tab(c, 0);
        assert_eq!(t.in_view_ids(), vec![c, a, b]);
        t.move_tab(c, 9);
        assert_eq!(t.in_view_ids(), vec![a, b, c]);
        t.switch_workspace(work);
        assert_eq!(t.in_view_ids(), vec![x, y]);
    }
}
