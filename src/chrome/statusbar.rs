use url::Url;

/// Security level of the current page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityLevel {
    /// The page is loaded over HTTPS with a valid certificate.
    Secure,
    /// The page is loaded over HTTP (no encryption).
    Insecure,
    /// The page has mixed content (HTTPS page with HTTP subresources).
    Mixed,
    /// Internal page (about:blank, etc.).
    Internal,
}

impl SecurityLevel {
    /// Determine security level from a URL.
    #[must_use]
    pub fn from_url(url: &Url) -> Self {
        match url.scheme() {
            "https" => Self::Secure,
            "http" => Self::Insecure,
            "about" | "data" => Self::Internal,
            _ => Self::Insecure,
        }
    }

    /// A human-readable label for the security level.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Secure => "Secure",
            Self::Insecure => "Not Secure",
            Self::Mixed => "Mixed Content",
            Self::Internal => "",
        }
    }
}

/// The bottom status bar showing loading progress and security indicators.
#[derive(Debug, Clone)]
pub struct StatusBar {
    /// Current loading progress (0.0 to 1.0), or `None` if not loading.
    pub progress: Option<f32>,

    /// Security level of the current page.
    pub security: SecurityLevel,

    /// Hover link target — shows the URL the cursor is hovering over.
    pub hover_url: Option<String>,

    /// Status text (e.g., "Connecting...", "Transferring data...").
    pub status_text: Option<String>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            progress: None,
            security: SecurityLevel::Internal,
            hover_url: None,
            status_text: None,
        }
    }

    /// Update from a page load start event.
    pub fn on_load_start(&mut self, url: &Url) {
        self.progress = Some(0.0);
        self.security = SecurityLevel::from_url(url);
        self.status_text = Some(format!("Loading {}...", url.host_str().unwrap_or("")));
    }

    /// Update loading progress.
    pub fn on_progress(&mut self, fraction: f32) {
        self.progress = Some(fraction.clamp(0.0, 1.0));
    }

    /// Update from a page load completion event.
    pub fn on_load_end(&mut self, url: &Url) {
        self.progress = None;
        self.security = SecurityLevel::from_url(url);
        self.status_text = None;
    }

    /// Update the hover URL (shown when the user hovers over a link).
    pub fn set_hover_url(&mut self, url: Option<String>) {
        self.hover_url = url;
    }

    /// The text to display in the status bar.
    #[must_use]
    pub fn display_text(&self) -> &str {
        if let Some(ref hover) = self.hover_url {
            return hover;
        }
        if let Some(ref status) = self.status_text {
            return status;
        }
        ""
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_from_url() {
        assert_eq!(
            SecurityLevel::from_url(&Url::parse("https://x.com").unwrap()),
            SecurityLevel::Secure
        );
        assert_eq!(
            SecurityLevel::from_url(&Url::parse("http://x.com").unwrap()),
            SecurityLevel::Insecure
        );
        assert_eq!(
            SecurityLevel::from_url(&Url::parse("about:blank").unwrap()),
            SecurityLevel::Internal
        );
    }

    #[test]
    fn load_lifecycle() {
        let mut bar = StatusBar::new();
        let url = Url::parse("https://example.com").unwrap();

        bar.on_load_start(&url);
        assert!(bar.progress.is_some());
        assert_eq!(bar.security, SecurityLevel::Secure);

        bar.on_progress(0.5);
        assert!((bar.progress.unwrap() - 0.5).abs() < f32::EPSILON);

        bar.on_load_end(&url);
        assert!(bar.progress.is_none());
    }

    #[test]
    fn hover_url_takes_priority() {
        let mut bar = StatusBar::new();
        bar.status_text = Some("Loading...".into());
        bar.set_hover_url(Some("https://link.com".into()));
        assert_eq!(bar.display_text(), "https://link.com");
    }
}
