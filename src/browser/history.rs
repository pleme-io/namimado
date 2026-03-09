use serde::{Deserialize, Serialize};
use url::Url;

/// A single browsing history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Page title at time of visit.
    pub title: String,
    /// URL visited.
    #[serde(with = "crate::browser::bookmark::url_serde")]
    pub url: Url,
    /// Unix timestamp of the visit.
    pub timestamp: i64,
    /// Number of times this URL has been visited.
    pub visit_count: u32,
}

/// Manages browsing history: recording visits, searching, clearing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryManager {
    /// All history entries, most recent first.
    entries: Vec<HistoryEntry>,
    /// Maximum number of entries to keep.
    max_entries: usize,
}

impl HistoryManager {
    /// Create a new empty history manager.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 10_000,
        }
    }

    /// Record a page visit. If the URL was visited before, increments
    /// the visit count and moves it to the top.
    pub fn record_visit(&mut self, title: impl Into<String>, url: Url) {
        let title = title.into();
        let now = chrono::Utc::now().timestamp();

        // Check if we already have this URL in history
        if let Some(pos) = self.entries.iter().position(|e| e.url == url) {
            let mut entry = self.entries.remove(pos);
            entry.title = title;
            entry.timestamp = now;
            entry.visit_count += 1;
            self.entries.insert(0, entry);
        } else {
            self.entries.insert(
                0,
                HistoryEntry {
                    title,
                    url,
                    timestamp: now,
                    visit_count: 1,
                },
            );
        }

        // Enforce maximum size
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Search history by title or URL substring (case-insensitive).
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.title.to_lowercase().contains(&query_lower)
                    || e.url.as_str().to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Get the most recent N history entries.
    #[must_use]
    pub fn recent(&self, count: usize) -> &[HistoryEntry] {
        let end = count.min(self.entries.len());
        &self.entries[..end]
    }

    /// Get all entries for a given date (YYYY-MM-DD string).
    #[must_use]
    pub fn entries_for_date(&self, date: &str) -> Vec<&HistoryEntry> {
        self.entries
            .iter()
            .filter(|e| {
                let dt = chrono::DateTime::from_timestamp(e.timestamp, 0);
                dt.map_or(false, |d| d.format("%Y-%m-%d").to_string() == date)
            })
            .collect()
    }

    /// Get the most frequently visited URLs.
    #[must_use]
    pub fn most_visited(&self, count: usize) -> Vec<&HistoryEntry> {
        let mut sorted: Vec<&HistoryEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| b.visit_count.cmp(&a.visit_count));
        sorted.truncate(count);
        sorted
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Clear history older than the given timestamp.
    pub fn clear_before(&mut self, timestamp: i64) {
        self.entries.retain(|e| e.timestamp >= timestamp);
    }

    /// Remove all entries for a specific URL.
    pub fn remove_url(&mut self, url: &Url) {
        self.entries.retain(|e| &e.url != url);
    }

    /// Total number of history entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether history is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get all entries (most recent first).
    #[must_use]
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Provide autocomplete suggestions based on history, sorted by
    /// visit count (most visited first).
    #[must_use]
    pub fn autocomplete(&self, prefix: &str) -> Vec<&HistoryEntry> {
        let prefix_lower = prefix.to_lowercase();
        let mut matches: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                e.url.as_str().to_lowercase().contains(&prefix_lower)
                    || e.title.to_lowercase().contains(&prefix_lower)
            })
            .collect();
        matches.sort_by(|a, b| b.visit_count.cmp(&a.visit_count));
        matches.truncate(10);
        matches
    }
}

impl Default for HistoryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn record_and_search() {
        let mut hist = HistoryManager::new();
        hist.record_visit("Rust", test_url("https://rust-lang.org"));
        hist.record_visit("Go", test_url("https://go.dev"));

        let results = hist.search("rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust");
    }

    #[test]
    fn duplicate_visit_increments_count() {
        let mut hist = HistoryManager::new();
        let url = test_url("https://example.com");
        hist.record_visit("Example", url.clone());
        hist.record_visit("Example Updated", url);

        assert_eq!(hist.len(), 1);
        assert_eq!(hist.entries[0].visit_count, 2);
        assert_eq!(hist.entries[0].title, "Example Updated");
    }

    #[test]
    fn recent_entries() {
        let mut hist = HistoryManager::new();
        hist.record_visit("A", test_url("https://a.com"));
        hist.record_visit("B", test_url("https://b.com"));
        hist.record_visit("C", test_url("https://c.com"));

        let recent = hist.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].title, "C");
        assert_eq!(recent[1].title, "B");
    }

    #[test]
    fn most_visited() {
        let mut hist = HistoryManager::new();
        let url_a = test_url("https://a.com");
        hist.record_visit("A", url_a.clone());
        hist.record_visit("A", url_a.clone());
        hist.record_visit("A", url_a);
        hist.record_visit("B", test_url("https://b.com"));

        let top = hist.most_visited(1);
        assert_eq!(top[0].title, "A");
        assert_eq!(top[0].visit_count, 3);
    }

    #[test]
    fn clear_all() {
        let mut hist = HistoryManager::new();
        hist.record_visit("A", test_url("https://a.com"));
        hist.clear();
        assert!(hist.is_empty());
    }

    #[test]
    fn remove_url() {
        let mut hist = HistoryManager::new();
        let url = test_url("https://example.com");
        hist.record_visit("Example", url.clone());
        hist.record_visit("Other", test_url("https://other.com"));

        hist.remove_url(&url);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist.entries[0].title, "Other");
    }

    #[test]
    fn autocomplete_sorted_by_visit_count() {
        let mut hist = HistoryManager::new();
        let url_a = test_url("https://rust-lang.org");
        let url_b = test_url("https://rust-analyzer.github.io");
        hist.record_visit("Rust Lang", url_a.clone());
        hist.record_visit("Rust Analyzer", url_b.clone());
        hist.record_visit("Rust Analyzer", url_b.clone());
        hist.record_visit("Rust Analyzer", url_b);

        let suggestions = hist.autocomplete("rust");
        assert_eq!(suggestions.len(), 2);
        // Rust Analyzer has more visits, should be first
        assert_eq!(suggestions[0].title, "Rust Analyzer");
    }
}
