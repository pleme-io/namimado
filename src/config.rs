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
}
