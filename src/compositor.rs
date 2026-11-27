//! Draws floating pieces of the UI (the address panel, dialogs, popup menus) over the pages.
//!
//! The normal frame is UI first, panes on top, both through Servo's blit callback. That
//! callback copies whole opaque framebuffers and Servo keeps the framebuffer ids private, so
//! a popup living in the UI webview would end up under the page. For each floating box the UI
//! reports, we read back the window (pages already drawn) and the UI for that region, mix
//! them on the CPU (rounded corners, soft shadow, an optional dimming scrim) and copy the
//! result into the window. Only while something floats, and only that region.

use std::sync::Arc;

use glow::HasContext;
use image::RgbaImage;
use serde::Deserialize;

use crate::layout::Rect;

/// A floating box as the UI describes it, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct FloatSpec {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub radius: f32,
    /// Blur distance of the drop shadow.
    #[serde(default)]
    pub shadow: f32,
    /// 0..1, how much to darken everything around the box (modal dialogs).
    #[serde(default)]
    pub scrim: f32,
}

/// Same thing in window pixels, ready to composite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Float {
    /// The box itself.
    pub rect: Rect,
    /// Everything that gets touched: box plus shadow, or the whole window with a scrim.
    pub region: Rect,
    pub radius: f32,
    pub shadow: f32,
    pub scrim: f32,
}

/// How dark the shadow gets right at the box edge.
const SHADOW_STRENGTH: f32 = 0.38;
/// Shadows fall a little downwards, like light from above.
const SHADOW_OFFSET: f32 = 0.25;

impl FloatSpec {
    pub fn to_float(self, scale: f32, window: Rect) -> Float {
        let rect = crate::layout::CssRect {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
        }
        .to_physical(scale);
        let shadow = (self.shadow * scale).max(0.0);
        let scrim = self.scrim.clamp(0.0, 1.0);
        let pad = shadow.ceil() as i32;
        let region = if scrim > 0.0 {
            window
        } else {
            Rect::new(
                rect.x - pad,
                rect.y - pad,
                rect.w + 2 * pad,
                rect.h + 2 * pad + (shadow * SHADOW_OFFSET).ceil() as i32,
            )
        };
        Float {
            rect,
            region: region.intersect(&window),
            radius: (self.radius * scale).max(0.0),
            shadow,
            scrim,
        }
    }
}

/// Signed distance from a point to a rounded rect: negative inside, positive outside.
fn rounded_rect_distance(px: f32, py: f32, r: &Rect, radius: f32) -> f32 {
    let (hw, hh) = (r.w as f32 / 2.0, r.h as f32 / 2.0);
    let radius = radius.min(hw).min(hh);
    let cx = px - (r.x as f32 + hw);
    let cy = py - (r.y as f32 + hh);
    let qx = cx.abs() - (hw - radius);
    let qy = cy.abs() - (hh - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Mixes `ui` over `base` for `float`. Both images cover `float.region`, top row first.
pub fn composite(base: &RgbaImage, ui: &RgbaImage, float: &Float) -> RgbaImage {
    let (w, h) = base.dimensions();
    let mut out = RgbaImage::new(w, h);
    let shadow_rect = Rect::new(
        float.rect.x,
        float.rect.y + (float.shadow * SHADOW_OFFSET) as i32,
        float.rect.w,
        float.rect.h,
    );
    for y in 0..h {
        for x in 0..w {
            // pixel centre in window coordinates
            let px = float.region.x as f32 + x as f32 + 0.5;
            let py = float.region.y as f32 + y as f32 + 0.5;
            let d = rounded_rect_distance(px, py, &float.rect, float.radius);
            let coverage = (0.5 - d).clamp(0.0, 1.0);

            let mut dim = 1.0 - float.scrim;
            if float.shadow > 0.0 {
                let ds = rounded_rect_distance(px, py, &shadow_rect, float.radius);
                let s = SHADOW_STRENGTH * (1.0 - smoothstep(-float.shadow * 0.3, float.shadow, ds));
                dim *= 1.0 - s;
            }

            let b = base.get_pixel(x, y).0;
            let u = ui.get_pixel(x, y).0;
            let mut px_out = [0u8; 4];
            for c in 0..3 {
                let under = b[c] as f32 * dim;
                px_out[c] = (u[c] as f32 * coverage + under * (1.0 - coverage)).round() as u8;
            }
            px_out[3] = 255;
            out.put_pixel(x, y, image::Rgba(px_out));
        }
    }
    out
}

pub struct OverlayBlitter {
    gl: Arc<glow::Context>,
    framebuffer: Option<glow::NativeFramebuffer>,
    texture: Option<glow::NativeTexture>,
}

impl OverlayBlitter {
    pub fn new(gl: Arc<glow::Context>) -> Self {
        Self {
            gl,
            framebuffer: None,
            texture: None,
        }
    }

    /// The framebuffer currently bound for drawing. Ask right after Servo's pane blits, that
    /// is the window's, a readback afterwards rebinds another one.
    pub fn window_framebuffer(&self) -> Option<glow::NativeFramebuffer> {
        unsafe {
            self.gl
                .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING)
        }
    }

    /// Copies `image` (top row first) into `window_fb` at `target`, which is in GL
    /// coordinates (origin bottom left).
    pub fn draw(
        &mut self,
        image: &RgbaImage,
        target: Rect,
        window_fb: Option<glow::NativeFramebuffer>,
    ) {
        let (w, h) = (image.width() as i32, image.height() as i32);
        if w == 0 || h == 0 || target.is_empty() {
            return;
        }
        // yeah, raw GL. Every call here only touches objects we created, and the window
        // framebuffer is bound again at the end
        unsafe {
            let gl = &self.gl;
            let texture = match self.texture {
                Some(t) => t,
                None => match gl.create_texture() {
                    Ok(t) => *self.texture.insert(t),
                    Err(err) => {
                        log::warn!("overlay texture: {err}");
                        return;
                    }
                },
            };
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(image.as_raw())),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);

            let framebuffer = match self.framebuffer {
                Some(f) => f,
                None => match gl.create_framebuffer() {
                    Ok(f) => *self.framebuffer.insert(f),
                    Err(err) => {
                        log::warn!("overlay framebuffer: {err}");
                        return;
                    }
                },
            };
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, window_fb);

            // texture row 0 is the top of the image, GL's y grows upwards, so flip on the way
            gl.blit_framebuffer(
                0,
                0,
                w,
                h,
                target.x,
                target.y + target.h,
                target.x + target.w,
                target.y,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, window_fb);
        }
    }
}

impl Drop for OverlayBlitter {
    fn drop(&mut self) {
        unsafe {
            if let Some(f) = self.framebuffer.take() {
                self.gl.delete_framebuffer(f);
            }
            if let Some(t) = self.texture.take() {
                self.gl.delete_texture(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba(c))
    }

    fn float(rect: Rect, region: Rect, radius: f32, shadow: f32, scrim: f32) -> Float {
        Float {
            rect,
            region,
            radius,
            shadow,
            scrim,
        }
    }

    #[test]
    fn box_takes_ui_pixels_outside_keeps_page() {
        let f = float(
            Rect::new(10, 10, 20, 20),
            Rect::new(0, 0, 40, 40),
            0.0,
            0.0,
            0.0,
        );
        let out = composite(
            &solid(40, 40, [200, 0, 0, 255]),
            &solid(40, 40, [0, 0, 200, 255]),
            &f,
        );
        assert_eq!(out.get_pixel(20, 20).0, [0, 0, 200, 255]);
        assert_eq!(out.get_pixel(2, 2).0, [200, 0, 0, 255]);
    }

    #[test]
    fn rounded_corners_show_the_page() {
        let f = float(
            Rect::new(0, 0, 40, 40),
            Rect::new(0, 0, 40, 40),
            12.0,
            0.0,
            0.0,
        );
        let out = composite(
            &solid(40, 40, [255, 255, 255, 255]),
            &solid(40, 40, [0, 0, 0, 255]),
            &f,
        );
        // the very corner is outside the rounding, the middle is solid box
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(20, 20).0, [0, 0, 0, 255]);
    }

    #[test]
    fn shadow_darkens_near_the_box_only() {
        let f = float(
            Rect::new(20, 20, 20, 20),
            Rect::new(0, 0, 60, 70),
            4.0,
            16.0,
            0.0,
        );
        let out = composite(
            &solid(60, 70, [200, 200, 200, 255]),
            &solid(60, 70, [0, 0, 0, 255]),
            &f,
        );
        let near = out.get_pixel(30, 43).0[0];
        let far = out.get_pixel(1, 1).0[0];
        assert!(near < 200, "shadow under the box: {near}");
        assert_eq!(far, 200);
    }

    #[test]
    fn scrim_dims_everything_around() {
        let f = float(
            Rect::new(10, 10, 10, 10),
            Rect::new(0, 0, 30, 30),
            0.0,
            0.0,
            0.5,
        );
        let out = composite(
            &solid(30, 30, [200, 200, 200, 255]),
            &solid(30, 30, [0, 0, 0, 255]),
            &f,
        );
        assert_eq!(out.get_pixel(1, 1).0, [100, 100, 100, 255]);
    }

    #[test]
    fn spec_with_scrim_covers_the_window() {
        let window = Rect::new(0, 0, 800, 600);
        let spec = FloatSpec {
            x: 100.0,
            y: 100.0,
            w: 200.0,
            h: 100.0,
            radius: 8.0,
            shadow: 20.0,
            scrim: 0.3,
        };
        assert_eq!(spec.to_float(1.0, window).region, window);
        let spec = FloatSpec { scrim: 0.0, ..spec };
        let f = spec.to_float(2.0, window);
        assert_eq!(f.rect, Rect::new(200, 200, 400, 200));
        assert!(f.region.x < 200 && f.region.x + f.region.w > 600);
    }
}
