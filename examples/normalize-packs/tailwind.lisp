;; Tailwind utility-class component patterns → canonical n-* vocab.
;;
;; Tailwind doesn't prescribe component shapes — most Tailwind sites
;; use semantic HTML + utility classes, OR combine Tailwind with
;; headless-UI libs (which generally ship their own data-* idioms).
;;
;; These rules cover common bare-Tailwind component conventions:
;;   - buttons styled with px-*, py-*, rounded-*, bg-*
;;   - cards styled with rounded-lg, shadow, p-*
;;   - nav containers with flex, items-center, justify-between
;;
;; Gated on "tailwind" detection so these only fire on Tailwind
;; pages (detected via >40 utility classes). Conservative —
;; Tailwind's lack of uniform component tagging means this pack
;; makes fewer promises than MUI/Bootstrap.
;;
;; Drop into ~/.config/namimado/substrate.d/tailwind.lisp.

;; Buttons — the most common Tailwind component pattern.
(defnormalize :name "tw-button-primary"
              :framework "tailwind"
              :selector "button.rounded"
              :rename-to "n-button")

(defnormalize :name "tw-button-link"
              :framework "tailwind"
              :selector "a.rounded"
              :rename-to "n-button")

;; Cards — varies wildly in practice; common baseline is rounded +
;; shadow wrapping a titled body.
(defnormalize :name "tw-card-rounded-shadow"
              :framework "tailwind"
              :selector "div.rounded-lg.shadow"
              :rename-to "n-card")

(defnormalize :name "tw-card-rounded"
              :framework "tailwind"
              :selector "div.rounded.shadow-md"
              :rename-to "n-card")

;; Nav containers — usually flex + items-center.
(defnormalize :name "tw-nav-flex"
              :framework "tailwind"
              :selector "nav.flex"
              :rename-to "n-nav")

;; Lists.
(defnormalize :name "tw-list-divide"
              :framework "tailwind"
              :selector "ul.divide-y"
              :rename-to "n-list")

;; Dialogs (usually headless-ui signatures).
(defnormalize :name "tw-dialog-backdrop"
              :framework "tailwind"
              :selector "[role=dialog]"
              :rename-to "n-dialog")

;; Alerts.
(defnormalize :name "tw-alert-role"
              :framework "tailwind"
              :selector "[role=alert]"
              :rename-to "n-alert")

;; Forms.
(defnormalize :name "tw-input-bordered"
              :framework "tailwind"
              :selector "input.rounded"
              :rename-to "n-input")
