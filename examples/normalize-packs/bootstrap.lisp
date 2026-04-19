;; Bootstrap (v4/v5) component classes → canonical n-* vocabulary.
;;
;; Bootstrap is selector-heavy (one class per semantic role). Rules
;; gated on "bootstrap" detection.
;;
;; Drop into ~/.config/namimado/substrate.d/bootstrap.lisp.

(defnormalize :name "bs-card"
              :framework "bootstrap"
              :selector ".card"
              :rename-to "n-card")

(defnormalize :name "bs-card-header"
              :framework "bootstrap"
              :selector ".card-header"
              :rename-to "n-card-header")

(defnormalize :name "bs-card-body"
              :framework "bootstrap"
              :selector ".card-body"
              :rename-to "n-card-content")

(defnormalize :name "bs-card-title"
              :framework "bootstrap"
              :selector ".card-title"
              :rename-to "n-card-title")

(defnormalize :name "bs-card-footer"
              :framework "bootstrap"
              :selector ".card-footer"
              :rename-to "n-card-footer")

(defnormalize :name "bs-navbar"
              :framework "bootstrap"
              :selector ".navbar"
              :rename-to "n-nav")

(defnormalize :name "bs-btn"
              :framework "bootstrap"
              :selector ".btn"
              :rename-to "n-button")

(defnormalize :name "bs-modal"
              :framework "bootstrap"
              :selector ".modal"
              :rename-to "n-dialog")

(defnormalize :name "bs-alert"
              :framework "bootstrap"
              :selector ".alert"
              :rename-to "n-alert")

(defnormalize :name "bs-list-group"
              :framework "bootstrap"
              :selector ".list-group"
              :rename-to "n-list")

(defnormalize :name "bs-list-group-item"
              :framework "bootstrap"
              :selector ".list-group-item"
              :rename-to "n-list-item")

(defnormalize :name "bs-nav-tabs"
              :framework "bootstrap"
              :selector ".nav-tabs"
              :rename-to "n-tabs-list")

(defnormalize :name "bs-nav-link"
              :framework "bootstrap"
              :selector ".nav-link"
              :rename-to "n-nav-link")

(defnormalize :name "bs-badge"
              :framework "bootstrap"
              :selector ".badge"
              :rename-to "n-badge")

(defnormalize :name "bs-breadcrumb"
              :framework "bootstrap"
              :selector ".breadcrumb"
              :rename-to "n-breadcrumb")

(defnormalize :name "bs-form-control"
              :framework "bootstrap"
              :selector ".form-control"
              :rename-to "n-input")
