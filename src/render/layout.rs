/// Layout rectangles for the browser chrome areas.
///
/// All coordinates are in logical pixels. The layout is computed from
/// the window size and sidebar visibility, then consumed by the renderer
/// to position the GPU chrome widgets and the content area.

/// A rectangle in logical pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Create a new rectangle.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The right edge (x + width).
    #[must_use]
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// The bottom edge (y + height).
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Whether a point is inside this rectangle.
    #[must_use]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }
}

/// Heights for chrome areas (in logical pixels).
pub const TAB_BAR_HEIGHT: f32 = 36.0;
pub const TOOLBAR_HEIGHT: f32 = 40.0;
pub const BOOKMARK_BAR_HEIGHT: f32 = 28.0;
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

/// Computed layout of all browser chrome regions.
#[derive(Debug, Clone)]
pub struct ChromeLayout {
    /// The tab bar area at the top.
    pub tab_bar: Rect,
    /// The toolbar (address bar, nav buttons) below the tab bar.
    pub toolbar: Rect,
    /// The bookmark bar below the toolbar (may be zero-height if hidden).
    pub bookmark_bar: Rect,
    /// The sidebar panel (may be zero-width if hidden).
    pub sidebar: Rect,
    /// The web content area (the remaining space).
    pub content: Rect,
    /// The status bar at the bottom.
    pub status_bar: Rect,
}

impl ChromeLayout {
    /// Compute the chrome layout from window dimensions and sidebar state.
    #[must_use]
    pub fn compute(
        window_width: f32,
        window_height: f32,
        sidebar_visible: bool,
        sidebar_width: f32,
        sidebar_left: bool,
        bookmark_bar_visible: bool,
    ) -> Self {
        let tab_bar = Rect::new(0.0, 0.0, window_width, TAB_BAR_HEIGHT);

        let toolbar = Rect::new(
            0.0,
            tab_bar.bottom(),
            window_width,
            TOOLBAR_HEIGHT,
        );

        let bm_height = if bookmark_bar_visible {
            BOOKMARK_BAR_HEIGHT
        } else {
            0.0
        };
        let bookmark_bar = Rect::new(
            0.0,
            toolbar.bottom(),
            window_width,
            bm_height,
        );

        let chrome_top = bookmark_bar.bottom();
        let status_bar = Rect::new(
            0.0,
            window_height - STATUS_BAR_HEIGHT,
            window_width,
            STATUS_BAR_HEIGHT,
        );

        let content_height = status_bar.y - chrome_top;
        let sb_width = if sidebar_visible { sidebar_width } else { 0.0 };

        let (sidebar, content) = if sidebar_visible {
            if sidebar_left {
                let sidebar = Rect::new(0.0, chrome_top, sb_width, content_height);
                let content = Rect::new(
                    sb_width,
                    chrome_top,
                    window_width - sb_width,
                    content_height,
                );
                (sidebar, content)
            } else {
                let content = Rect::new(
                    0.0,
                    chrome_top,
                    window_width - sb_width,
                    content_height,
                );
                let sidebar = Rect::new(
                    window_width - sb_width,
                    chrome_top,
                    sb_width,
                    content_height,
                );
                (sidebar, content)
            }
        } else {
            let sidebar = Rect::new(0.0, chrome_top, 0.0, content_height);
            let content = Rect::new(0.0, chrome_top, window_width, content_height);
            (sidebar, content)
        };

        Self {
            tab_bar,
            toolbar,
            bookmark_bar,
            sidebar,
            content,
            status_bar,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_layout_no_sidebar() {
        let layout = ChromeLayout::compute(1280.0, 800.0, false, 300.0, true, true);

        assert_eq!(layout.tab_bar.width, 1280.0);
        assert_eq!(layout.tab_bar.height, TAB_BAR_HEIGHT);
        assert_eq!(layout.toolbar.y, TAB_BAR_HEIGHT);
        assert_eq!(layout.content.width, 1280.0);
        assert!(layout.content.height > 0.0);
        assert_eq!(layout.status_bar.bottom(), 800.0);
        assert_eq!(layout.sidebar.width, 0.0);
    }

    #[test]
    fn layout_with_left_sidebar() {
        let layout = ChromeLayout::compute(1280.0, 800.0, true, 300.0, true, true);

        assert_eq!(layout.sidebar.width, 300.0);
        assert_eq!(layout.sidebar.x, 0.0);
        assert_eq!(layout.content.x, 300.0);
        assert_eq!(layout.content.width, 980.0);
    }

    #[test]
    fn layout_with_right_sidebar() {
        let layout = ChromeLayout::compute(1280.0, 800.0, true, 300.0, false, true);

        assert_eq!(layout.sidebar.x, 980.0);
        assert_eq!(layout.content.x, 0.0);
        assert_eq!(layout.content.width, 980.0);
    }

    #[test]
    fn layout_without_bookmark_bar() {
        let with_bar = ChromeLayout::compute(1280.0, 800.0, false, 300.0, true, true);
        let without_bar = ChromeLayout::compute(1280.0, 800.0, false, 300.0, true, false);

        // Without bookmark bar, content area should be taller
        assert!(without_bar.content.height > with_bar.content.height);
        assert_eq!(without_bar.bookmark_bar.height, 0.0);
    }

    #[test]
    fn rect_contains() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(50.0, 40.0));
        assert!(!r.contains(5.0, 40.0));
        assert!(!r.contains(50.0, 80.0));
    }
}
