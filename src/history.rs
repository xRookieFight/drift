//! Browsing history: what was visited, how often and when. Feeds the history page, the
//! suggestions under the address field and inline site name completion.
//!
//! Stored as one JSON file. A few thousand entries load and save in milliseconds, which is
//! all a single profile needs, so no database.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use url::Url;

/// Oldest, least visited entries go once the history grows past this.
const MAX_ENTRIES: usize = 10_000;
const DAY: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub url: String,
    pub title: String,
    pub visits: u32,
    /// Unix seconds.
    pub last_visit: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Suggestion {
    pub url: String,
    pub title: String,
    /// history or search, the UI picks an icon from it
    pub kind: &'static str,
}

#[derive(Default)]
pub struct History {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    dirty: bool,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Pages worth remembering: real web pages, not our own UI or blank tabs.
fn recordable(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://")
}

/// `https://www.github.com/x` -> `github.com/x`, what people actually type.
fn bare(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.strip_prefix("www.").unwrap_or(rest)
}

impl History {
    pub fn load(path: &Path) -> Self {
        let entries: Vec<Entry> = fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut history = History::default();
        for entry in entries {
            history
                .index
                .insert(entry.url.clone(), history.entries.len());
            history.entries.push(entry);
        }
        history
    }

    pub fn save(&mut self, path: &Path) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(&self.entries)?)?;
        fs::rename(tmp, path)?;
        self.dirty = false;
        Ok(())
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Counts a visit. Call once per finished page load.
    pub fn visit(&mut self, url: &str, title: &str, at: u64) {
        if !recordable(url) {
            return;
        }
        match self.index.get(url) {
            Some(&i) => {
                let entry = &mut self.entries[i];
                entry.visits = entry.visits.saturating_add(1);
                entry.last_visit = at;
                if !title.is_empty() {
                    entry.title = title.to_owned();
                }
            }
            None => {
                self.index.insert(url.to_owned(), self.entries.len());
                self.entries.push(Entry {
                    url: url.to_owned(),
                    title: title.to_owned(),
                    visits: 1,
                    last_visit: at,
                });
                if self.entries.len() > MAX_ENTRIES {
                    self.trim(at);
                }
            }
        }
        self.dirty = true;
    }

    /// Titles often arrive after the load finished, this fills them in without counting a visit.
    pub fn set_title(&mut self, url: &str, title: &str) {
        if let Some(&i) = self.index.get(url)
            && !title.is_empty()
            && self.entries[i].title != title
        {
            self.entries[i].title = title.to_owned();
            self.dirty = true;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.dirty = true;
    }

    pub fn remove(&mut self, url: &str) {
        if self.index.remove(url).is_some() {
            self.entries.retain(|e| e.url != url);
            self.reindex();
            self.dirty = true;
        }
    }

    fn reindex(&mut self) {
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.url.clone(), i))
            .collect();
    }

    fn trim(&mut self, now: u64) {
        self.entries
            .sort_by(|a, b| frecency(b, now).total_cmp(&frecency(a, now)));
        self.entries.truncate(MAX_ENTRIES * 9 / 10);
        self.reindex();
    }

    /// Newest first, for the history page.
    pub fn recent(&self, limit: usize) -> Vec<Entry> {
        let mut list = self.entries.clone();
        list.sort_by_key(|e| std::cmp::Reverse(e.last_visit));
        list.truncate(limit);
        list
    }

    /// What to offer under the address field for `query`. An empty query lists the most
    /// visited pages, which is what the panel shows when it opens.
    pub fn suggest(&self, query: &str, limit: usize, now: u64) -> Vec<Suggestion> {
        let query = query.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut scored: Vec<(f64, &Entry)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let url = bare(&e.url).to_lowercase();
                let title = e.title.to_lowercase();
                let all_match = words.iter().all(|w| url.contains(w) || title.contains(w));
                if !all_match {
                    return None;
                }
                // pages whose address starts with what was typed float to the top
                let boost = if !query.is_empty() && url.starts_with(&query) {
                    4.0
                } else {
                    1.0
                };
                Some((frecency(e, now) * boost, e))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, e)| Suggestion {
                url: e.url.clone(),
                title: e.title.clone(),
                kind: "history",
            })
            .collect()
    }

    /// Inline completion for the address field: if `typed` is the start of a site the user
    /// visits, returns the full `host/` it should grow into. Only whole hosts, completing
    /// into the middle of a path surprises people.
    pub fn complete(&self, typed: &str, now: u64) -> Option<String> {
        let typed = typed.trim().to_lowercase();
        if typed.is_empty() || typed.contains(char::is_whitespace) || typed.contains('/') {
            return None;
        }
        let mut hosts: HashMap<String, f64> = HashMap::new();
        for e in &self.entries {
            let Ok(url) = Url::parse(&e.url) else {
                continue;
            };
            let Some(host) = url.host_str() else { continue };
            let host = host.strip_prefix("www.").unwrap_or(host).to_lowercase();
            if host.starts_with(&typed) && host != typed {
                *hosts.entry(host).or_default() += frecency(e, now);
            }
        }
        hosts
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.len().cmp(&a.0.len())))
            .map(|(host, _)| format!("{host}/"))
    }
}

/// Visits weighted by how recent the last one was, the usual browser recipe.
fn frecency(e: &Entry, now: u64) -> f64 {
    let age_days = now.saturating_sub(e.last_visit) / DAY;
    let recency = match age_days {
        0..=3 => 1.0,
        4..=14 => 0.7,
        15..=31 => 0.5,
        32..=90 => 0.3,
        _ => 0.1,
    };
    e.visits as f64 * recency
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn sample() -> History {
        let mut h = History::default();
        for _ in 0..5 {
            h.visit("https://github.com/servo/servo", "servo/servo", NOW);
        }
        h.visit("https://www.github.com/", "GitHub", NOW);
        h.visit("https://gitlab.com/", "GitLab", NOW - 200 * DAY);
        h.visit("https://servo.org/", "Servo", NOW);
        h.visit(
            "https://en.wikipedia.org/wiki/Web_browser",
            "Web browser - Wikipedia",
            NOW,
        );
        h
    }

    #[test]
    fn visits_count_and_skip_internal_pages() {
        let mut h = sample();
        h.visit("drift://ui/newtab.html", "New Tab", NOW);
        h.visit("about:blank", "", NOW);
        assert_eq!(h.len(), 5);
        assert_eq!(
            h.recent(10)
                .iter()
                .find(|e| e.url.contains("servo/servo"))
                .unwrap()
                .visits,
            5
        );
    }

    #[test]
    fn completes_frequent_hosts() {
        let h = sample();
        assert_eq!(h.complete("git", NOW).as_deref(), Some("github.com/"));
        assert_eq!(h.complete("gitl", NOW).as_deref(), Some("gitlab.com/"));
        assert_eq!(h.complete("ser", NOW).as_deref(), Some("servo.org/"));
        assert_eq!(h.complete("github.com", NOW), None);
        assert_eq!(h.complete("two words", NOW), None);
        assert_eq!(h.complete("", NOW), None);
    }

    #[test]
    fn suggestions_match_url_and_title_words() {
        let h = sample();
        let s = h.suggest("web brow", 5, NOW);
        assert_eq!(s[0].url, "https://en.wikipedia.org/wiki/Web_browser");
        let s = h.suggest("git", 5, NOW);
        assert_eq!(s[0].url, "https://github.com/servo/servo");
        assert!(h.suggest("nothing-like-this", 5, NOW).is_empty());
        assert_eq!(h.suggest("", 2, NOW).len(), 2);
    }

    #[test]
    fn titles_fill_in_later() {
        let mut h = History::default();
        h.visit("https://a.example/", "", NOW);
        h.set_title("https://a.example/", "A");
        assert_eq!(h.recent(1)[0].title, "A");
        assert_eq!(h.recent(1)[0].visits, 1);
    }

    #[test]
    fn remove_and_clear() {
        let mut h = sample();
        h.remove("https://servo.org/");
        assert_eq!(h.len(), 4);
        assert!(h.suggest("servo.org", 5, NOW).is_empty());
        // the index must still point at the right entries after a removal
        h.visit("https://gitlab.com/", "GitLab", NOW);
        assert_eq!(
            h.recent(10)
                .iter()
                .find(|e| e.url == "https://gitlab.com/")
                .unwrap()
                .visits,
            2
        );
        h.clear();
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("drift-history-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = sample();
        h.save(&path).unwrap();
        let loaded = History::load(&path);
        assert_eq!(loaded.len(), 5);
        assert_eq!(loaded.complete("git", NOW).as_deref(), Some("github.com/"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn trims_when_full() {
        let mut h = History::default();
        for i in 0..MAX_ENTRIES + 1 {
            h.visit(&format!("https://site{i}.example/"), "", NOW - i as u64);
        }
        assert!(h.len() <= MAX_ENTRIES);
        // lookups still work after the trim reshuffled everything
        h.visit("https://site0.example/", "", NOW);
        assert_eq!(h.recent(1)[0].url, "https://site0.example/");
    }
}
