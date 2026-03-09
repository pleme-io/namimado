use anyhow::Result;
use url::Url;

use super::tab::Tab;

/// Normalize user input into a valid URL.
///
/// - If the input parses as a valid URL, use it directly.
/// - If it looks like a bare domain (contains a dot, no spaces), prepend `https://`.
/// - Otherwise, treat it as a search query.
pub fn normalize_url(input: &str) -> Result<Url> {
    let trimmed = input.trim();

    // Already a valid URL
    if let Ok(url) = Url::parse(trimmed) {
        if url.scheme() == "http" || url.scheme() == "https" || url.scheme() == "about" {
            return Ok(url);
        }
    }

    // Looks like a bare domain (e.g. "example.com" or "example.com/path")
    if !trimmed.contains(' ') && trimmed.contains('.') {
        if let Ok(url) = Url::parse(&format!("https://{trimmed}")) {
            return Ok(url);
        }
    }

    // Fall back to a search query
    let encoded = urlencoding::encode(trimmed);
    let search_url = format!("https://www.google.com/search?q={encoded}");
    Url::parse(&search_url).map_err(|e| anyhow::anyhow!("failed to build search URL: {e}"))
}

/// Navigate a tab to a new URL, pushing the current URL onto the back stack.
pub fn navigate(tab: &mut Tab, url: Url) {
    tracing::info!(tab = %tab.id, url = %url, "navigating");
    tab.push_navigation(url);
    tab.loading = true;
}

/// Move the tab back in history. Returns the URL to navigate the webview to.
pub fn go_back(tab: &mut Tab) -> Option<Url> {
    tab.go_back().cloned()
}

/// Move the tab forward in history. Returns the URL to navigate the webview to.
pub fn go_forward(tab: &mut Tab) -> Option<Url> {
    tab.go_forward().cloned()
}

/// Mark the tab as reloading (the webview is responsible for actually reloading).
pub fn reload(tab: &mut Tab) {
    tracing::info!(tab = %tab.id, "reload");
    tab.loading = true;
}

/// Cancel ongoing load.
pub fn stop(tab: &mut Tab) {
    tracing::info!(tab = %tab.id, "stop");
    tab.loading = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_full_url() {
        let url = normalize_url("https://example.com/page").unwrap();
        assert_eq!(url.as_str(), "https://example.com/page");
    }

    #[test]
    fn normalize_bare_domain() {
        let url = normalize_url("example.com").unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("example.com"));
    }

    #[test]
    fn normalize_search_query() {
        let url = normalize_url("rust programming").unwrap();
        assert!(url.as_str().contains("google.com/search"));
        assert!(url.as_str().contains("rust"));
    }

    #[test]
    fn normalize_about_blank() {
        let url = normalize_url("about:blank").unwrap();
        assert_eq!(url.as_str(), "about:blank");
    }

    #[test]
    fn navigate_pushes_history() {
        let mut tab = Tab::new(Url::parse("https://one.com").unwrap());
        navigate(&mut tab, Url::parse("https://two.com").unwrap());
        assert_eq!(tab.url.as_str(), "https://two.com/");
        assert!(tab.can_go_back);
        assert!(tab.loading);
    }

    #[test]
    fn back_forward_round_trip() {
        let mut tab = Tab::new(Url::parse("https://one.com").unwrap());
        navigate(&mut tab, Url::parse("https://two.com").unwrap());

        let back = go_back(&mut tab).unwrap();
        assert_eq!(back.as_str(), "https://one.com/");

        let fwd = go_forward(&mut tab).unwrap();
        assert_eq!(fwd.as_str(), "https://two.com/");
    }
}
