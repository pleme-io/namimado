use serde::{Deserialize, Serialize};

/// Messages sent between the Rust backend and the webview's JavaScript context.
///
/// These are serialized as JSON and passed through wry's IPC mechanism.
/// The webview sends messages via `window.ipc.postMessage(JSON.stringify(msg))`,
/// and Rust receives them in the `ipc_handler` callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum IpcMessage {
    /// The webview wants to navigate to a new URL.
    Navigate {
        url: String,
    },

    /// The page title changed.
    TitleChanged {
        title: String,
    },

    /// A page load started.
    LoadStart,

    /// A page load completed.
    LoadEnd,

    /// The page favicon changed.
    FaviconChanged {
        url: String,
    },
}

/// Bidirectional IPC bridge between Rust and the webview.
///
/// In the current implementation this is a lightweight coordinator.
/// Future versions will use channels for async message dispatch when
/// the GPU chrome layer needs to react to webview events.
#[derive(Debug)]
pub struct IpcBridge {
    /// Messages received from the webview, buffered for the next frame.
    incoming: Vec<IpcMessage>,
}

impl IpcBridge {
    /// Create a new IPC bridge.
    pub fn new() -> Self {
        Self {
            incoming: Vec::new(),
        }
    }

    /// Queue an incoming message from the webview.
    pub fn push_incoming(&mut self, message: IpcMessage) {
        self.incoming.push(message);
    }

    /// Drain all buffered incoming messages.
    pub fn drain_incoming(&mut self) -> Vec<IpcMessage> {
        std::mem::take(&mut self.incoming)
    }

    /// Generate the JavaScript initialization script that the webview
    /// should evaluate to set up the IPC bridge on the JS side.
    #[must_use]
    pub fn js_init_script() -> &'static str {
        r#"
        (function() {
            'use strict';

            // Title change observer
            const titleObserver = new MutationObserver(function() {
                window.ipc.postMessage(JSON.stringify({
                    type: "TitleChanged",
                    payload: { title: document.title }
                }));
            });

            // Observe title element changes
            const titleEl = document.querySelector('title');
            if (titleEl) {
                titleObserver.observe(titleEl, { childList: true, characterData: true, subtree: true });
            }

            // Report initial title
            if (document.title) {
                window.ipc.postMessage(JSON.stringify({
                    type: "TitleChanged",
                    payload: { title: document.title }
                }));
            }

            // Expose namimado IPC namespace
            window.__namimado = {
                navigate: function(url) {
                    window.ipc.postMessage(JSON.stringify({
                        type: "Navigate",
                        payload: { url: url }
                    }));
                }
            };
        })();
        "#
    }
}

impl Default for IpcBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_serialize() {
        let msg = IpcMessage::Navigate {
            url: "https://example.com".to_owned(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: IpcMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            IpcMessage::Navigate { url } => assert_eq!(url, "https://example.com"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn roundtrip_title_changed() {
        let msg = IpcMessage::TitleChanged {
            title: "Hello World".to_owned(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: IpcMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            IpcMessage::TitleChanged { title } => assert_eq!(title, "Hello World"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn drain_clears_queue() {
        let mut bridge = IpcBridge::new();
        bridge.push_incoming(IpcMessage::LoadStart);
        bridge.push_incoming(IpcMessage::LoadEnd);
        let drained = bridge.drain_incoming();
        assert_eq!(drained.len(), 2);
        assert!(bridge.drain_incoming().is_empty());
    }

    #[test]
    fn js_init_script_is_nonempty() {
        let script = IpcBridge::js_init_script();
        assert!(!script.is_empty());
        assert!(script.contains("__namimado"));
    }
}
