//! Saves open tabs and split state on quit and brings them back on the next start.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tabs::Tabs;

const VERSION: u32 = 1;

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub tabs: Vec<SavedTab>,
    pub active: usize,
    pub split: Option<SavedSplit>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSplit {
    pub left: usize,
    pub right: usize,
    pub ratio: f32,
}

impl Session {
    pub fn capture(tabs: &Tabs) -> Self {
        let pos = |id| tabs.position(id);
        Session {
            version: VERSION,
            tabs: tabs
                .all()
                .iter()
                .map(|t| SavedTab {
                    url: t.url.clone(),
                    title: t.title.clone(),
                })
                .collect(),
            active: tabs.focused().and_then(pos).unwrap_or(0),
            split: tabs.split().and_then(|s| {
                Some(SavedSplit {
                    left: pos(s.left)?,
                    right: pos(s.right)?,
                    ratio: s.ratio,
                })
            }),
        }
    }

    /// Rebuilds the tab list. Webviews are created later, only for tabs that get shown.
    pub fn restore(&self) -> Tabs {
        let mut tabs = Tabs::default();
        let mut ids = Vec::with_capacity(self.tabs.len());
        for saved in &self.tabs {
            // open_in_background inserts after the focused tab, nothing is focused yet so it appends
            let id = tabs.open_in_background(saved.url.clone());
            if let Some(tab) = tabs.get_mut(id) {
                tab.title = saved.title.clone();
            }
            ids.push(id);
        }
        if let Some(&id) = ids.get(self.active).or(ids.first()) {
            tabs.activate(id);
        }
        if let Some(split) = &self.split {
            tabs.restore_split(split.left, split.right, split.ratio);
        }
        tabs
    }

    pub fn load(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<Session>(&text) {
            Ok(session) if session.version == VERSION && !session.tabs.is_empty() => Some(session),
            Ok(_) => None,
            Err(err) => {
                log::warn!("ignoring unreadable session file {}: {err}", path.display());
                None
            }
        }
    }

    /// Writes next to the target and renames, so a crash mid write never eats the session.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("drift").join("session.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Tabs {
        let mut t = Tabs::default();
        let a = t.open("https://a.example/");
        let _b = t.open("https://b.example/");
        let c = t.open("https://c.example/");
        t.get_mut(c).unwrap().title = "C".into();
        t.activate(a);
        t.split_with(c);
        t.set_ratio(0.35);
        t
    }

    #[test]
    fn capture_restore_round_trip() {
        let tabs = sample();
        let session = Session::capture(&tabs);
        assert_eq!(session.tabs.len(), 3);
        let split = session.split.as_ref().unwrap();
        assert_eq!((split.left, split.right), (0, 2));

        let restored = session.restore();
        let urls: Vec<_> = restored.all().iter().map(|t| t.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://a.example/",
                "https://b.example/",
                "https://c.example/"
            ]
        );
        let s = restored.split().unwrap();
        assert_eq!(restored.position(s.left), Some(0));
        assert_eq!(restored.position(s.right), Some(2));
        assert!((s.ratio - 0.35).abs() < 1e-6);
        assert_eq!(restored.get(s.right).unwrap().title, "C");
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("drift-session-test-{}", std::process::id()));
        let path = dir.join("session.json");
        let session = Session::capture(&sample());
        session.save(&path).unwrap();
        assert_eq!(Session::load(&path), Some(session));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn garbage_and_empty_sessions_are_ignored() {
        let dir = std::env::temp_dir().join(format!("drift-session-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Session::load(&path), None);
        fs::write(&path, r#"{"version":1,"tabs":[],"active":0,"split":null}"#).unwrap();
        assert_eq!(Session::load(&path), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn out_of_range_indices_are_safe() {
        let session = Session {
            version: VERSION,
            tabs: vec![SavedTab {
                url: "https://a.example/".into(),
                title: String::new(),
            }],
            active: 7,
            split: Some(SavedSplit {
                left: 0,
                right: 5,
                ratio: 0.5,
            }),
        };
        let tabs = session.restore();
        assert_eq!(tabs.len(), 1);
        assert!(tabs.split().is_none());
        assert!(tabs.focused().is_some());
    }
}
