;; HTML5 semantic tags → canonical n-* vocabulary.
;;
;; Drop into ~/.config/namimado/substrate.d/html5.lisp to have every
;; <article>, <nav>, <main>, <aside>, <section>, <header>, <footer>
;; element renamed to its canonical n-* form on every page load.
;;
;; Transforms, scrapes, agents, and MCP tools can then target one
;; canonical schema instead of guessing at framework-specific shapes.

(defnormalize :name "html5-article"
              :selector "article"
              :rename-to "n-article")

(defnormalize :name "html5-nav"
              :selector "nav"
              :rename-to "n-nav")

(defnormalize :name "html5-main"
              :selector "main"
              :rename-to "n-main")

(defnormalize :name "html5-aside"
              :selector "aside"
              :rename-to "n-aside")

(defnormalize :name "html5-section"
              :selector "section"
              :rename-to "n-section")

(defnormalize :name "html5-header"
              :selector "header"
              :rename-to "n-header")

(defnormalize :name "html5-footer"
              :selector "footer"
              :rename-to "n-footer")

(defnormalize :name "html5-figure"
              :selector "figure"
              :rename-to "n-figure")
