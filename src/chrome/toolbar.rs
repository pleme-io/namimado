use url::Url;

/// Represents the state of a navigation button in the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    /// The button is enabled and can be clicked.
    Enabled,
    /// The button is disabled (greyed out).
    Disabled,
}

impl ButtonState {
    #[must_use]
    pub fn is_enabled(self) -> bool {
        self == Self::Enabled
    }

    /// Create a `ButtonState` from a boolean.
    #[must_use]
    pub fn from_bool(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

/// An autocomplete suggestion for the address bar.
#[derive(Debug, Clone)]
pub struct AddressSuggestion {
    /// The URL this suggestion leads to.
    pub url: String,
    /// Display title.
    pub title: String,
    /// Source of the suggestion (history, bookmark, or search).
    pub source: SuggestionSource,
}

/// Where an autocomplete suggestion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionSource {
    History,
    Bookmark,
    Search,
}

/// The browser toolbar: address bar, navigation buttons, bookmark toggle,
/// download indicator.
///
/// This is a pure model/state object. When the `gpu-chrome` feature is enabled,
/// it can be rendered via egaku widgets. Without that feature it still provides
/// the state for any UI layer.
#[derive(Debug, Clone)]
pub struct Toolbar {
    /// Text currently displayed/edited in the address bar.
    address_text: String,

    /// Whether the address bar is focused for editing.
    pub address_focused: bool,

    /// Autocomplete suggestions for the address bar.
    pub suggestions: Vec<AddressSuggestion>,

    /// Index of the currently selected suggestion (if any).
    pub selected_suggestion: Option<usize>,

    /// Back button state.
    pub back: ButtonState,

    /// Forward button state.
    pub forward: ButtonState,

    /// Whether a page is currently loading (controls reload/stop button).
    pub loading: bool,

    /// Whether the current page is bookmarked (filled star vs outline).
    pub is_bookmarked: bool,

    /// Number of active downloads (shown as badge on download button).
    pub active_downloads: usize,

    /// Whether the bookmark bar is visible below the toolbar.
    pub show_bookmark_bar: bool,
}

impl Toolbar {
    /// Create a new toolbar with default state.
    pub fn new() -> Self {
        Self {
            address_text: String::new(),
            address_focused: false,
            suggestions: Vec::new(),
            selected_suggestion: None,
            back: ButtonState::Disabled,
            forward: ButtonState::Disabled,
            loading: false,
            is_bookmarked: false,
            active_downloads: 0,
            show_bookmark_bar: true,
        }
    }

    /// Get the current address bar text.
    #[must_use]
    pub fn url_bar_text(&self) -> &str {
        &self.address_text
    }

    /// Set the address bar text (e.g., when navigation completes).
    pub fn set_url(&mut self, url: &Url) {
        self.address_text = url.as_str().to_owned();
        self.address_focused = false;
        self.suggestions.clear();
        self.selected_suggestion = None;
    }

    /// Update the toolbar state from the current tab state.
    pub fn sync_with_tab(
        &mut self,
        url: &Url,
        can_go_back: bool,
        can_go_forward: bool,
        loading: bool,
        is_bookmarked: bool,
    ) {
        self.set_url(url);
        self.back = ButtonState::from_bool(can_go_back);
        self.forward = ButtonState::from_bool(can_go_forward);
        self.loading = loading;
        self.is_bookmarked = is_bookmarked;
    }

    /// Handle user typing in the address bar.
    pub fn handle_input(&mut self, text: &str) {
        self.address_text = text.to_owned();
        self.address_focused = true;
    }

    /// Set autocomplete suggestions for the current address bar text.
    pub fn set_suggestions(&mut self, suggestions: Vec<AddressSuggestion>) {
        self.suggestions = suggestions;
        self.selected_suggestion = None;
    }

    /// Select the next suggestion in the dropdown.
    pub fn select_next_suggestion(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }
        self.selected_suggestion = Some(match self.selected_suggestion {
            Some(i) if i + 1 < self.suggestions.len() => i + 1,
            Some(_) => 0,
            None => 0,
        });
    }

    /// Select the previous suggestion in the dropdown.
    pub fn select_prev_suggestion(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }
        self.selected_suggestion = Some(match self.selected_suggestion {
            Some(0) => self.suggestions.len() - 1,
            Some(i) => i - 1,
            None => self.suggestions.len() - 1,
        });
    }

    /// Submit the address bar (user pressed Enter). Returns the text
    /// that should be navigated to. If a suggestion is selected, returns
    /// that suggestion's URL instead.
    pub fn submit_address(&mut self) -> String {
        self.address_focused = false;
        let result = if let Some(idx) = self.selected_suggestion {
            if let Some(suggestion) = self.suggestions.get(idx) {
                suggestion.url.clone()
            } else {
                self.address_text.clone()
            }
        } else {
            self.address_text.clone()
        };
        self.suggestions.clear();
        self.selected_suggestion = None;
        result
    }

    /// Focus the address bar and select all text (Cmd+L / Ctrl+L).
    pub fn focus_address_bar(&mut self) {
        self.address_focused = true;
    }

    /// Dismiss the address bar (Escape).
    pub fn dismiss_address_bar(&mut self) {
        self.address_focused = false;
        self.suggestions.clear();
        self.selected_suggestion = None;
    }

    /// Update the active download count.
    pub fn set_active_downloads(&mut self, count: usize) {
        self.active_downloads = count;
    }

    /// Toggle the bookmark bar visibility.
    pub fn toggle_bookmark_bar(&mut self) {
        self.show_bookmark_bar = !self.show_bookmark_bar;
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_with_tab() {
        let mut toolbar = Toolbar::new();
        let url = Url::parse("https://example.com").unwrap();
        toolbar.sync_with_tab(&url, true, false, true, true);

        assert_eq!(toolbar.url_bar_text(), "https://example.com/");
        assert!(toolbar.back.is_enabled());
        assert!(!toolbar.forward.is_enabled());
        assert!(toolbar.loading);
        assert!(toolbar.is_bookmarked);
    }

    #[test]
    fn handle_input_and_submit() {
        let mut toolbar = Toolbar::new();
        toolbar.handle_input("rust");
        assert!(toolbar.address_focused);
        assert_eq!(toolbar.url_bar_text(), "rust");

        let submitted = toolbar.submit_address();
        assert_eq!(submitted, "rust");
        assert!(!toolbar.address_focused);
    }

    #[test]
    fn suggestion_navigation() {
        let mut toolbar = Toolbar::new();
        toolbar.set_suggestions(vec![
            AddressSuggestion {
                url: "https://a.com".into(),
                title: "A".into(),
                source: SuggestionSource::History,
            },
            AddressSuggestion {
                url: "https://b.com".into(),
                title: "B".into(),
                source: SuggestionSource::Bookmark,
            },
        ]);

        toolbar.select_next_suggestion();
        assert_eq!(toolbar.selected_suggestion, Some(0));

        toolbar.select_next_suggestion();
        assert_eq!(toolbar.selected_suggestion, Some(1));

        // Wraps around
        toolbar.select_next_suggestion();
        assert_eq!(toolbar.selected_suggestion, Some(0));

        toolbar.select_prev_suggestion();
        assert_eq!(toolbar.selected_suggestion, Some(1));
    }

    #[test]
    fn submit_with_selected_suggestion() {
        let mut toolbar = Toolbar::new();
        toolbar.handle_input("exa");
        toolbar.set_suggestions(vec![AddressSuggestion {
            url: "https://example.com".into(),
            title: "Example".into(),
            source: SuggestionSource::History,
        }]);
        toolbar.select_next_suggestion();

        let submitted = toolbar.submit_address();
        assert_eq!(submitted, "https://example.com");
    }

    #[test]
    fn dismiss_clears_suggestions() {
        let mut toolbar = Toolbar::new();
        toolbar.set_suggestions(vec![AddressSuggestion {
            url: "https://a.com".into(),
            title: "A".into(),
            source: SuggestionSource::History,
        }]);
        toolbar.focus_address_bar();
        toolbar.dismiss_address_bar();

        assert!(!toolbar.address_focused);
        assert!(toolbar.suggestions.is_empty());
    }

    #[test]
    fn download_indicator() {
        let mut toolbar = Toolbar::new();
        assert_eq!(toolbar.active_downloads, 0);
        toolbar.set_active_downloads(3);
        assert_eq!(toolbar.active_downloads, 3);
    }

    #[test]
    fn bookmark_bar_toggle() {
        let mut toolbar = Toolbar::new();
        assert!(toolbar.show_bookmark_bar);
        toolbar.toggle_bookmark_bar();
        assert!(!toolbar.show_bookmark_bar);
    }
}
