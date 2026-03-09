use crate::browser::bookmark::{Bookmark, BookmarkManager};
use crate::browser::download::DownloadManager;
use crate::browser::history::HistoryManager;

/// Which panel the sidebar is showing, if visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPanel {
    Bookmarks,
    History,
    Downloads,
}

/// The sidebar state: visibility, active panel, and search filter.
///
/// The sidebar does not own data — it provides a view over the
/// `BookmarkManager`, `HistoryManager`, and `DownloadManager` that live
/// in the `App` struct. This struct tracks UI state only.
#[derive(Debug, Clone)]
pub struct Sidebar {
    /// Whether the sidebar is visible.
    pub visible: bool,

    /// Which panel is active.
    pub panel: SidebarPanel,

    /// Search/filter text within the active panel.
    pub search_text: String,

    /// Whether the search field is focused.
    pub search_focused: bool,

    /// Width in logical pixels.
    pub width: u32,

    /// Scroll offset (in items) for the active panel.
    pub scroll_offset: usize,
}

impl Sidebar {
    /// Create a new sidebar with default state.
    pub fn new() -> Self {
        Self {
            visible: false,
            panel: SidebarPanel::Bookmarks,
            search_text: String::new(),
            search_focused: false,
            width: 300,
            scroll_offset: 0,
        }
    }

    /// Toggle sidebar visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Show the sidebar with a specific panel.
    pub fn show(&mut self, panel: SidebarPanel) {
        self.visible = true;
        self.panel = panel;
        self.search_text.clear();
        self.scroll_offset = 0;
    }

    /// Hide the sidebar.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Switch to a different panel.
    pub fn switch_panel(&mut self, panel: SidebarPanel) {
        self.panel = panel;
        self.search_text.clear();
        self.scroll_offset = 0;
    }

    /// Set the search text for filtering.
    pub fn set_search(&mut self, text: &str) {
        self.search_text = text.to_owned();
        self.scroll_offset = 0;
    }

    /// Clear the search filter.
    pub fn clear_search(&mut self) {
        self.search_text.clear();
        self.scroll_offset = 0;
    }

    /// Scroll down by one page (e.g. 20 items).
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(20);
    }

    /// Scroll up by one page.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(20);
    }

    /// Get the filtered bookmark list for the current search text.
    #[must_use]
    pub fn filtered_bookmarks<'a>(
        &self,
        bookmarks: &'a BookmarkManager,
    ) -> Vec<&'a Bookmark> {
        if self.search_text.is_empty() {
            bookmarks.list(None)
        } else {
            bookmarks.search(&self.search_text)
        }
    }

    /// Get the filtered history list for the current search text.
    #[must_use]
    pub fn filtered_history<'a>(
        &self,
        history: &'a HistoryManager,
    ) -> Vec<&'a crate::browser::history::HistoryEntry> {
        if self.search_text.is_empty() {
            history.recent(100).iter().collect()
        } else {
            history.search(&self.search_text)
        }
    }

    /// Get the download list (no filtering, just active + recent).
    #[must_use]
    pub fn download_list<'a>(
        &self,
        downloads: &'a DownloadManager,
    ) -> &'a [crate::browser::download::Download] {
        downloads.all()
    }
}

impl Default for Sidebar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn toggle_visibility() {
        let mut sidebar = Sidebar::new();
        assert!(!sidebar.visible);
        sidebar.toggle();
        assert!(sidebar.visible);
        sidebar.toggle();
        assert!(!sidebar.visible);
    }

    #[test]
    fn show_panel() {
        let mut sidebar = Sidebar::new();
        sidebar.show(SidebarPanel::History);
        assert!(sidebar.visible);
        assert_eq!(sidebar.panel, SidebarPanel::History);
    }

    #[test]
    fn switch_panel_clears_search() {
        let mut sidebar = Sidebar::new();
        sidebar.set_search("test");
        sidebar.switch_panel(SidebarPanel::Downloads);
        assert!(sidebar.search_text.is_empty());
        assert_eq!(sidebar.panel, SidebarPanel::Downloads);
    }

    #[test]
    fn filtered_bookmarks_with_search() {
        let mut bookmarks = BookmarkManager::new();
        bookmarks.add(Bookmark::new(
            "Rust",
            Url::parse("https://rust-lang.org").unwrap(),
        ));
        bookmarks.add(Bookmark::new(
            "Go",
            Url::parse("https://go.dev").unwrap(),
        ));

        let mut sidebar = Sidebar::new();
        sidebar.set_search("rust");
        let filtered = sidebar.filtered_bookmarks(&bookmarks);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Rust");
    }

    #[test]
    fn scroll_operations() {
        let mut sidebar = Sidebar::new();
        sidebar.scroll_down();
        assert_eq!(sidebar.scroll_offset, 20);
        sidebar.scroll_up();
        assert_eq!(sidebar.scroll_offset, 0);
        // Should not underflow
        sidebar.scroll_up();
        assert_eq!(sidebar.scroll_offset, 0);
    }
}
