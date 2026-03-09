//! Rhai scripting plugin system.
//!
//! Loads user scripts from `~/.config/namimado/scripts/*.rhai` and registers
//! app-specific functions for desktop browser automation.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use soushi::ScriptEngine;

/// Event hooks that scripts can define.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEvent {
    /// Fired when the browser starts.
    OnStart,
    /// Fired when the browser is quitting.
    OnQuit,
    /// Fired on key press with the key name.
    OnKey(String),
}

/// Manages the Rhai scripting engine with namimado-specific functions.
pub struct NamimadoScriptEngine {
    engine: ScriptEngine,
    /// Shared state for script-triggered actions.
    pub pending_actions: Arc<Mutex<Vec<ScriptAction>>>,
}

/// Actions that scripts can trigger.
#[derive(Debug, Clone)]
pub enum ScriptAction {
    /// Open a new tab with a URL.
    NewTab(String),
    /// Close the current tab.
    CloseTab,
    /// Navigate the current tab to a URL.
    Navigate(String),
}

impl NamimadoScriptEngine {
    /// Create a new scripting engine with namimado-specific functions registered.
    #[must_use]
    pub fn new() -> Self {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_log();
        engine.register_builtin_env();
        engine.register_builtin_string();

        let pending = Arc::new(Mutex::new(Vec::<ScriptAction>::new()));

        // Register namimado.new_tab(url)
        let p = Arc::clone(&pending);
        engine.register_fn("namimado_new_tab", move |url: &str| {
            if let Ok(mut actions) = p.lock() {
                actions.push(ScriptAction::NewTab(url.to_string()));
            }
        });

        // Register namimado.close_tab()
        let p = Arc::clone(&pending);
        engine.register_fn("namimado_close_tab", move || {
            if let Ok(mut actions) = p.lock() {
                actions.push(ScriptAction::CloseTab);
            }
        });

        // Register namimado.navigate(url)
        let p = Arc::clone(&pending);
        engine.register_fn("namimado_navigate", move |url: &str| {
            if let Ok(mut actions) = p.lock() {
                actions.push(ScriptAction::Navigate(url.to_string()));
            }
        });

        // Register namimado.get_url() — returns empty string (placeholder)
        engine.register_fn("namimado_get_url", || -> String {
            String::new()
        });

        Self {
            engine,
            pending_actions: pending,
        }
    }

    /// Load scripts from the default config directory.
    pub fn load_user_scripts(&mut self) {
        let scripts_dir = scripts_dir();
        if scripts_dir.is_dir() {
            match self.engine.load_scripts_dir(&scripts_dir) {
                Ok(names) => {
                    if !names.is_empty() {
                        tracing::info!(count = names.len(), "loaded namimado scripts: {names:?}");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to load namimado scripts");
                }
            }
        }
    }

    /// Fire an event hook.
    pub fn fire_event(&self, event: &ScriptEvent) {
        let hook_name = match event {
            ScriptEvent::OnStart => "on_start",
            ScriptEvent::OnQuit => "on_quit",
            ScriptEvent::OnKey(_) => "on_key",
        };

        let script = match event {
            ScriptEvent::OnKey(key) => format!("if is_def_fn(\"{hook_name}\", 1) {{ {hook_name}(\"{key}\"); }}"),
            _ => format!("if is_def_fn(\"{hook_name}\", 0) {{ {hook_name}(); }}"),
        };

        if let Err(e) = self.engine.eval(&script) {
            tracing::debug!(hook = hook_name, error = %e, "script hook not defined or failed");
        }
    }

    /// Drain any pending actions triggered by scripts.
    pub fn drain_actions(&self) -> Vec<ScriptAction> {
        if let Ok(mut actions) = self.pending_actions.lock() {
            actions.drain(..).collect()
        } else {
            Vec::new()
        }
    }
}

impl Default for NamimadoScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Default scripts directory: `~/.config/namimado/scripts/`.
fn scripts_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("namimado")
        .join("scripts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_creation() {
        let _engine = NamimadoScriptEngine::new();
    }

    #[test]
    fn new_tab_action() {
        let engine = NamimadoScriptEngine::new();
        engine
            .engine
            .eval(r#"namimado_new_tab("https://example.com")"#)
            .unwrap();
        let actions = engine.drain_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ScriptAction::NewTab(url) if url == "https://example.com"));
    }

    #[test]
    fn close_tab_action() {
        let engine = NamimadoScriptEngine::new();
        engine.engine.eval("namimado_close_tab()").unwrap();
        let actions = engine.drain_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ScriptAction::CloseTab));
    }

    #[test]
    fn navigate_action() {
        let engine = NamimadoScriptEngine::new();
        engine
            .engine
            .eval(r#"namimado_navigate("https://rust-lang.org")"#)
            .unwrap();
        let actions = engine.drain_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ScriptAction::Navigate(url) if url == "https://rust-lang.org"));
    }

    #[test]
    fn get_url_returns_string() {
        let engine = NamimadoScriptEngine::new();
        let result = engine.engine.eval("namimado_get_url()").unwrap();
        assert!(result.is_string());
    }

    #[test]
    fn fire_event_does_not_panic() {
        let engine = NamimadoScriptEngine::new();
        engine.fire_event(&ScriptEvent::OnStart);
        engine.fire_event(&ScriptEvent::OnQuit);
        engine.fire_event(&ScriptEvent::OnKey("t".to_string()));
    }

    #[test]
    fn drain_actions_clears() {
        let engine = NamimadoScriptEngine::new();
        engine
            .engine
            .eval(r#"namimado_navigate("https://a.com")"#)
            .unwrap();
        assert_eq!(engine.drain_actions().len(), 1);
        assert!(engine.drain_actions().is_empty());
    }
}
