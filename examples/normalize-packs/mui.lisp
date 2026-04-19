;; Material-UI component idioms → canonical n-* vocabulary.
;;
;; MUI emits class names like `MuiCard-root`, `MuiButton-root` with a
;; small number of stable root classes per component. Gated on `mui`
;; detection so these only fire on MUI-backed pages.
;;
;; Drop into ~/.config/namimado/substrate.d/mui.lisp.

(defnormalize :name "mui-card"
              :framework "mui"
              :selector ".MuiCard-root"
              :rename-to "n-card")

(defnormalize :name "mui-card-header"
              :framework "mui"
              :selector ".MuiCardHeader-root"
              :rename-to "n-card-header")

(defnormalize :name "mui-card-content"
              :framework "mui"
              :selector ".MuiCardContent-root"
              :rename-to "n-card-content")

(defnormalize :name "mui-card-actions"
              :framework "mui"
              :selector ".MuiCardActions-root"
              :rename-to "n-card-actions")

(defnormalize :name "mui-button"
              :framework "mui"
              :selector ".MuiButton-root"
              :rename-to "n-button")

(defnormalize :name "mui-icon-button"
              :framework "mui"
              :selector ".MuiIconButton-root"
              :rename-to "n-icon-button")

(defnormalize :name "mui-app-bar"
              :framework "mui"
              :selector ".MuiAppBar-root"
              :rename-to "n-app-bar")

(defnormalize :name "mui-toolbar"
              :framework "mui"
              :selector ".MuiToolbar-root"
              :rename-to "n-toolbar")

(defnormalize :name "mui-drawer"
              :framework "mui"
              :selector ".MuiDrawer-root"
              :rename-to "n-drawer")

(defnormalize :name "mui-dialog"
              :framework "mui"
              :selector ".MuiDialog-root"
              :rename-to "n-dialog")

(defnormalize :name "mui-tabs"
              :framework "mui"
              :selector ".MuiTabs-root"
              :rename-to "n-tabs-list")

(defnormalize :name "mui-tab"
              :framework "mui"
              :selector ".MuiTab-root"
              :rename-to "n-tab")

(defnormalize :name "mui-input"
              :framework "mui"
              :selector ".MuiInputBase-root"
              :rename-to "n-input")

(defnormalize :name "mui-avatar"
              :framework "mui"
              :selector ".MuiAvatar-root"
              :rename-to "n-avatar")

(defnormalize :name "mui-list"
              :framework "mui"
              :selector ".MuiList-root"
              :rename-to "n-list")
