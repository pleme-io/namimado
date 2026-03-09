use serde::{Deserialize, Serialize};

/// Top-level configuration for namimado.
///
/// When the `gpu-chrome` feature is enabled, this is loaded via shikumi
/// from `~/.config/namimado/namimado.yaml`. Without shikumi, it uses
/// compiled-in defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamimadoConfig {
    /// Homepage URL opened for new tabs and on startup.
    #[serde(default = "default_homepage")]
    pub homepage: String,

    /// Default search engine URL template. `%s` is replaced with the query.
    #[serde(default = "default_search_engine")]
    pub search_engine: String,

    /// Whether developer tools are enabled by default.
    #[serde(default)]
    pub devtools_enabled: bool,

    /// Theme configuration.
    #[serde(default)]
    pub theme: ThemeConfig,

    /// Content blocking configuration.
    #[serde(default)]
    pub content_blocking: ContentBlockingConfig,

    /// Privacy configuration.
    #[serde(default)]
    pub privacy: PrivacyConfig,

    /// Sidebar configuration.
    #[serde(default)]
    pub sidebar: SidebarConfig,

    /// Download configuration.
    #[serde(default)]
    pub downloads: DownloadConfig,

    /// Permissions configuration.
    #[serde(default)]
    pub permissions: PermissionsConfig,
}

/// Theme configuration for the browser chrome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Whether to use the dark theme variant.
    #[serde(default = "default_true")]
    pub dark: bool,

    /// Font size in points for the browser chrome (toolbar, tabs, etc.).
    #[serde(default = "default_font_size")]
    pub font_size: f32,

    /// Opacity of the toolbar area (0.0 = transparent, 1.0 = opaque).
    #[serde(default = "default_opacity")]
    pub toolbar_opacity: f32,
}

/// Content blocking settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlockingConfig {
    /// Block third-party cookies.
    #[serde(default = "default_true")]
    pub block_third_party_cookies: bool,

    /// Block known trackers.
    #[serde(default = "default_true")]
    pub block_trackers: bool,

    /// Block ads.
    #[serde(default)]
    pub block_ads: bool,
}

/// Privacy settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// Clear browsing data on exit.
    #[serde(default)]
    pub clear_on_exit: bool,

    /// Send Do Not Track header.
    #[serde(default = "default_true")]
    pub do_not_track: bool,

    /// HTTPS-only mode — refuse to load HTTP pages.
    #[serde(default)]
    pub https_only: bool,
}

/// Sidebar configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarConfig {
    /// Whether the sidebar is visible by default.
    #[serde(default)]
    pub visible: bool,

    /// Which side the sidebar appears on.
    #[serde(default = "default_sidebar_position")]
    pub position: SidebarPosition,

    /// Width of the sidebar in pixels.
    #[serde(default = "default_sidebar_width")]
    pub width: u32,
}

/// Which side the sidebar is displayed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarPosition {
    Left,
    Right,
}

/// Download configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadConfig {
    /// Default download directory.
    #[serde(default = "default_download_dir")]
    pub directory: String,

    /// Whether to ask the user where to save each download.
    #[serde(default)]
    pub ask_location: bool,
}

/// Permission policy for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionPolicy {
    Allow,
    Deny,
    Ask,
}

/// Permissions configuration for web capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    /// Geolocation access policy.
    #[serde(default = "default_ask")]
    pub geolocation: PermissionPolicy,

    /// Notification permission policy.
    #[serde(default = "default_ask")]
    pub notifications: PermissionPolicy,

    /// Camera access policy.
    #[serde(default = "default_deny")]
    pub camera: PermissionPolicy,

    /// Microphone access policy.
    #[serde(default = "default_deny")]
    pub microphone: PermissionPolicy,
}

impl NamimadoConfig {
    /// Load configuration.
    ///
    /// With the `gpu-chrome` feature, uses shikumi to discover and load
    /// `~/.config/namimado/namimado.yaml`. Without it, returns defaults.
    pub fn load() -> Self {
        #[cfg(feature = "gpu-chrome")]
        {
            Self::load_shikumi()
        }
        #[cfg(not(feature = "gpu-chrome"))]
        {
            tracing::info!("using default configuration (shikumi not available)");
            Self::default()
        }
    }

    /// Load config via shikumi (only available with the `gpu-chrome` feature).
    #[cfg(feature = "gpu-chrome")]
    fn load_shikumi() -> Self {
        use tracing::{info, warn};
        match shikumi::ConfigDiscovery::new("namimado")
            .env_override("NAMIMADO_CONFIG")
            .discover()
        {
            Ok(path) => {
                info!(path = %path.display(), "loading config via shikumi");
                match shikumi::ConfigStore::<Self>::load(&path, "NAMIMADO_") {
                    Ok(store) => store.get().clone(),
                    Err(e) => {
                        warn!(error = %e, "failed to load config — using defaults");
                        Self::default()
                    }
                }
            }
            Err(e) => {
                info!(error = %e, "no config file found — using defaults");
                Self::default()
            }
        }
    }

    /// Build the search URL for a query using the configured search engine.
    #[must_use]
    pub fn search_url(&self, query: &str) -> String {
        let encoded = urlencoding::encode(query);
        self.search_engine.replace("%s", &encoded)
    }
}

impl Default for NamimadoConfig {
    fn default() -> Self {
        Self {
            homepage: default_homepage(),
            search_engine: default_search_engine(),
            devtools_enabled: false,
            theme: ThemeConfig::default(),
            content_blocking: ContentBlockingConfig::default(),
            privacy: PrivacyConfig::default(),
            sidebar: SidebarConfig::default(),
            downloads: DownloadConfig::default(),
            permissions: PermissionsConfig::default(),
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            dark: true,
            font_size: default_font_size(),
            toolbar_opacity: default_opacity(),
        }
    }
}

impl Default for ContentBlockingConfig {
    fn default() -> Self {
        Self {
            block_third_party_cookies: true,
            block_trackers: true,
            block_ads: false,
        }
    }
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            clear_on_exit: false,
            do_not_track: true,
            https_only: false,
        }
    }
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            visible: false,
            position: default_sidebar_position(),
            width: default_sidebar_width(),
        }
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            directory: default_download_dir(),
            ask_location: false,
        }
    }
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            geolocation: default_ask(),
            notifications: default_ask(),
            camera: default_deny(),
            microphone: default_deny(),
        }
    }
}

fn default_homepage() -> String {
    "about:blank".to_owned()
}

fn default_search_engine() -> String {
    "https://www.google.com/search?q=%s".to_owned()
}

const fn default_true() -> bool {
    true
}

const fn default_font_size() -> f32 {
    14.0
}

const fn default_opacity() -> f32 {
    1.0
}

const fn default_sidebar_position() -> SidebarPosition {
    SidebarPosition::Left
}

const fn default_sidebar_width() -> u32 {
    300
}

fn default_download_dir() -> String {
    std::env::var("HOME")
        .map(|h| format!("{h}/Downloads"))
        .unwrap_or_else(|_| "~/Downloads".to_owned())
}

const fn default_ask() -> PermissionPolicy {
    PermissionPolicy::Ask
}

const fn default_deny() -> PermissionPolicy {
    PermissionPolicy::Deny
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = NamimadoConfig::default();
        assert_eq!(config.homepage, "about:blank");
        assert!(config.search_engine.contains("%s"));
        assert!(!config.devtools_enabled);
        assert!(config.theme.dark);
    }

    #[test]
    fn serde_roundtrip() {
        let config = NamimadoConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let deserialized: NamimadoConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(deserialized.homepage, config.homepage);
        assert_eq!(deserialized.theme.dark, config.theme.dark);
    }

    #[test]
    fn sidebar_config_defaults() {
        let config = SidebarConfig::default();
        assert!(!config.visible);
        assert_eq!(config.position, SidebarPosition::Left);
        assert_eq!(config.width, 300);
    }

    #[test]
    fn download_config_defaults() {
        let config = DownloadConfig::default();
        assert!(!config.ask_location);
        assert!(config.directory.contains("Downloads"));
    }

    #[test]
    fn permissions_config_defaults() {
        let config = PermissionsConfig::default();
        assert_eq!(config.geolocation, PermissionPolicy::Ask);
        assert_eq!(config.camera, PermissionPolicy::Deny);
    }

    #[test]
    fn search_url_encoding() {
        let config = NamimadoConfig::default();
        let url = config.search_url("rust programming");
        assert!(url.contains("google.com"));
        assert!(url.contains("rust"));
    }

    #[test]
    fn sidebar_position_serde() {
        let yaml = "\"left\"";
        let pos: SidebarPosition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(pos, SidebarPosition::Left);

        let yaml = "\"right\"";
        let pos: SidebarPosition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(pos, SidebarPosition::Right);
    }

    #[test]
    fn permission_policy_serde() {
        let yaml = "\"ask\"";
        let policy: PermissionPolicy = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(policy, PermissionPolicy::Ask);
    }
}
