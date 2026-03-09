use serde::{Deserialize, Serialize};
use url::Url;

/// A single bookmark entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Bookmark title.
    pub title: String,
    /// The bookmarked URL.
    #[serde(with = "url_serde")]
    pub url: Url,
    /// Folder path (e.g. "News/Tech"). `None` means root.
    pub folder: Option<String>,
    /// Tags for search/filtering.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Unix timestamp when the bookmark was created.
    pub created_at: i64,
}

impl Bookmark {
    /// Create a new bookmark at the root level.
    pub fn new(title: impl Into<String>, url: Url) -> Self {
        Self {
            title: title.into(),
            url,
            folder: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Place this bookmark in a folder.
    #[must_use]
    pub fn with_folder(mut self, folder: impl Into<String>) -> Self {
        self.folder = Some(folder.into());
        self
    }

    /// Add tags to this bookmark.
    #[must_use]
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// A folder in the bookmark tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkFolder {
    /// Folder name.
    pub name: String,
    /// Full path (e.g. "News/Tech").
    pub path: String,
}

/// Manages the user's bookmarks: add, remove, organize, search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkManager {
    /// All bookmarks, in insertion order.
    bookmarks: Vec<Bookmark>,
    /// Known folders (populated from bookmarks + explicit creation).
    folders: Vec<BookmarkFolder>,
}

impl BookmarkManager {
    /// Create an empty bookmark manager.
    pub fn new() -> Self {
        Self {
            bookmarks: Vec::new(),
            folders: Vec::new(),
        }
    }

    /// Add a bookmark. Returns `false` if a bookmark with the same URL already
    /// exists.
    pub fn add(&mut self, bookmark: Bookmark) -> bool {
        if self.bookmarks.iter().any(|b| b.url == bookmark.url) {
            return false;
        }
        // Auto-create folder if specified and not yet known
        if let Some(ref folder) = bookmark.folder {
            self.ensure_folder(folder);
        }
        self.bookmarks.push(bookmark);
        true
    }

    /// Remove a bookmark by URL. Returns `true` if found and removed.
    pub fn remove(&mut self, url: &Url) -> bool {
        let len = self.bookmarks.len();
        self.bookmarks.retain(|b| &b.url != url);
        self.bookmarks.len() < len
    }

    /// Toggle bookmark: if it exists, remove it; otherwise add it.
    /// Returns `true` if the bookmark now exists (was added).
    pub fn toggle(&mut self, title: impl Into<String>, url: Url) -> bool {
        if self.is_bookmarked(&url) {
            self.remove(&url);
            false
        } else {
            self.add(Bookmark::new(title, url));
            true
        }
    }

    /// Check if a URL is bookmarked.
    #[must_use]
    pub fn is_bookmarked(&self, url: &Url) -> bool {
        self.bookmarks.iter().any(|b| &b.url == url)
    }

    /// Move a bookmark into a folder.
    pub fn move_to_folder(&mut self, url: &Url, folder: Option<String>) {
        if let Some(folder_name) = &folder {
            self.ensure_folder(folder_name);
        }
        if let Some(bm) = self.bookmarks.iter_mut().find(|b| &b.url == url) {
            bm.folder = folder;
        }
    }

    /// Create a folder (if it does not already exist).
    pub fn create_folder(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.ensure_folder(&path);
    }

    /// Remove a folder. Bookmarks in the folder are moved to root.
    pub fn remove_folder(&mut self, path: &str) {
        self.folders.retain(|f| f.path != path);
        for bm in &mut self.bookmarks {
            if bm.folder.as_deref() == Some(path) {
                bm.folder = None;
            }
        }
    }

    /// List all bookmarks, optionally filtered to a specific folder.
    #[must_use]
    pub fn list(&self, folder: Option<&str>) -> Vec<&Bookmark> {
        self.bookmarks
            .iter()
            .filter(|b| match folder {
                Some(f) => b.folder.as_deref() == Some(f),
                None => true,
            })
            .collect()
    }

    /// Search bookmarks by title or URL substring (case-insensitive).
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&Bookmark> {
        let query_lower = query.to_lowercase();
        self.bookmarks
            .iter()
            .filter(|b| {
                b.title.to_lowercase().contains(&query_lower)
                    || b.url.as_str().to_lowercase().contains(&query_lower)
                    || b.tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Bookmarks that should be displayed in the bookmark bar (root level).
    #[must_use]
    pub fn bar_bookmarks(&self) -> Vec<&Bookmark> {
        self.bookmarks
            .iter()
            .filter(|b| b.folder.is_none())
            .collect()
    }

    /// List all known folders.
    #[must_use]
    pub fn folders(&self) -> &[BookmarkFolder] {
        &self.folders
    }

    /// Total number of bookmarks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bookmarks.len()
    }

    /// Whether the bookmark collection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bookmarks.is_empty()
    }

    /// Ensure a folder exists, creating it and any parent paths if needed.
    fn ensure_folder(&mut self, path: &str) {
        if !self.folders.iter().any(|f| f.path == path) {
            // Extract folder name from the path
            let name = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .to_owned();
            self.folders.push(BookmarkFolder {
                name,
                path: path.to_owned(),
            });
        }
    }
}

impl Default for BookmarkManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Serde helper for `url::Url`.
pub(crate) mod url_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use url::Url;

    pub fn serialize<S>(url: &Url, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(url.as_str())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Url, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Url::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn add_and_remove() {
        let mut mgr = BookmarkManager::new();
        assert!(mgr.add(Bookmark::new("Example", test_url("https://example.com"))));
        assert_eq!(mgr.len(), 1);
        assert!(mgr.is_bookmarked(&test_url("https://example.com")));

        assert!(mgr.remove(&test_url("https://example.com")));
        assert!(mgr.is_empty());
    }

    #[test]
    fn reject_duplicate() {
        let mut mgr = BookmarkManager::new();
        assert!(mgr.add(Bookmark::new("A", test_url("https://a.com"))));
        assert!(!mgr.add(Bookmark::new("A again", test_url("https://a.com"))));
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn toggle_bookmark() {
        let mut mgr = BookmarkManager::new();
        let url = test_url("https://example.com");

        assert!(mgr.toggle("Example", url.clone()));
        assert!(mgr.is_bookmarked(&url));

        assert!(!mgr.toggle("Example", url.clone()));
        assert!(!mgr.is_bookmarked(&url));
    }

    #[test]
    fn folder_operations() {
        let mut mgr = BookmarkManager::new();
        mgr.add(
            Bookmark::new("News", test_url("https://news.ycombinator.com"))
                .with_folder("Tech"),
        );
        mgr.add(Bookmark::new("Root", test_url("https://root.com")));

        let tech = mgr.list(Some("Tech"));
        assert_eq!(tech.len(), 1);
        assert_eq!(tech[0].title, "News");

        let all = mgr.list(None);
        assert_eq!(all.len(), 2);

        let bar = mgr.bar_bookmarks();
        assert_eq!(bar.len(), 1);
        assert_eq!(bar[0].title, "Root");
    }

    #[test]
    fn search_by_title_and_url() {
        let mut mgr = BookmarkManager::new();
        mgr.add(Bookmark::new("Rust Lang", test_url("https://rust-lang.org")));
        mgr.add(Bookmark::new("Go Lang", test_url("https://go.dev")));
        mgr.add(
            Bookmark::new("Example", test_url("https://example.com"))
                .with_tags(vec!["rust".into()]),
        );

        let results = mgr.search("rust");
        assert_eq!(results.len(), 2); // "Rust Lang" by title + "Example" by tag
    }

    #[test]
    fn move_to_folder() {
        let mut mgr = BookmarkManager::new();
        let url = test_url("https://example.com");
        mgr.add(Bookmark::new("Example", url.clone()));
        assert!(mgr.list(None)[0].folder.is_none());

        mgr.move_to_folder(&url, Some("Saved".into()));
        assert_eq!(
            mgr.list(Some("Saved"))[0].url.as_str(),
            "https://example.com/"
        );
    }

    #[test]
    fn remove_folder_moves_bookmarks_to_root() {
        let mut mgr = BookmarkManager::new();
        mgr.add(
            Bookmark::new("In folder", test_url("https://example.com"))
                .with_folder("MyFolder"),
        );
        mgr.remove_folder("MyFolder");

        assert!(mgr.folders().is_empty());
        assert!(mgr.list(None)[0].folder.is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let mut mgr = BookmarkManager::new();
        mgr.add(
            Bookmark::new("Test", test_url("https://test.com"))
                .with_folder("Dev")
                .with_tags(vec!["testing".into()]),
        );

        let json = serde_json::to_string(&mgr).unwrap();
        let deserialized: BookmarkManager = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), 1);
        assert_eq!(deserialized.bookmarks[0].title, "Test");
    }
}
