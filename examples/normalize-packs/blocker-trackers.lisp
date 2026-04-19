;; Content-blocker starter pack — common third-party trackers +
;; ad-network endpoints that ship in ~every mainstream blocklist.
;; Rules fire both at the outbound-fetch layer (namimado refuses
;; the HTTP call with a BlockedByRule error) AND at the DOM layer
;; (matching elements are stripped post-normalize).
;;
;; Drop into ~/.config/namimado/substrate.d/blocker-trackers.lisp
;; for privacy-first defaults. Pair with a fuller EasyList import
;; when the V2 grammar ships.

(defblocker :name "analytics-trackers"
            :description "Major web-analytics endpoints"
            :domains ("google-analytics.com"
                      "googletagmanager.com"
                      "googletagservices.com"
                      "scorecardresearch.com"
                      "hotjar.com"
                      "mixpanel.com"
                      "fullstory.com"
                      "segment.io"
                      "amplitude.com"))

(defblocker :name "ad-networks"
            :description "Ad-network endpoints"
            :domains ("doubleclick.net"
                      "googlesyndication.com"
                      "googleadservices.com"
                      "adnxs.com"
                      "criteo.com"
                      "taboola.com"
                      "outbrain.com"
                      "quantserve.com"))

(defblocker :name "social-pixels"
            :description "Social-network tracking pixels"
            :domains ("facebook.com/tr"
                      "facebook.net/en_US/fbevents.js"
                      "connect.facebook.net/signals/config"
                      "analytics.twitter.com"
                      "t.co/i/adsct"
                      "linkedin.com/px"
                      "pinterest.com/ct"))

(defblocker :name "cosmetic-ad-containers"
            :description "Common ad-container selectors"
            :selectors (".ad-slot"
                        ".ad-sidebar"
                        ".ad-container"
                        ".adsbygoogle"
                        "[data-ad-placement]"
                        "[data-ad-slot]"
                        "[aria-label=advertisement]"
                        "ins.adsbygoogle"))
