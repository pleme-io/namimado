use url::Url;

/// Which panel the sidebar is showing, if visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPanel {
    Bookmarks,
    History,
    Downloads,
}

/// A single bookmark entry.
#[derive(Debug, Clone)]
pub struct Bookmark {
    pub title: String,
    pub url: Url,
    pub folder: Option<String>,
}

impl Bookmark {
    pub fn new(title: impl Into<String>, url: Url) -> Self {
        Self {
            title: title.into(),
            url,
            folder: None,
        }
    }

    pub fn with_folder(mut self, folder: impl Into<String>) -> Self {
        self.folder = Some(folder.into());
        self
    }
}

/// A history entry recording a page visit.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub title: String,
    pub url: Url,
    pub timestamp: u64,
}

/// A download entry.
#[derive(Debug, Clone)]
pub struct DownloadEntry {
    pub filename: String,
    pub url: Url,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub state: DownloadState,
}

/// State of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

/// The sidebar state: visibility, active panel, and data for each panel.
#[derive(Debug, Clone)]
pub struct Sidebar {
    /// Whether the sidebar is visible.
    pub visible: bool,

    /// Which panel is active.
    pub panel: SidebarPanel,

    /// Bookmarks list.
    pub bookmarks: Vec<Bookmark>,

    /// History entries (most recent first).
    pub history: Vec<HistoryEntry>,

    /// Active and recent downloads.
    pub downloads: Vec<DownloadEntry>,
}

impl Sidebar {
    pub fn new() -> Self {
        Self {
            visible: false,
            panel: SidebarPanel::Bookmarks,
            bookmarks: Vec::new(),
            history: Vec::new(),
            downloads: Vec::new(),
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
    }

    /// Hide the sidebar.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Add a bookmark. Returns `false` if a bookmark with the same URL
    /// already exists.
    pub fn add_bookmark(&mut self, bookmark: Bookmark) -> bool {
        if self
            .bookmarks
            .iter()
            .any(|b| b.url == bookmark.url)
        {
            return false;
        }
        self.bookmarks.push(bookmark);
        true
    }

    /// Remove a bookmark by URL. Returns `true` if found and removed.
    pub fn remove_bookmark(&mut self, url: &Url) -> bool {
        let len = self.bookmarks.len();
        self.bookmarks.retain(|b| &b.url != url);
        self.bookmarks.len() < len
    }

    /// Record a history visit.
    pub fn record_visit(&mut self, title: String, url: Url, timestamp: u64) {
        self.history.insert(
            0,
            HistoryEntry {
                title,
                url,
                timestamp,
            },
        );
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
    fn add_duplicate_bookmark_rejected() {
        let mut sidebar = Sidebar::new();
        let url = Url::parse("https://example.com").unwrap();
        assert!(sidebar.add_bookmark(Bookmark::new("Example", url.clone())));
        assert!(!sidebar.add_bookmark(Bookmark::new("Duplicate", url)));
        assert_eq!(sidebar.bookmarks.len(), 1);
    }

    #[test]
    fn remove_bookmark() {
        let mut sidebar = Sidebar::new();
        let url = Url::parse("https://example.com").unwrap();
        sidebar.add_bookmark(Bookmark::new("Example", url.clone()));
        assert!(sidebar.remove_bookmark(&url));
        assert!(sidebar.bookmarks.is_empty());
    }

    #[test]
    fn history_most_recent_first() {
        let mut sidebar = Sidebar::new();
        sidebar.record_visit("First".into(), Url::parse("https://a.com").unwrap(), 100);
        sidebar.record_visit("Second".into(), Url::parse("https://b.com").unwrap(), 200);
        assert_eq!(sidebar.history[0].title, "Second");
        assert_eq!(sidebar.history[1].title, "First");
    }
}
