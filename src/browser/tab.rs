use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use url::Url;

/// Monotonically increasing tab identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u64);

impl TabId {
    /// Generate a fresh, globally unique tab id.
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Return the raw numeric id.
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tab-{}", self.0)
    }
}

/// A single browser tab's state.
///
/// This is a pure model object — it does not hold a reference to any webview
/// or platform resource. The webview engine maps `TabId` to platform handles
/// separately.
#[derive(Debug, Clone)]
pub struct Tab {
    /// Unique identifier for this tab.
    pub id: TabId,

    /// Current URL displayed in this tab.
    pub url: Url,

    /// Page title. Defaults to the URL string until the page sets a title.
    pub title: String,

    /// Whether the page is currently loading.
    pub loading: bool,

    /// Whether the back button should be enabled.
    pub can_go_back: bool,

    /// Whether the forward button should be enabled.
    pub can_go_forward: bool,

    /// Whether this tab is pinned (pinned tabs cannot be closed with Cmd+W,
    /// appear first in the tab bar, and show only favicon).
    pub pinned: bool,

    /// Whether the address bar text is selected (for editing).
    pub address_selected: bool,

    /// Navigation history — back stack (most recent last).
    history_back: Vec<Url>,

    /// Navigation history — forward stack (most recent last).
    history_forward: Vec<Url>,
}

impl Tab {
    /// Create a new tab pointing at the given URL.
    pub fn new(url: Url) -> Self {
        let title = url.as_str().to_owned();
        Self {
            id: TabId::next(),
            url,
            title,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            pinned: false,
            address_selected: false,
            history_back: Vec::new(),
            history_forward: Vec::new(),
        }
    }

    /// Push a new URL into the navigation history and update the current URL.
    /// Clears the forward stack (as in a real browser).
    pub fn push_navigation(&mut self, new_url: Url) {
        self.history_back.push(self.url.clone());
        self.url = new_url.clone();
        self.title = new_url.as_str().to_owned();
        self.history_forward.clear();
        self.can_go_back = true;
        self.can_go_forward = false;
    }

    /// Move back in history. Returns the URL to navigate to, or `None` if
    /// the back stack is empty.
    pub fn go_back(&mut self) -> Option<&Url> {
        let prev = self.history_back.pop()?;
        self.history_forward.push(self.url.clone());
        self.url = prev;
        self.can_go_back = !self.history_back.is_empty();
        self.can_go_forward = true;
        Some(&self.url)
    }

    /// Move forward in history. Returns the URL to navigate to, or `None` if
    /// the forward stack is empty.
    pub fn go_forward(&mut self) -> Option<&Url> {
        let next = self.history_forward.pop()?;
        self.history_back.push(self.url.clone());
        self.url = next;
        self.can_go_back = true;
        self.can_go_forward = !self.history_forward.is_empty();
        Some(&self.url)
    }

    /// Toggle the pinned state of this tab.
    pub fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
    }

    /// Create a duplicate of this tab (same URL, fresh ID, history reset).
    #[must_use]
    pub fn duplicate(&self) -> Self {
        Self::new(self.url.clone())
    }

    /// Return the number of entries in the back history.
    #[must_use]
    pub fn back_history_len(&self) -> usize {
        self.history_back.len()
    }

    /// Return the number of entries in the forward history.
    #[must_use]
    pub fn forward_history_len(&self) -> usize {
        self.history_forward.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_ids_are_unique() {
        let a = TabId::next();
        let b = TabId::next();
        assert_ne!(a, b);
    }

    #[test]
    fn new_tab_has_url_as_title() {
        let tab = Tab::new(Url::parse("https://example.com").unwrap());
        assert_eq!(tab.title, "https://example.com/");
    }

    #[test]
    fn push_navigation_clears_forward_stack() {
        let mut tab = Tab::new(Url::parse("https://one.com").unwrap());
        tab.push_navigation(Url::parse("https://two.com").unwrap());
        tab.push_navigation(Url::parse("https://three.com").unwrap());
        tab.go_back();
        // Now forward stack has three.com. Navigating clears it.
        tab.push_navigation(Url::parse("https://four.com").unwrap());
        assert!(!tab.can_go_forward);
        assert!(tab.can_go_back);
    }

    #[test]
    fn back_and_forward() {
        let mut tab = Tab::new(Url::parse("https://a.com").unwrap());
        tab.push_navigation(Url::parse("https://b.com").unwrap());
        tab.push_navigation(Url::parse("https://c.com").unwrap());

        assert_eq!(tab.url.as_str(), "https://c.com/");

        let back_url = tab.go_back().unwrap().clone();
        assert_eq!(back_url.as_str(), "https://b.com/");
        assert!(tab.can_go_forward);

        let fwd_url = tab.go_forward().unwrap().clone();
        assert_eq!(fwd_url.as_str(), "https://c.com/");
    }

    #[test]
    fn go_back_on_empty_returns_none() {
        let mut tab = Tab::new(Url::parse("https://only.com").unwrap());
        assert!(tab.go_back().is_none());
    }

    #[test]
    fn pin_toggle() {
        let mut tab = Tab::new(Url::parse("https://example.com").unwrap());
        assert!(!tab.pinned);
        tab.toggle_pin();
        assert!(tab.pinned);
        tab.toggle_pin();
        assert!(!tab.pinned);
    }

    #[test]
    fn duplicate_creates_new_id() {
        let original = Tab::new(Url::parse("https://example.com").unwrap());
        let dup = original.duplicate();
        assert_ne!(original.id, dup.id);
        assert_eq!(original.url, dup.url);
        assert!(!dup.pinned);
    }

    #[test]
    fn history_lengths() {
        let mut tab = Tab::new(Url::parse("https://a.com").unwrap());
        assert_eq!(tab.back_history_len(), 0);
        assert_eq!(tab.forward_history_len(), 0);

        tab.push_navigation(Url::parse("https://b.com").unwrap());
        assert_eq!(tab.back_history_len(), 1);

        tab.push_navigation(Url::parse("https://c.com").unwrap());
        assert_eq!(tab.back_history_len(), 2);

        tab.go_back();
        assert_eq!(tab.back_history_len(), 1);
        assert_eq!(tab.forward_history_len(), 1);
    }
}
