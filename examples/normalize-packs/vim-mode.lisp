;; Vim-mode pack — keyboard-driven navigation for namimado.
;;
;; Ships as a default. Drop into `~/.config/namimado/substrate.d/`
;; to enable; tweak any (defbind) to remap; author additional
;; (defcommand)s in Lisp to build your own verbs.
;;
;; Modes: normal (default), insert (form fields + URL bar),
;;        visual (text selection), command (`:`-prompt).
;;
;; Fully Lisp-programmable — add your own commands and bindings by
;; dropping another `.lisp` file into the same directory.

;; ── Navigation commands ──────────────────────────────────────────

(defcommand :name        "scroll:down"
            :description "Scroll viewport down by line."
            :action      "scroll:line-down")

(defcommand :name        "scroll:up"
            :description "Scroll viewport up by line."
            :action      "scroll:line-up")

(defcommand :name        "scroll:left"
            :description "Scroll viewport left."
            :action      "scroll:left")

(defcommand :name        "scroll:right"
            :description "Scroll viewport right."
            :action      "scroll:right")

(defcommand :name        "scroll:page-down"
            :description "Scroll down one viewport."
            :action      "scroll:page-down")

(defcommand :name        "scroll:page-up"
            :description "Scroll up one viewport."
            :action      "scroll:page-up")

(defcommand :name        "scroll:top"
            :description "Jump to top of document."
            :action      "scroll:top")

(defcommand :name        "scroll:bottom"
            :description "Jump to bottom of document."
            :action      "scroll:bottom")

;; ── Page lifecycle ───────────────────────────────────────────────

(defcommand :name        "reload"
            :description "Reload the current page."
            :action      "reload")

(defcommand :name        "back"
            :description "History back."
            :action      "history:back")

(defcommand :name        "forward"
            :description "History forward."
            :action      "history:forward")

;; ── Feature toggles ──────────────────────────────────────────────

(defcommand :name        "reader:toggle"
            :description "Toggle Readability-style simplified view."
            :action      "reader:toggle")

(defcommand :name        "blocker:toggle"
            :description "Toggle content blocker on the current host."
            :action      "blocker:toggle")

;; ── Mode switches ────────────────────────────────────────────────

(defcommand :name        "mode:normal"
            :description "Enter normal mode."
            :action      "mode:set:normal")

(defcommand :name        "mode:insert"
            :description "Enter insert mode (URL bar / form)."
            :action      "mode:set:insert")

(defcommand :name        "mode:visual"
            :description "Enter visual mode for text selection."
            :action      "mode:set:visual")

(defcommand :name        "mode:command"
            :description "Open the `:` command palette."
            :action      "mode:set:command")

;; ── Normal-mode bindings ─────────────────────────────────────────

(defbind :key "h"     :command "scroll:left"       :mode "normal")
(defbind :key "j"     :command "scroll:down"       :mode "normal")
(defbind :key "k"     :command "scroll:up"         :mode "normal")
(defbind :key "l"     :command "scroll:right"      :mode "normal")

(defbind :key "Ctrl+f" :command "scroll:page-down" :mode "normal")
(defbind :key "Ctrl+b" :command "scroll:page-up"   :mode "normal")
(defbind :key "Ctrl+d" :command "scroll:page-down" :mode "normal")
(defbind :key "Ctrl+u" :command "scroll:page-up"   :mode "normal")

(defbind :key "g g"   :command "scroll:top"        :mode "normal")
(defbind :key "Shift+g" :command "scroll:bottom"   :mode "normal")

(defbind :key "r"     :command "reload"            :mode "normal")
(defbind :key "Shift+r" :command "reload"          :mode "normal")

(defbind :key "Shift+h" :command "back"            :mode "normal")
(defbind :key "Shift+l" :command "forward"         :mode "normal")

(defbind :key "Shift+m" :command "reader:toggle"   :mode "normal"
         :description "Mark page as readable — toggle reader view.")

(defbind :key "Shift+b" :command "blocker:toggle"  :mode "normal")

(defbind :key "i"     :command "mode:insert"       :mode "normal")
(defbind :key "v"     :command "mode:visual"       :mode "normal")
(defbind :key ":"     :command "mode:command"      :mode "normal")

;; ── Global bindings (work in any mode) ───────────────────────────

(defbind :key "Escape" :command "mode:normal"
         :description "Cancel any pending sequence and return to normal mode.")

;; ── Insert mode overrides ────────────────────────────────────────

;; Inside a text input, h/j/k/l are just characters; only Escape
;; matters here. Insert-mode bindings intentionally stay sparse so
;; the substrate forwards every keypress to the underlying input.
(defbind :key "Ctrl+["  :command "mode:normal" :mode "insert"
         :description "Vim tradition: Ctrl-[ exits insert mode.")

;; ── Visual mode ──────────────────────────────────────────────────

;; In visual mode, h/j/k/l extend the selection instead of scrolling.
(defcommand :name "select:extend-left"  :action "select:extend-left")
(defcommand :name "select:extend-right" :action "select:extend-right")
(defcommand :name "select:extend-up"    :action "select:extend-up")
(defcommand :name "select:extend-down"  :action "select:extend-down")
(defcommand :name "select:yank"         :action "select:yank")

(defbind :key "h" :command "select:extend-left"  :mode "visual")
(defbind :key "j" :command "select:extend-down"  :mode "visual")
(defbind :key "k" :command "select:extend-up"    :mode "visual")
(defbind :key "l" :command "select:extend-right" :mode "visual")
(defbind :key "y" :command "select:yank"         :mode "visual"
         :description "Yank selection into clipboard.")
