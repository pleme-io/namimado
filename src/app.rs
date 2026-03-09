use anyhow::Result;
use tracing::info;
use url::Url;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::browser::tabs::TabManager;
use crate::config::NamimadoConfig;
use crate::ipc::bridge::IpcMessage;
use crate::webview::engine::WebViewEngine;

/// Top-level application state holding the tab manager, webview engine,
/// and IPC bridge. This struct coordinates between the browser model layer
/// and the platform rendering layer.
pub struct App {
    pub tabs: TabManager,
    pub engine: Option<WebViewEngine>,
    pub config: NamimadoConfig,
    pub window: Option<Window>,
    initial_url: String,
    devtools: bool,
}

impl App {
    /// Create a new application instance (window is created lazily on resume).
    pub fn new(initial_url: &str, devtools: bool) -> Result<Self> {
        let config = NamimadoConfig::load();
        let mut tabs = TabManager::new();

        let url = if initial_url == "about:blank" && config.homepage != "about:blank" {
            config.homepage.clone()
        } else {
            initial_url.to_owned()
        };

        let parsed = crate::browser::navigation::normalize_url(&url)?;
        tabs.add_tab(parsed);

        Ok(Self {
            tabs,
            engine: None,
            config,
            window: None,
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
                }
            }
            IpcMessage::TitleChanged { title } => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.title = title;
                }
            }
            IpcMessage::LoadStart => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.loading = true;
                }
            }
            IpcMessage::LoadEnd => {
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.loading = false;
                }
            }
            IpcMessage::FaviconChanged { .. } => {
                // TODO: update tab favicon
            }
        }
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
            WindowEvent::Resized(_size) => {
                // TODO: resize web engine viewport + GPU chrome
            }
            WindowEvent::RedrawRequested => {
                // TODO: render GPU chrome (toolbar, tabs, sidebar) via garasu
                // The web content area will be composited by Servo.
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
