use std::path::PathBuf;

use anyhow::Result;
use tracing::info;
use url::Url;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::browser::bookmark::BookmarkManager;
use crate::browser::download::DownloadManager;
use crate::browser::history::HistoryManager;
use crate::browser::tabs::TabManager;
use crate::chrome::sidebar::{Sidebar, SidebarPanel};
use crate::chrome::statusbar::StatusBar;
use crate::chrome::toolbar::{AddressSuggestion, SuggestionSource, Toolbar};
use crate::config::NamimadoConfig;
use crate::input::keybindings::{BrowserAction, KeybindingManager};
use crate::ipc::bridge::IpcMessage;
use crate::render::chrome::ChromeFrame;
use crate::webview::engine::WebViewEngine;

/// Top-level application state holding all browser subsystems.
///
/// Coordinates between the browser model layer (tabs, bookmarks, history,
/// downloads), the chrome UI layer (toolbar, sidebar, status bar), the
/// input system (keybindings), and the platform rendering layer (webview
/// engine, GPU chrome).
pub struct App {
    // Browser model
    pub tabs: TabManager,
    pub bookmarks: BookmarkManager,
    pub history: HistoryManager,
    pub downloads: DownloadManager,

    // Chrome UI state
    pub toolbar: Toolbar,
    pub sidebar: Sidebar,
    pub status_bar: StatusBar,

    // Input
    pub keybindings: KeybindingManager,

    // Engine
    pub engine: Option<WebViewEngine>,
    pub config: NamimadoConfig,
    pub window: Option<Window>,

    // Window dimensions (logical pixels)
    window_width: f32,
    window_height: f32,

    initial_url: String,
    devtools: bool,
}

impl App {
    /// Create a new application instance (window is created lazily on resume).
    pub fn new(initial_url: &str, devtools: bool) -> Result<Self> {
        let config = NamimadoConfig::load();

        let mut tabs = TabManager::new();
        let bookmarks = BookmarkManager::new();
        let history = HistoryManager::new();
        let download_dir = PathBuf::from(&config.downloads.directory);
        let mut downloads = DownloadManager::new(download_dir);
        downloads.ask_location = config.downloads.ask_location;

        let mut toolbar = Toolbar::new();
        let mut sidebar = Sidebar::new();
        sidebar.visible = config.sidebar.visible;
        sidebar.width = config.sidebar.width;

        let status_bar = StatusBar::new();
        let keybindings = KeybindingManager::new();

        let url = if initial_url == "about:blank" && config.homepage != "about:blank" {
            config.homepage.clone()
        } else {
            initial_url.to_owned()
        };

        let parsed = crate::browser::navigation::normalize_url(&url)?;
        tabs.add_tab(parsed.clone());
        toolbar.set_url(&parsed);

        Ok(Self {
            tabs,
            bookmarks,
            history,
            downloads,
            toolbar,
            sidebar,
            status_bar,
            keybindings,
            engine: None,
            config,
            window: None,
            window_width: 1280.0,
            window_height: 800.0,
            initial_url: url,
            devtools,
        })
    }

    /// Handle an IPC message arriving from the webview.
    pub fn handle_ipc(&mut self, message: IpcMessage) {
        match message {
            IpcMessage::Navigate { url } => {
                if let Ok(parsed) = crate::browser::navigation::normalize_url(&url) {
                    if let Some(tab) = self.tabs.active_tab_mut() {
                        crate::browser::navigation::navigate(tab, parsed.clone());
                    }
                    if let Some(engine) = &mut self.engine {
                        engine.navigate(&parsed);
                    }
                    self.sync_toolbar();
                }
            }
            IpcMessage::TitleChanged { title } => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.title = title.clone();
                    // Record the visit in history
                    let url = tab.url.clone();
                    self.history.record_visit(&title, url);
                }
            }
            IpcMessage::LoadStart => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.loading = true;
                    let url = tab.url.clone();
                    self.status_bar.on_load_start(&url);
                    self.sync_toolbar();
                }
            }
            IpcMessage::LoadEnd => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.loading = false;
                    let url = tab.url.clone();
                    self.status_bar.on_load_end(&url);
                    self.sync_toolbar();
                }
            }
            IpcMessage::FaviconChanged { .. } => {
                // TODO: update tab favicon
            }
        }
    }

    /// Handle a browser action (from keyboard shortcut or other input).
    pub fn handle_action(&mut self, action: BrowserAction) {
        match action {
            // -- Tab management --
            BrowserAction::NewTab => {
                let url = crate::browser::navigation::normalize_url(&self.config.homepage)
                    .unwrap_or_else(|_| Url::parse("about:blank").unwrap());
                self.tabs.add_tab_after_active(url.clone());
                self.sync_toolbar();
                // Focus address bar *after* sync so sync doesn't clear it
                self.toolbar.focus_address_bar();
            }
            BrowserAction::CloseTab => {
                self.tabs.close_active_tab();
                if self.tabs.is_empty() {
                    // Open a new blank tab instead of closing the window
                    let url = Url::parse("about:blank").unwrap();
                    self.tabs.add_tab(url);
                }
                self.sync_toolbar();
                self.navigate_engine_to_active();
            }
            BrowserAction::NextTab => {
                self.tabs.next_tab();
                self.sync_toolbar();
                self.navigate_engine_to_active();
            }
            BrowserAction::PrevTab => {
                self.tabs.prev_tab();
                self.sync_toolbar();
                self.navigate_engine_to_active();
            }
            BrowserAction::SwitchToTab(index) => {
                if self.tabs.switch_to_index(index) {
                    self.sync_toolbar();
                    self.navigate_engine_to_active();
                }
            }
            BrowserAction::RestoreClosedTab => {
                if self.tabs.restore_closed_tab().is_some() {
                    self.sync_toolbar();
                    self.navigate_engine_to_active();
                }
            }
            BrowserAction::DuplicateTab => {
                if self.tabs.duplicate_active_tab().is_some() {
                    self.sync_toolbar();
                    self.navigate_engine_to_active();
                }
            }
            BrowserAction::PinTab => {
                if let Some(tab) = self.tabs.active_tab() {
                    let id = tab.id;
                    self.tabs.toggle_pin(id);
                }
            }

            // -- Navigation --
            BrowserAction::FocusAddressBar => {
                self.toolbar.focus_address_bar();
                self.keybindings.address_bar_focused = true;
            }
            BrowserAction::GoBack => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    if let Some(url) = crate::browser::navigation::go_back(tab) {
                        let url = url.clone();
                        if let Some(engine) = &mut self.engine {
                            engine.navigate(&url);
                        }
                    }
                }
                self.sync_toolbar();
            }
            BrowserAction::GoForward => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    if let Some(url) = crate::browser::navigation::go_forward(tab) {
                        let url = url.clone();
                        if let Some(engine) = &mut self.engine {
                            engine.navigate(&url);
                        }
                    }
                }
                self.sync_toolbar();
            }
            BrowserAction::Reload | BrowserAction::ReloadHard => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    crate::browser::navigation::reload(tab);
                    let url = tab.url.clone();
                    if let Some(engine) = &mut self.engine {
                        engine.navigate(&url);
                    }
                }
                self.sync_toolbar();
            }
            BrowserAction::Stop => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    crate::browser::navigation::stop(tab);
                }
                self.sync_toolbar();
            }

            // -- Bookmarks --
            BrowserAction::BookmarkPage => {
                if let Some(tab) = self.tabs.active_tab() {
                    let url = tab.url.clone();
                    let title = tab.title.clone();
                    let is_now_bookmarked = self.bookmarks.toggle(title, url);
                    self.toolbar.is_bookmarked = is_now_bookmarked;
                }
            }
            BrowserAction::ToggleBookmarkBar => {
                self.toolbar.toggle_bookmark_bar();
            }
            BrowserAction::ShowBookmarks => {
                self.sidebar.show(SidebarPanel::Bookmarks);
            }

            // -- Find --
            BrowserAction::FindOnPage => {
                // TODO: implement find-on-page UI
                info!("find on page (not yet implemented)");
            }

            // -- Sidebar --
            BrowserAction::ToggleSidebar => {
                self.sidebar.toggle();
            }
            BrowserAction::ShowHistory => {
                self.sidebar.show(SidebarPanel::History);
            }
            BrowserAction::ShowDownloads => {
                self.sidebar.show(SidebarPanel::Downloads);
            }

            // -- Vim mode --
            BrowserAction::ToggleVimMode => {
                self.keybindings.toggle_vim();
                info!(vim_enabled = self.keybindings.vim_enabled, "vim mode toggled");
            }

            // -- Window --
            BrowserAction::ZoomIn => {
                info!("zoom in (not yet implemented)");
            }
            BrowserAction::ZoomOut => {
                info!("zoom out (not yet implemented)");
            }
            BrowserAction::ZoomReset => {
                info!("zoom reset (not yet implemented)");
            }
            BrowserAction::ToggleFullscreen => {
                info!("toggle fullscreen (not yet implemented)");
            }

            // -- Developer tools --
            BrowserAction::ToggleDevTools => {
                info!("toggle devtools (not yet implemented)");
            }

            // -- Address bar --
            BrowserAction::AddressBarSubmit => {
                let text = self.toolbar.submit_address();
                self.keybindings.address_bar_focused = false;
                if let Ok(url) = crate::browser::navigation::normalize_url(&text) {
                    if let Some(tab) = self.tabs.active_tab_mut() {
                        crate::browser::navigation::navigate(tab, url.clone());
                    }
                    if let Some(engine) = &mut self.engine {
                        engine.navigate(&url);
                    }
                    self.sync_toolbar();
                }
            }
            BrowserAction::AddressBarDismiss => {
                self.toolbar.dismiss_address_bar();
                self.keybindings.address_bar_focused = false;
                self.sync_toolbar();
            }
            BrowserAction::AddressBarUp => {
                self.toolbar.select_prev_suggestion();
            }
            BrowserAction::AddressBarDown => {
                self.toolbar.select_next_suggestion();
            }
        }
    }

    /// Synchronize the toolbar state with the currently active tab.
    fn sync_toolbar(&mut self) {
        if let Some(tab) = self.tabs.active_tab() {
            let url = tab.url.clone();
            let can_back = tab.can_go_back;
            let can_fwd = tab.can_go_forward;
            let loading = tab.loading;
            let is_bm = self.bookmarks.is_bookmarked(&url);
            self.toolbar
                .sync_with_tab(&url, can_back, can_fwd, loading, is_bm);
        }
        self.toolbar
            .set_active_downloads(self.downloads.active_count());
    }

    /// Navigate the webview engine to the currently active tab's URL.
    fn navigate_engine_to_active(&mut self) {
        if let Some(tab) = self.tabs.active_tab() {
            let url = tab.url.clone();
            if let Some(engine) = &mut self.engine {
                engine.navigate(&url);
            }
        }
    }

    /// Update autocomplete suggestions for the address bar based on typing.
    pub fn update_autocomplete(&mut self) {
        let text = self.toolbar.url_bar_text();
        if text.is_empty() {
            self.toolbar.set_suggestions(Vec::new());
            return;
        }

        let mut suggestions = Vec::new();

        // History suggestions
        for entry in self.history.autocomplete(text) {
            suggestions.push(AddressSuggestion {
                url: entry.url.as_str().to_owned(),
                title: entry.title.clone(),
                source: SuggestionSource::History,
            });
        }

        // Bookmark suggestions
        for bm in self.bookmarks.search(text) {
            // Avoid duplicates from history
            if !suggestions.iter().any(|s| s.url == bm.url.as_str()) {
                suggestions.push(AddressSuggestion {
                    url: bm.url.as_str().to_owned(),
                    title: bm.title.clone(),
                    source: SuggestionSource::Bookmark,
                });
            }
        }

        // Limit to 10 suggestions
        suggestions.truncate(10);

        self.toolbar.set_suggestions(suggestions);
    }

    /// Build a chrome frame snapshot for rendering.
    #[must_use]
    pub fn build_chrome_frame(&self) -> ChromeFrame {
        ChromeFrame::build(
            self.window_width,
            self.window_height,
            &self.tabs,
            &self.toolbar,
            &self.sidebar,
            &self.status_bar,
            &self.bookmarks,
            &self.downloads,
        )
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("namimado")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0_f64, 800.0));

        match event_loop.create_window(attrs) {
            Ok(window) => {
                info!("window created");

                // Store window size
                let size = window.inner_size();
                #[allow(clippy::cast_precision_loss)]
                {
                    self.window_width = size.width as f32;
                    self.window_height = size.height as f32;
                }

                self.window = Some(window);

                // Initialize the web engine scaffold
                let start_url = self
                    .tabs
                    .active_tab()
                    .map_or_else(
                        || Url::parse("about:blank").unwrap(),
                        |t| t.url.clone(),
                    );
                match WebViewEngine::new(&start_url, self.devtools) {
                    Ok(engine) => {
                        info!(url = %start_url, "webview engine ready");
                        self.engine = Some(engine);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "failed to create webview engine");
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to create window");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("close requested — shutting down");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                #[allow(clippy::cast_precision_loss)]
                {
                    self.window_width = size.width as f32;
                    self.window_height = size.height as f32;
                }
                // TODO: resize web engine viewport + GPU chrome
            }
            WindowEvent::RedrawRequested => {
                // Build and render the chrome frame
                let frame = self.build_chrome_frame();
                frame.render();

                // Request next frame
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let modifiers = winit::event::Modifiers::default();
                if let Some(action) =
                    self.keybindings
                        .process_key(&event.logical_key, event.state, &modifiers)
                {
                    self.handle_action(action);
                }
            }
            WindowEvent::ModifiersChanged(_modifiers) => {
                // Modifier state is tracked by winit internally
            }
            _ => {}
        }
    }
}

/// Main entry point: creates the winit event loop, builds the application
/// state, and runs the event loop until the user closes the window.
pub fn run(initial_url: &str, devtools: bool) -> Result<()> {
    info!("namimado starting — url={initial_url} devtools={devtools}");

    let mut app = App::new(initial_url, devtools)?;
    let event_loop = EventLoop::new()?;

    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_new_creates_tab() {
        let app = App::new("https://example.com", false).unwrap();
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(
            app.tabs.active_tab().unwrap().url.as_str(),
            "https://example.com/"
        );
    }

    #[test]
    fn app_uses_homepage_when_about_blank() {
        // Default config has homepage = "about:blank", so this should stay
        let app = App::new("about:blank", false).unwrap();
        assert_eq!(
            app.tabs.active_tab().unwrap().url.as_str(),
            "about:blank"
        );
    }

    #[test]
    fn handle_action_new_tab() {
        let mut app = App::new("https://example.com", false).unwrap();
        app.handle_action(BrowserAction::NewTab);
        assert_eq!(app.tabs.len(), 2);
        assert!(app.toolbar.address_focused);
    }

    #[test]
    fn handle_action_close_tab_keeps_one() {
        let mut app = App::new("https://example.com", false).unwrap();
        app.handle_action(BrowserAction::CloseTab);
        // Should not have zero tabs — a blank tab is created
        assert_eq!(app.tabs.len(), 1);
    }

    #[test]
    fn handle_action_bookmark_toggle() {
        let mut app = App::new("https://example.com", false).unwrap();
        assert!(!app.toolbar.is_bookmarked);

        app.handle_action(BrowserAction::BookmarkPage);
        assert!(app.toolbar.is_bookmarked);
        assert!(app.bookmarks.is_bookmarked(
            &Url::parse("https://example.com").unwrap()
        ));

        app.handle_action(BrowserAction::BookmarkPage);
        assert!(!app.toolbar.is_bookmarked);
    }

    #[test]
    fn handle_action_back_forward() {
        let mut app = App::new("https://one.com", false).unwrap();
        // Navigate to a second page
        if let Some(tab) = app.tabs.active_tab_mut() {
            crate::browser::navigation::navigate(
                tab,
                Url::parse("https://two.com").unwrap(),
            );
        }
        app.sync_toolbar();
        assert!(app.toolbar.back.is_enabled());

        app.handle_action(BrowserAction::GoBack);
        assert_eq!(
            app.tabs.active_tab().unwrap().url.as_str(),
            "https://one.com/"
        );

        app.handle_action(BrowserAction::GoForward);
        assert_eq!(
            app.tabs.active_tab().unwrap().url.as_str(),
            "https://two.com/"
        );
    }

    #[test]
    fn handle_action_restore_closed() {
        let mut app = App::new("https://one.com", false).unwrap();
        let id2 = app.tabs.add_tab(Url::parse("https://two.com").unwrap());
        app.tabs.close_tab(id2);
        assert_eq!(app.tabs.len(), 1);

        app.handle_action(BrowserAction::RestoreClosedTab);
        assert_eq!(app.tabs.len(), 2);
    }

    #[test]
    fn handle_action_address_bar_flow() {
        let mut app = App::new("https://example.com", false).unwrap();

        app.handle_action(BrowserAction::FocusAddressBar);
        assert!(app.toolbar.address_focused);
        assert!(app.keybindings.address_bar_focused);

        app.toolbar.handle_input("rust-lang.org");
        app.handle_action(BrowserAction::AddressBarSubmit);
        assert!(!app.toolbar.address_focused);
        assert_eq!(
            app.tabs.active_tab().unwrap().url.as_str(),
            "https://rust-lang.org/"
        );
    }

    #[test]
    fn handle_action_sidebar() {
        let mut app = App::new("https://example.com", false).unwrap();
        assert!(!app.sidebar.visible);

        app.handle_action(BrowserAction::ShowHistory);
        assert!(app.sidebar.visible);
        assert_eq!(app.sidebar.panel, SidebarPanel::History);

        app.handle_action(BrowserAction::ToggleSidebar);
        assert!(!app.sidebar.visible);
    }

    #[test]
    fn autocomplete_from_history() {
        let mut app = App::new("https://example.com", false).unwrap();
        app.history.record_visit(
            "Rust",
            Url::parse("https://rust-lang.org").unwrap(),
        );
        app.history.record_visit(
            "Go",
            Url::parse("https://go.dev").unwrap(),
        );

        app.toolbar.handle_input("rust");
        app.update_autocomplete();
        assert_eq!(app.toolbar.suggestions.len(), 1);
        assert_eq!(app.toolbar.suggestions[0].title, "Rust");
    }

    #[test]
    fn ipc_title_changed_records_history() {
        let mut app = App::new("https://example.com", false).unwrap();
        app.handle_ipc(IpcMessage::TitleChanged {
            title: "Example Domain".into(),
        });

        assert_eq!(
            app.tabs.active_tab().unwrap().title,
            "Example Domain"
        );
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn ipc_load_lifecycle() {
        let mut app = App::new("https://example.com", false).unwrap();

        app.handle_ipc(IpcMessage::LoadStart);
        assert!(app.tabs.active_tab().unwrap().loading);
        assert!(app.status_bar.progress.is_some());

        app.handle_ipc(IpcMessage::LoadEnd);
        assert!(!app.tabs.active_tab().unwrap().loading);
        assert!(app.status_bar.progress.is_none());
    }

    #[test]
    fn chrome_frame_builds_successfully() {
        let app = App::new("https://example.com", false).unwrap();
        let frame = app.build_chrome_frame();
        assert_eq!(frame.tabs.len(), 1);
    }
}
