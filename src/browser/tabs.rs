use url::Url;

use super::tab::{Tab, TabId};

/// A snapshot of a closed tab that can be restored.
#[derive(Debug, Clone)]
pub struct ClosedTab {
    /// The URL the tab was showing when closed.
    pub url: Url,
    /// The title the tab had when closed.
    pub title: String,
    /// The index the tab occupied before closing.
    pub index: usize,
}

/// Manages the collection of open tabs and tracks which one is active.
///
/// Supports creation, closing, switching, reordering, pinning, duplicating,
/// and restoring recently closed tabs.
#[derive(Debug)]
pub struct TabManager {
    tabs: Vec<Tab>,
    active_index: usize,
    /// Stack of recently closed tabs (most recent last), for Cmd+Shift+T.
    closed_tabs: Vec<ClosedTab>,
    /// Maximum number of closed tabs to remember.
    max_closed: usize,
}

impl TabManager {
    /// Create an empty tab manager with no tabs.
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_index: 0,
            closed_tabs: Vec::new(),
            max_closed: 25,
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

    /// Add a new tab immediately after the currently active tab.
    /// Makes the new tab active. Returns its id.
    pub fn add_tab_after_active(&mut self, url: Url) -> TabId {
        let tab = Tab::new(url);
        let id = tab.id;
        let insert_at = if self.tabs.is_empty() {
            0
        } else {
            self.active_index + 1
        };
        self.tabs.insert(insert_at, tab);
        self.active_index = insert_at;
        id
    }

    /// Close the tab with the given id. If the active tab is closed, the
    /// adjacent tab becomes active (preferring the one to the right, then left).
    /// The closed tab is remembered for restoration. Returns `true` if a tab
    /// was actually closed.
    pub fn close_tab(&mut self, id: TabId) -> bool {
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };

        let tab = &self.tabs[pos];
        // Remember this tab for restoration
        let closed = ClosedTab {
            url: tab.url.clone(),
            title: tab.title.clone(),
            index: pos,
        };
        self.closed_tabs.push(closed);
        if self.closed_tabs.len() > self.max_closed {
            self.closed_tabs.remove(0);
        }

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

    /// Close the currently active tab. Returns `true` if a tab was closed.
    pub fn close_active_tab(&mut self) -> bool {
        if let Some(tab) = self.active_tab() {
            let id = tab.id;
            // Don't close pinned tabs via keyboard shortcut
            if tab.pinned {
                return false;
            }
            self.close_tab(id)
        } else {
            false
        }
    }

    /// Restore the most recently closed tab. Returns the new tab's id if
    /// successful, or `None` if there are no closed tabs to restore.
    pub fn restore_closed_tab(&mut self) -> Option<TabId> {
        let closed = self.closed_tabs.pop()?;
        let tab = Tab::new(closed.url);
        let id = tab.id;
        // Insert at original position if possible, otherwise at end
        let insert_at = closed.index.min(self.tabs.len());
        self.tabs.insert(insert_at, tab);
        self.active_index = insert_at;
        Some(id)
    }

    /// Get the list of recently closed tabs (most recent last).
    #[must_use]
    pub fn closed_tabs(&self) -> &[ClosedTab] {
        &self.closed_tabs
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

    /// Get a reference to a tab by id.
    #[must_use]
    pub fn get_tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// Get a mutable reference to a tab by id.
    pub fn get_tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
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

    /// Pin or unpin a tab by id. Pinned tabs are moved to the start of the
    /// tab bar; unpinned tabs are moved to after the last pinned tab.
    pub fn toggle_pin(&mut self, id: TabId) {
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };

        self.tabs[pos].toggle_pin();

        if self.tabs[pos].pinned {
            // Move to the end of the pinned section
            let pinned_end = self
                .tabs
                .iter()
                .take(pos)
                .filter(|t| t.pinned)
                .count();
            if pos != pinned_end {
                self.reorder(pos, pinned_end);
            }
        } else {
            // Move to after the last pinned tab
            let pinned_count = self.tabs.iter().filter(|t| t.pinned).count();
            let target = if pinned_count > 0 {
                pinned_count
            } else {
                0
            };
            if pos != target && target < self.tabs.len() {
                self.reorder(pos, target);
            }
        }
    }

    /// Duplicate the currently active tab. Returns the new tab's id if
    /// successful.
    pub fn duplicate_active_tab(&mut self) -> Option<TabId> {
        let url = self.active_tab()?.url.clone();
        Some(self.add_tab_after_active(url))
    }

    /// Return the number of pinned tabs.
    #[must_use]
    pub fn pinned_count(&self) -> usize {
        self.tabs.iter().filter(|t| t.pinned).count()
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

        // Active is c (index 2). Close it — b should become active.
        mgr.close_tab(id3);
        assert_eq!(mgr.active_tab().unwrap().id, id2);

        // Close b — a should become active.
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
        // a moved from 0->2, so active should follow to 2
        assert_eq!(mgr.active_tab().unwrap().id, id1);
        assert_eq!(mgr.active_index(), 2);
    }

    #[test]
    fn close_remembers_for_restore() {
        let mut mgr = TabManager::new();
        let _id1 = mgr.add_tab(url("https://a.com"));
        let id2 = mgr.add_tab(url("https://b.com"));

        mgr.close_tab(id2);
        assert_eq!(mgr.closed_tabs().len(), 1);
        assert_eq!(mgr.closed_tabs()[0].url.as_str(), "https://b.com/");
    }

    #[test]
    fn restore_closed_tab() {
        let mut mgr = TabManager::new();
        let _id1 = mgr.add_tab(url("https://a.com"));
        let id2 = mgr.add_tab(url("https://b.com"));

        mgr.close_tab(id2);
        let restored_id = mgr.restore_closed_tab().unwrap();
        assert_eq!(mgr.len(), 2);
        assert_eq!(mgr.active_tab().unwrap().id, restored_id);
        assert_eq!(
            mgr.active_tab().unwrap().url.as_str(),
            "https://b.com/"
        );
    }

    #[test]
    fn restore_returns_none_when_empty() {
        let mut mgr = TabManager::new();
        assert!(mgr.restore_closed_tab().is_none());
    }

    #[test]
    fn add_tab_after_active() {
        let mut mgr = TabManager::new();
        let _id1 = mgr.add_tab(url("https://a.com"));
        let _id2 = mgr.add_tab(url("https://b.com"));
        mgr.switch_to_index(0);

        let id3 = mgr.add_tab_after_active(url("https://c.com"));
        assert_eq!(mgr.active_index(), 1);
        assert_eq!(mgr.active_tab().unwrap().id, id3);
        // Tab order should be: a, c, b
        let urls: Vec<&str> = mgr.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, vec!["https://a.com/", "https://c.com/", "https://b.com/"]);
    }

    #[test]
    fn close_active_respects_pinned() {
        let mut mgr = TabManager::new();
        let id1 = mgr.add_tab(url("https://a.com"));
        mgr.get_tab_mut(id1).unwrap().pinned = true;
        mgr.switch_to(id1);

        // Should not close a pinned tab
        assert!(!mgr.close_active_tab());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn duplicate_active() {
        let mut mgr = TabManager::new();
        let _id1 = mgr.add_tab(url("https://a.com"));
        let _id2 = mgr.add_tab(url("https://b.com"));
        mgr.switch_to_index(0);

        let dup_id = mgr.duplicate_active_tab().unwrap();
        assert_eq!(mgr.len(), 3);
        assert_eq!(mgr.active_tab().unwrap().id, dup_id);
        assert_eq!(mgr.active_tab().unwrap().url.as_str(), "https://a.com/");
    }

    #[test]
    fn pin_moves_to_front() {
        let mut mgr = TabManager::new();
        let _id1 = mgr.add_tab(url("https://a.com"));
        let _id2 = mgr.add_tab(url("https://b.com"));
        let id3 = mgr.add_tab(url("https://c.com"));

        mgr.toggle_pin(id3);
        // c should now be at index 0
        assert_eq!(mgr.iter().next().unwrap().url.as_str(), "https://c.com/");
        assert!(mgr.iter().next().unwrap().pinned);
        assert_eq!(mgr.pinned_count(), 1);
    }
}
