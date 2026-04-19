;; shadcn/radix component idioms → canonical n-* vocabulary.
;;
;; shadcn tags most components with `data-slot="NAME"` attributes,
;; which makes normalization easy. Framework-gated on shadcn/radix so
;; these only fire on pages where shadcn was detected.
;;
;; Drop into ~/.config/namimado/substrate.d/shadcn.lisp.

(defnormalize :name "shadcn-card"
              :framework "shadcn"
              :selector "[data-slot=card]"
              :rename-to "n-card")

(defnormalize :name "shadcn-card-title"
              :framework "shadcn"
              :selector "[data-slot=card-title]"
              :rename-to "n-card-title")

(defnormalize :name "shadcn-card-description"
              :framework "shadcn"
              :selector "[data-slot=card-description]"
              :rename-to "n-card-description")

(defnormalize :name "shadcn-card-content"
              :framework "shadcn"
              :selector "[data-slot=card-content]"
              :rename-to "n-card-content")

(defnormalize :name "shadcn-tab"
              :framework "shadcn"
              :selector "[data-slot=tab]"
              :rename-to "n-tab")

(defnormalize :name "shadcn-tabs-list"
              :framework "shadcn"
              :selector "[data-slot=tabs-list]"
              :rename-to "n-tabs-list")

(defnormalize :name "shadcn-button"
              :framework "shadcn"
              :selector "[data-slot=button]"
              :rename-to "n-button")

(defnormalize :name "shadcn-input"
              :framework "shadcn"
              :selector "[data-slot=input]"
              :rename-to "n-input")

(defnormalize :name "shadcn-avatar"
              :framework "shadcn"
              :selector "[data-slot=avatar]"
              :rename-to "n-avatar")

(defnormalize :name "shadcn-dialog"
              :framework "shadcn"
              :selector "[data-slot=dialog-content]"
              :rename-to "n-dialog")
