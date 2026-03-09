use url::Url;

use super::tab::{Tab, TabId};

/// Manages the collection of open tabs and tracks which one is active.
#[derive(Debug)]
pub struct TabManager {
    tabs: Vec<Tab>,
    active_index: usize,
}

impl TabManager {
    /// Create an empty tab manager with no tabs.
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_index: 0,
        }
    }

    /// Number of open tabs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether there are no open tabs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Add a new tab at the end and make it active. Returns the new tab's id.
    pub fn add_tab(&mut self, url: Url) -> TabId {
        let tab = Tab::new(url);
        let id = tab.id;
        self.tabs.push(tab);
        self.active_index = self.tabs.len() - 1;
        id
    }

    /// Close the tab with the given id. If the active tab is closed, the
    /// adjacent tab becomes active (preferring the one to the right, then left).
    /// Returns `true` if a tab was actually closed.
    pub fn close_tab(&mut self, id: TabId) -> bool {
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };

        self.tabs.remove(pos);

        if self.tabs.is_empty() {
            self.active_index = 0;
        } else if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        } else if pos < self.active_index {
            self.active_index -= 1;
        }

        true
    }

    /// Switch to the tab with the given id. Returns `true` if found.
    pub fn switch_to(&mut self, id: TabId) -> bool {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.active_index = pos;
            true
        } else {
            false
        }
    }

    /// Switch to the tab at the given index. Returns `true` if the index is
    /// valid.
    pub fn switch_to_index(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_index = index;
            true
        } else {
            false
        }
    }

    /// Move to the next tab, wrapping around.
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_index = (self.active_index + 1) % self.tabs.len();
        }
    }

    /// Move to the previous tab, wrapping around.
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_index = if self.active_index == 0 {
                self.tabs.len() - 1
            } else {
                self.active_index - 1
            };
        }
    }

    /// Get a reference to the currently active tab, if any.
    #[must_use]
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_index)
    }

    /// Get a mutable reference to the currently active tab, if any.
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_index)
    }

    /// Iterate over all tabs.
    pub fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter()
    }

    /// Return the index of the active tab.
    #[must_use]
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Reorder a tab from `from` index to `to` index.
    pub fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }

        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);

        // Keep the active index pointing at the same tab.
        if self.active_index == from {
            self.active_index = to;
        } else if from < self.active_index && to >= self.active_index {
            self.active_index -= 1;
        } else if from > self.active_index && to <= self.active_index {
            self.active_index += 1;
        }
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn add_and_active() {
        let mut mgr = TabManager::new();
        assert!(mgr.is_empty());

        let id1 = mgr.add_tab(url("https://a.com"));
        assert_eq!(mgr.len(), 1);
        assert_eq!(mgr.active_tab().unwrap().id, id1);

        let id2 = mgr.add_tab(url("https://b.com"));
        assert_eq!(mgr.len(), 2);
        // newly added tab is active
        assert_eq!(mgr.active_tab().unwrap().id, id2);
    }

    #[test]
    fn close_active_selects_neighbor() {
        let mut mgr = TabManager::new();
        let _id1 = mgr.add_tab(url("https://a.com"));
        let id2 = mgr.add_tab(url("https://b.com"));
        let id3 = mgr.add_tab(url("https://c.com"));

        // Active is c (index 2). Close it → b should become active.
        mgr.close_tab(id3);
        assert_eq!(mgr.active_tab().unwrap().id, id2);

        // Close b → a should become active.
        mgr.close_tab(id2);
        assert_eq!(mgr.active_tab().unwrap().url.as_str(), "https://a.com/");
    }

    #[test]
    fn switch_to_by_id() {
        let mut mgr = TabManager::new();
        let id1 = mgr.add_tab(url("https://a.com"));
        let _id2 = mgr.add_tab(url("https://b.com"));

        assert!(mgr.switch_to(id1));
        assert_eq!(mgr.active_tab().unwrap().id, id1);
    }

    #[test]
    fn next_prev_wraps() {
        let mut mgr = TabManager::new();
        let id1 = mgr.add_tab(url("https://a.com"));
        let _id2 = mgr.add_tab(url("https://b.com"));

        // Active is b (index 1). Next wraps to a (index 0).
        mgr.next_tab();
        assert_eq!(mgr.active_tab().unwrap().id, id1);

        // Prev wraps back to b (index 1).
        mgr.prev_tab();
        assert_eq!(mgr.active_index(), 1);
    }

    #[test]
    fn reorder_updates_active() {
        let mut mgr = TabManager::new();
        let id1 = mgr.add_tab(url("https://a.com"));
        let _id2 = mgr.add_tab(url("https://b.com"));
        let _id3 = mgr.add_tab(url("https://c.com"));

        // Active is c (index 2). Move a (index 0) to index 2.
        mgr.switch_to(id1);
        mgr.reorder(0, 2);
        // a moved from 0→2, so active should follow to 2
        assert_eq!(mgr.active_tab().unwrap().id, id1);
        assert_eq!(mgr.active_index(), 2);
    }
}
