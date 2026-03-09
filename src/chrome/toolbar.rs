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

/// The browser toolbar: address bar, navigation buttons, tab strip controls.
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

    /// Back button state.
    pub back: ButtonState,

    /// Forward button state.
    pub forward: ButtonState,

    /// Whether a page is currently loading (controls reload/stop button).
    pub loading: bool,
}

impl Toolbar {
    /// Create a new toolbar with default state.
    pub fn new() -> Self {
        Self {
            address_text: String::new(),
            address_focused: false,
            back: ButtonState::Disabled,
            forward: ButtonState::Disabled,
            loading: false,
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
    }

    /// Update the toolbar state from the current tab state.
    pub fn sync_with_tab(
        &mut self,
        url: &Url,
        can_go_back: bool,
        can_go_forward: bool,
        loading: bool,
    ) {
        self.set_url(url);
        self.back = ButtonState::from_bool(can_go_back);
        self.forward = ButtonState::from_bool(can_go_forward);
        self.loading = loading;
    }

    /// Handle user typing in the address bar.
    pub fn handle_input(&mut self, text: &str) {
        self.address_text = text.to_owned();
        self.address_focused = true;
    }

    /// Submit the address bar (user pressed Enter). Returns the text
    /// that should be navigated to.
    pub fn submit_address(&mut self) -> String {
        self.address_focused = false;
        self.address_text.clone()
    }

    /// Focus the address bar and select all text (Cmd+L / Ctrl+L).
    pub fn focus_address_bar(&mut self) {
        self.address_focused = true;
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
        toolbar.sync_with_tab(&url, true, false, true);

        assert_eq!(toolbar.url_bar_text(), "https://example.com/");
        assert!(toolbar.back.is_enabled());
        assert!(!toolbar.forward.is_enabled());
        assert!(toolbar.loading);
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
}
