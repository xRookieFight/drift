//! Where everything goes in the window. Pure math on physical pixels so it can be unit tested
//! without a GPU. The UI webview always covers the whole window, content panes sit on top of it.

use serde::Serialize;

/// Sidebar width in CSS pixels. ui/style.css uses the same value, keep them in sync.
pub const SIDEBAR_WIDTH: f32 = 248.0;
/// Height of the address and navigation bar above the content card, in CSS pixels.
/// ui/style.css --toolbar-height uses the same value.
pub const TOOLBAR_HEIGHT: f32 = 46.0;
/// Space between the content card and the window edge, in CSS pixels.
pub const CARD_MARGIN: f32 = 8.0;
/// Space between the two panes in split view, in CSS pixels. The divider handle lives here.
pub const SPLIT_GAP: f32 = 8.0;
/// Split ratio limits, so neither pane can be squeezed to nothing.
pub const MIN_RATIO: f32 = 0.2;
pub const MAX_RATIO: f32 = 0.8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x as f64
            && py >= self.y as f64
            && px < (self.x + self.w) as f64
            && py < (self.y + self.h) as f64
    }

    /// GL wants the origin at the bottom left, so flip y against the window height.
    pub fn to_gl(self, window_height: i32) -> Rect {
        Rect::new(self.x, window_height - self.y - self.h, self.w, self.h)
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// Part of this rect that lies inside `other`, empty when they don't touch.
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    /// Same rect in CSS pixels, which is what the UI webview thinks in.
    pub fn to_css(self, scale: f32) -> CssRect {
        CssRect {
            x: self.x as f32 / scale,
            y: self.y as f32 / scale,
            w: self.w as f32 / scale,
            h: self.h as f32 / scale,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CssRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl CssRect {
    /// Back to physical pixels, rounded outwards so a floating UI rect never loses its edge.
    pub fn to_physical(self, scale: f32) -> Rect {
        let x0 = (self.x * scale).floor() as i32;
        let y0 = (self.y * scale).floor() as i32;
        let x1 = ((self.x + self.w) * scale).ceil() as i32;
        let y1 = ((self.y + self.h) * scale).ceil() as i32;
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

pub fn clamp_ratio(ratio: f32) -> f32 {
    if ratio.is_nan() {
        return 0.5;
    }
    ratio.clamp(MIN_RATIO, MAX_RATIO)
}

/// The rounded card that holds page content: right of the sidebar, below the toolbar,
/// with a margin on the remaining sides.
pub fn content_rect(window_w: u32, window_h: u32, scale: f32) -> Rect {
    let sidebar = (SIDEBAR_WIDTH * scale).round() as i32;
    let toolbar = (TOOLBAR_HEIGHT * scale).round() as i32;
    let margin = (CARD_MARGIN * scale).round() as i32;
    let (w, h) = (window_w as i32, window_h as i32);
    Rect::new(
        sidebar,
        toolbar,
        (w - sidebar - margin).max(1),
        (h - toolbar - margin).max(1),
    )
}

/// Splits the content card into one or two panes. `ratio` is the left pane's share.
pub fn panes(content: Rect, split: Option<f32>, scale: f32) -> Vec<Rect> {
    let Some(ratio) = split else {
        return vec![content];
    };
    let gap = (SPLIT_GAP * scale).round() as i32;
    let usable = (content.w - gap).max(2);
    let left_w = ((usable as f32) * clamp_ratio(ratio)).round() as i32;
    let left_w = left_w.clamp(1, usable - 1);
    let right_w = usable - left_w;
    vec![
        Rect::new(content.x, content.y, left_w, content.h),
        Rect::new(content.x + left_w + gap, content.y, right_w, content.h),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_fills_card() {
        let c = content_rect(1280, 800, 1.0);
        assert_eq!(c, Rect::new(248, 46, 1280 - 248 - 8, 800 - 46 - 8));
        assert_eq!(panes(c, None, 1.0), vec![c]);
    }

    #[test]
    fn hidpi_scales_chrome() {
        let c = content_rect(2560, 1600, 2.0);
        assert_eq!(c.x, 496);
        assert_eq!(c.y, 92);
        assert_eq!(c.h, 1600 - 92 - 16);
    }

    #[test]
    fn split_panes_cover_card_without_overlap() {
        let c = content_rect(1280, 800, 1.0);
        let p = panes(c, Some(0.5), 1.0);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].x, c.x);
        assert_eq!(p[0].x + p[0].w + 8, p[1].x);
        assert_eq!(p[1].x + p[1].w, c.x + c.w);
        assert!((p[0].w - p[1].w).abs() <= 1);
    }

    #[test]
    fn ratio_is_clamped() {
        let c = content_rect(1280, 800, 1.0);
        let p = panes(c, Some(0.0), 1.0);
        assert!(p[0].w as f32 >= (c.w - 8) as f32 * MIN_RATIO - 1.0);
        assert_eq!(clamp_ratio(f32::NAN), 0.5);
        assert_eq!(clamp_ratio(2.0), MAX_RATIO);
    }

    #[test]
    fn intersect_clips_and_detects_misses() {
        let a = Rect::new(0, 0, 100, 100);
        assert_eq!(
            a.intersect(&Rect::new(50, 50, 100, 100)),
            Rect::new(50, 50, 50, 50)
        );
        assert!(a.intersect(&Rect::new(200, 0, 10, 10)).is_empty());
    }

    #[test]
    fn gl_flip() {
        assert_eq!(
            Rect::new(10, 20, 30, 40).to_gl(100),
            Rect::new(10, 40, 30, 40)
        );
    }

    #[test]
    fn tiny_window_never_goes_negative() {
        let c = content_rect(100, 10, 1.0);
        assert!(c.w >= 1 && c.h >= 1);
        let p = panes(c, Some(0.5), 1.0);
        assert!(p.iter().all(|r| r.w >= 1));
    }
}
