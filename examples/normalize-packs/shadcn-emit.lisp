;; Outbound emit: canonical n-* vocabulary → shadcn-shaped DOM.
;;
;; Pair this with html5.lisp (inbound fold) to convert any
;; semantic-HTML5 page into shadcn form:
;;
;;   <article>text</article>
;;       ↓ html5.lisp          (inbound: article → n-article)
;;   <n-article>text</n-article>
;;       ↓ shadcn-emit.lisp    (outbound: n-article → div[data-slot=article])
;;   <div data-slot="article">text</div>
;;
;; Or pair with the MUI / Bootstrap packs (once shipped) to convert
;; between frameworks via the canonical intermediate form.
;;
;; Drop into ~/.config/namimado/substrate.d/shadcn-emit.lisp.

(defnormalize :name "emit-shadcn-article"
              :selector "n-article"
              :rename-to "div"
              :set-attrs (("data-slot" "article")))

(defnormalize :name "emit-shadcn-nav"
              :selector "n-nav"
              :rename-to "nav"
              :set-attrs (("data-slot" "navigation")))

(defnormalize :name "emit-shadcn-card"
              :selector "n-card"
              :rename-to "div"
              :set-attrs (("data-slot" "card")))

(defnormalize :name "emit-shadcn-card-title"
              :selector "n-card-title"
              :rename-to "div"
              :set-attrs (("data-slot" "card-title")))

(defnormalize :name "emit-shadcn-card-content"
              :selector "n-card-content"
              :rename-to "div"
              :set-attrs (("data-slot" "card-content")))

(defnormalize :name "emit-shadcn-button"
              :selector "n-button"
              :rename-to "button"
              :set-attrs (("data-slot" "button")))

(defnormalize :name "emit-shadcn-input"
              :selector "n-input"
              :rename-to "input"
              :set-attrs (("data-slot" "input")))

(defnormalize :name "emit-shadcn-tab"
              :selector "n-tab"
              :rename-to "button"
              :set-attrs (("data-slot" "tab")))
