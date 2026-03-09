use winit::event::{ElementState, Modifiers};
use winit::keyboard::{Key, NamedKey};

use awase::{Hotkey, Key as AwaseKey, Modifiers as AwaseMods};

/// A keybinding definition: an awase `Hotkey` paired with an action name.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyBinding {
    /// The hotkey that triggers this binding (awase type).
    pub hotkey: Hotkey,
    /// The action name to perform.
    pub action: String,
}

/// Default keybindings using awase `Hotkey` types.
#[must_use]
pub fn default_bindings() -> Vec<KeyBinding> {
    vec![
        // Tab management
        KeyBinding { hotkey: Hotkey::new(AwaseMods::CMD, AwaseKey::T), action: "new_tab".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::CMD, AwaseKey::W), action: "close_tab".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::CMD, AwaseKey::L), action: "focus_address_bar".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::CMD, AwaseKey::R), action: "reload".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::CMD, AwaseKey::D), action: "bookmark_page".into() },
        KeyBinding { hotkey: Hotkey::new(AwaseMods::CMD, AwaseKey::F), action: "find_on_page".into() },
        // Zoom
        KeyBinding { hotkey: Hotkey::new(AwaseMods::CMD, AwaseKey::Num0), action: "zoom_reset".into() },
    ]
}

/// A browser action triggered by a keyboard shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    // Tab management
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    SwitchToTab(usize),
    RestoreClosedTab,
    DuplicateTab,
    PinTab,

    // Navigation
    FocusAddressBar,
    GoBack,
    GoForward,
    Reload,
    ReloadHard,
    Stop,

    // Bookmarks
    BookmarkPage,
    ToggleBookmarkBar,
    ShowBookmarks,

    // Find
    FindOnPage,

    // Sidebar
    ToggleSidebar,
    ShowHistory,
    ShowDownloads,

    // Vim mode
    ToggleVimMode,

    // Window
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleFullscreen,

    // Developer tools
    ToggleDevTools,

    // Address bar navigation
    AddressBarUp,
    AddressBarDown,
    AddressBarSubmit,
    AddressBarDismiss,
}

/// Vim-style navigation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    /// Normal mode — keyboard commands navigate the page.
    Normal,
    /// Insert mode — keyboard input goes to form fields / address bar.
    Insert,
    /// Command mode — `:` prefix commands.
    Command,
    /// Follow mode — link hint labels visible, waiting for selection.
    Follow,
}

/// Manages keyboard input and maps it to browser actions.
///
/// Supports both standard browser shortcuts (Cmd+T, Cmd+W, etc.)
/// and optional vim-style modal navigation.
#[derive(Debug)]
pub struct KeybindingManager {
    /// Whether vim-style mode is enabled.
    pub vim_enabled: bool,
    /// Current vim mode.
    pub vim_mode: VimMode,
    /// Whether the address bar is focused (affects keybinding dispatch).
    pub address_bar_focused: bool,
}

impl KeybindingManager {
    /// Create a new keybinding manager.
    pub fn new() -> Self {
        Self {
            vim_enabled: false,
            vim_mode: VimMode::Normal,
            address_bar_focused: false,
        }
    }

    /// Process a key event and return the corresponding browser action, if any.
    ///
    /// This checks standard browser shortcuts first, then vim bindings if
    /// enabled. Returns `None` if no action matches.
    pub fn process_key(
        &mut self,
        key: &Key,
        state: ElementState,
        modifiers: &Modifiers,
    ) -> Option<BrowserAction> {
        // Only handle key presses (not releases)
        if state != ElementState::Pressed {
            return None;
        }

        let ctrl_or_cmd = modifiers.state().super_key() || modifiers.state().control_key();
        let shift = modifiers.state().shift_key();

        // Address bar focused — handle special keys
        if self.address_bar_focused {
            return self.process_address_bar_key(key, ctrl_or_cmd, shift);
        }

        // Standard browser shortcuts (always active)
        if let Some(action) = self.process_standard_shortcut(key, ctrl_or_cmd, shift) {
            return Some(action);
        }

        // Vim bindings (only when enabled and not in insert mode)
        if self.vim_enabled && self.vim_mode != VimMode::Insert {
            return self.process_vim_key(key, shift);
        }

        None
    }

    /// Process keys when the address bar is focused.
    fn process_address_bar_key(
        &self,
        key: &Key,
        ctrl_or_cmd: bool,
        _shift: bool,
    ) -> Option<BrowserAction> {
        match key {
            Key::Named(NamedKey::Enter) => Some(BrowserAction::AddressBarSubmit),
            Key::Named(NamedKey::Escape) => Some(BrowserAction::AddressBarDismiss),
            Key::Named(NamedKey::ArrowUp) => Some(BrowserAction::AddressBarUp),
            Key::Named(NamedKey::ArrowDown) => Some(BrowserAction::AddressBarDown),
            // Allow Cmd+L to refocus (no-op, but handled)
            Key::Character(c) if ctrl_or_cmd && c.as_str() == "l" => {
                Some(BrowserAction::FocusAddressBar)
            }
            _ => None,
        }
    }

    /// Process standard browser shortcuts.
    fn process_standard_shortcut(
        &self,
        key: &Key,
        ctrl_or_cmd: bool,
        shift: bool,
    ) -> Option<BrowserAction> {
        if !ctrl_or_cmd {
            return None;
        }

        match key {
            Key::Character(c) => match c.as_str() {
                "t" if !shift => Some(BrowserAction::NewTab),
                "t" | "T" if shift => Some(BrowserAction::RestoreClosedTab),
                "w" => Some(BrowserAction::CloseTab),
                "l" => Some(BrowserAction::FocusAddressBar),
                "r" if !shift => Some(BrowserAction::Reload),
                "r" | "R" if shift => Some(BrowserAction::ReloadHard),
                "d" => Some(BrowserAction::BookmarkPage),
                "f" => Some(BrowserAction::FindOnPage),
                "b" | "B" if shift => Some(BrowserAction::ToggleBookmarkBar),
                "=" | "+" => Some(BrowserAction::ZoomIn),
                "-" => Some(BrowserAction::ZoomOut),
                "0" => Some(BrowserAction::ZoomReset),

                // Cmd+[ / Cmd+] for back/forward
                "[" => Some(BrowserAction::GoBack),
                "]" => Some(BrowserAction::GoForward),

                // Tab switching: Cmd+1..9
                "1" => Some(BrowserAction::SwitchToTab(0)),
                "2" => Some(BrowserAction::SwitchToTab(1)),
                "3" => Some(BrowserAction::SwitchToTab(2)),
                "4" => Some(BrowserAction::SwitchToTab(3)),
                "5" => Some(BrowserAction::SwitchToTab(4)),
                "6" => Some(BrowserAction::SwitchToTab(5)),
                "7" => Some(BrowserAction::SwitchToTab(6)),
                "8" => Some(BrowserAction::SwitchToTab(7)),
                "9" => Some(BrowserAction::SwitchToTab(8)),

                _ => None,
            },
            Key::Named(NamedKey::Tab) if !shift => Some(BrowserAction::NextTab),
            Key::Named(NamedKey::Tab) if shift => Some(BrowserAction::PrevTab),
            _ => None,
        }
    }

    /// Process vim-style keybindings.
    fn process_vim_key(&mut self, key: &Key, shift: bool) -> Option<BrowserAction> {
        match self.vim_mode {
            VimMode::Normal => self.process_vim_normal(key, shift),
            VimMode::Command => {
                // In command mode, Escape returns to normal
                if matches!(key, Key::Named(NamedKey::Escape)) {
                    self.vim_mode = VimMode::Normal;
                }
                None
            }
            VimMode::Follow => {
                // In follow mode, Escape returns to normal
                if matches!(key, Key::Named(NamedKey::Escape)) {
                    self.vim_mode = VimMode::Normal;
                }
                None
            }
            VimMode::Insert => None, // Should not reach here (guarded above)
        }
    }

    /// Process keys in vim normal mode.
    fn process_vim_normal(&mut self, key: &Key, shift: bool) -> Option<BrowserAction> {
        match key {
            Key::Character(c) => match c.as_str() {
                "i" => {
                    self.vim_mode = VimMode::Insert;
                    None
                }
                ":" => {
                    self.vim_mode = VimMode::Command;
                    None
                }
                "f" if !shift => {
                    self.vim_mode = VimMode::Follow;
                    None
                }
                "o" => Some(BrowserAction::FocusAddressBar),
                "H" => Some(BrowserAction::GoBack),
                "L" => Some(BrowserAction::GoForward),
                "r" => Some(BrowserAction::Reload),
                "d" => Some(BrowserAction::CloseTab),
                "u" => Some(BrowserAction::RestoreClosedTab),
                "J" => Some(BrowserAction::NextTab),
                "K" => Some(BrowserAction::PrevTab),
                "b" if shift => Some(BrowserAction::ShowBookmarks),
                "h" if shift => Some(BrowserAction::ShowHistory),
                _ => None,
            },
            Key::Named(NamedKey::Escape) => {
                self.vim_mode = VimMode::Normal;
                None
            }
            _ => None,
        }
    }

    /// Toggle vim mode on/off.
    pub fn toggle_vim(&mut self) {
        self.vim_enabled = !self.vim_enabled;
        if !self.vim_enabled {
            self.vim_mode = VimMode::Normal;
        }
    }

    /// Enter insert mode (e.g., when clicking in a text field).
    pub fn enter_insert_mode(&mut self) {
        if self.vim_enabled {
            self.vim_mode = VimMode::Insert;
        }
    }

    /// Return to normal mode (e.g., on Escape).
    pub fn enter_normal_mode(&mut self) {
        if self.vim_enabled {
            self.vim_mode = VimMode::Normal;
        }
    }
}

impl Default for KeybindingManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::Key;

    #[test]
    fn default_bindings_are_valid() {
        let bindings = default_bindings();
        assert!(!bindings.is_empty());
        let has_new_tab = bindings.iter().any(|b| b.action == "new_tab");
        assert!(has_new_tab, "should have a new_tab binding");
    }

    #[test]
    fn bindings_are_serializable() {
        let bindings = default_bindings();
        let json = serde_json::to_string(&bindings).unwrap();
        let deserialized: Vec<KeyBinding> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), bindings.len());
    }

    #[test]
    fn process_standard_shortcut_new_tab() {
        let mgr = KeybindingManager::new();
        let action = mgr.process_standard_shortcut(
            &Key::Character("t".into()),
            true,
            false,
        );
        assert_eq!(action, Some(BrowserAction::NewTab));
    }

    #[test]
    fn process_standard_shortcut_close_tab() {
        let mgr = KeybindingManager::new();
        let action = mgr.process_standard_shortcut(
            &Key::Character("w".into()),
            true,
            false,
        );
        assert_eq!(action, Some(BrowserAction::CloseTab));
    }

    #[test]
    fn process_standard_shortcut_focus_address() {
        let mgr = KeybindingManager::new();
        let action = mgr.process_standard_shortcut(
            &Key::Character("l".into()),
            true,
            false,
        );
        assert_eq!(action, Some(BrowserAction::FocusAddressBar));
    }

    #[test]
    fn process_standard_shortcut_tab_switching() {
        let mgr = KeybindingManager::new();
        for (key, expected_idx) in [("1", 0), ("5", 4), ("9", 8)] {
            let action = mgr.process_standard_shortcut(
                &Key::Character(key.into()),
                true,
                false,
            );
            assert_eq!(action, Some(BrowserAction::SwitchToTab(expected_idx)));
        }
    }

    #[test]
    fn process_standard_shortcut_bookmark() {
        let mgr = KeybindingManager::new();
        let action = mgr.process_standard_shortcut(
            &Key::Character("d".into()),
            true,
            false,
        );
        assert_eq!(action, Some(BrowserAction::BookmarkPage));
    }

    #[test]
    fn process_standard_shortcut_no_cmd_returns_none() {
        let mgr = KeybindingManager::new();
        let action = mgr.process_standard_shortcut(
            &Key::Character("t".into()),
            false,
            false,
        );
        assert_eq!(action, None);
    }

    #[test]
    fn address_bar_keys() {
        let mgr = KeybindingManager::new();
        let enter = mgr.process_address_bar_key(
            &Key::Named(NamedKey::Enter),
            false,
            false,
        );
        assert_eq!(enter, Some(BrowserAction::AddressBarSubmit));

        let esc = mgr.process_address_bar_key(
            &Key::Named(NamedKey::Escape),
            false,
            false,
        );
        assert_eq!(esc, Some(BrowserAction::AddressBarDismiss));

        let up = mgr.process_address_bar_key(
            &Key::Named(NamedKey::ArrowUp),
            false,
            false,
        );
        assert_eq!(up, Some(BrowserAction::AddressBarUp));
    }

    #[test]
    fn vim_normal_mode_keys() {
        let mut mgr = KeybindingManager::new();
        mgr.vim_enabled = true;

        let action = mgr.process_vim_normal(
            &Key::Character("o".into()),
            false,
        );
        assert_eq!(action, Some(BrowserAction::FocusAddressBar));

        let action = mgr.process_vim_normal(
            &Key::Character("H".into()),
            true,
        );
        assert_eq!(action, Some(BrowserAction::GoBack));
    }

    #[test]
    fn vim_mode_transitions() {
        let mut mgr = KeybindingManager::new();
        mgr.vim_enabled = true;
        assert_eq!(mgr.vim_mode, VimMode::Normal);

        // i -> insert
        mgr.process_vim_normal(&Key::Character("i".into()), false);
        assert_eq!(mgr.vim_mode, VimMode::Insert);

        // Return to normal
        mgr.enter_normal_mode();
        assert_eq!(mgr.vim_mode, VimMode::Normal);

        // : -> command
        mgr.process_vim_normal(&Key::Character(":".into()), false);
        assert_eq!(mgr.vim_mode, VimMode::Command);

        // Escape -> normal
        mgr.process_vim_key(&Key::Named(NamedKey::Escape), false);
        assert_eq!(mgr.vim_mode, VimMode::Normal);
    }

    #[test]
    fn toggle_vim() {
        let mut mgr = KeybindingManager::new();
        assert!(!mgr.vim_enabled);
        mgr.toggle_vim();
        assert!(mgr.vim_enabled);
        mgr.toggle_vim();
        assert!(!mgr.vim_enabled);
    }
}
