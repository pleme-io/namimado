use super::layout::ChromeLayout;
use crate::browser::bookmark::BookmarkManager;
use crate::browser::download::DownloadManager;
use crate::browser::tabs::TabManager;
use crate::chrome::sidebar::{Sidebar, SidebarPanel};
use crate::chrome::statusbar::StatusBar;
use crate::chrome::toolbar::Toolbar;

/// A renderable frame of the browser chrome.
///
/// This struct captures the state needed to render one frame of the
/// browser chrome (tab bar, toolbar, sidebar, status bar). When the
/// `gpu-chrome` feature is enabled, this drives garasu/egaku rendering.
/// Without it, the struct still serves as a testable snapshot of
/// what *would* be rendered.
#[derive(Debug)]
pub struct ChromeFrame {
    /// Computed layout rectangles.
    pub layout: ChromeLayout,

    /// Tab bar entries to render.
    pub tabs: Vec<TabEntry>,
    /// Index of the active tab in the `tabs` vec.
    pub active_tab_index: usize,

    /// Toolbar state.
    pub address_text: String,
    pub address_focused: bool,
    pub back_enabled: bool,
    pub forward_enabled: bool,
    pub loading: bool,
    pub is_bookmarked: bool,
    pub active_downloads: usize,

    /// Sidebar state.
    pub sidebar_visible: bool,
    pub sidebar_panel: SidebarPanel,

    /// Status bar state.
    pub status_text: String,
    pub security_label: &'static str,
    pub progress: Option<f32>,
}

/// A tab entry for the tab bar renderer.
#[derive(Debug, Clone)]
pub struct TabEntry {
    pub title: String,
    pub pinned: bool,
    pub loading: bool,
    pub active: bool,
}

impl ChromeFrame {
    /// Build a chrome frame from the current application state.
    #[must_use]
    pub fn build(
        window_width: f32,
        window_height: f32,
        tab_manager: &TabManager,
        toolbar: &Toolbar,
        sidebar: &Sidebar,
        status_bar: &StatusBar,
        _bookmarks: &BookmarkManager,
        _downloads: &DownloadManager,
    ) -> Self {
        let layout = ChromeLayout::compute(
            window_width,
            window_height,
            sidebar.visible,
            f32::from(sidebar.width as u16),
            true, // default left sidebar
            toolbar.show_bookmark_bar,
        );

        let active_idx = tab_manager.active_index();
        let tabs: Vec<TabEntry> = tab_manager
            .iter()
            .enumerate()
            .map(|(i, tab)| TabEntry {
                title: if tab.pinned {
                    // Pinned tabs show a truncated title
                    tab.title.chars().take(3).collect()
                } else {
                    tab.title.clone()
                },
                pinned: tab.pinned,
                loading: tab.loading,
                active: i == active_idx,
            })
            .collect();

        Self {
            layout,
            tabs,
            active_tab_index: active_idx,
            address_text: toolbar.url_bar_text().to_owned(),
            address_focused: toolbar.address_focused,
            back_enabled: toolbar.back.is_enabled(),
            forward_enabled: toolbar.forward.is_enabled(),
            loading: toolbar.loading,
            is_bookmarked: toolbar.is_bookmarked,
            active_downloads: toolbar.active_downloads,
            sidebar_visible: sidebar.visible,
            sidebar_panel: sidebar.panel,
            status_text: status_bar.display_text().to_owned(),
            security_label: status_bar.security.label(),
            progress: status_bar.progress,
        }
    }

    /// Render the chrome frame.
    ///
    /// With `gpu-chrome` feature, this would invoke garasu/egaku rendering.
    /// Currently logs the frame state for debugging.
    pub fn render(&self) {
        tracing::trace!(
            tabs = self.tabs.len(),
            active = self.active_tab_index,
            sidebar = self.sidebar_visible,
            "chrome frame rendered"
        );

        // GPU rendering would happen here:
        // 1. Clear the chrome areas with the theme background color
        // 2. Render tab bar: each TabEntry as a clickable tab widget
        // 3. Render toolbar: back/forward buttons, address bar, bookmark star,
        //    download indicator
        // 4. Render bookmark bar (if visible): bookmark entries from BookmarkManager
        // 5. Render sidebar (if visible): panel contents based on sidebar_panel
        // 6. Render status bar: security indicator, status text, progress bar
        // 7. The content area is left for Servo to render into
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use url::Url;

    #[test]
    fn build_chrome_frame() {
        let mut tabs = TabManager::new();
        tabs.add_tab(Url::parse("https://example.com").unwrap());
        tabs.add_tab(Url::parse("https://rust-lang.org").unwrap());

        let mut toolbar = Toolbar::new();
        toolbar.sync_with_tab(
            &Url::parse("https://rust-lang.org").unwrap(),
            true,
            false,
            false,
            false,
        );

        let sidebar = Sidebar::new();
        let status_bar = StatusBar::new();
        let bookmarks = BookmarkManager::new();
        let downloads = DownloadManager::new(PathBuf::from("/tmp"));

        let frame = ChromeFrame::build(
            1280.0,
            800.0,
            &tabs,
            &toolbar,
            &sidebar,
            &status_bar,
            &bookmarks,
            &downloads,
        );

        assert_eq!(frame.tabs.len(), 2);
        assert_eq!(frame.active_tab_index, 1);
        assert!(frame.back_enabled);
        assert!(!frame.forward_enabled);
        assert!(!frame.loading);
        assert!(!frame.sidebar_visible);
    }

    #[test]
    fn pinned_tab_truncated_title() {
        let mut tabs = TabManager::new();
        let id = tabs.add_tab(Url::parse("https://example.com").unwrap());
        tabs.get_tab_mut(id).unwrap().title = "Example Website".into();
        tabs.get_tab_mut(id).unwrap().pinned = true;

        let toolbar = Toolbar::new();
        let sidebar = Sidebar::new();
        let status_bar = StatusBar::new();
        let bookmarks = BookmarkManager::new();
        let downloads = DownloadManager::new(PathBuf::from("/tmp"));

        let frame = ChromeFrame::build(
            1280.0,
            800.0,
            &tabs,
            &toolbar,
            &sidebar,
            &status_bar,
            &bookmarks,
            &downloads,
        );

        assert_eq!(frame.tabs[0].title, "Exa");
        assert!(frame.tabs[0].pinned);
    }
}
