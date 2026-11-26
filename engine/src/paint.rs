//! Own software rasterizer: paints the layout tree into an RGBA framebuffer.

use crate::css::Color;
use crate::font;
use crate::layout::{BoxKind, LayoutBox};

#[derive(Debug, Clone)]
pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    /// RGBA8
    pub data: Vec<u8>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Framebuffer {
        Framebuffer {
            width,
            height,
            data: vec![0u8; width * height * 4],
        }
    }

    pub fn clear(&mut self, c: Color) {
        for px in self.data.chunks_exact_mut(4) {
            px[0] = c.r;
            px[1] = c.g;
            px[2] = c.b;
            px[3] = c.a;
        }
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, c: Color) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = ((y as usize) * self.width + x as usize) * 4;
        self.data[i] = c.r;
        self.data[i + 1] = c.g;
        self.data[i + 2] = c.b;
        self.data[i + 3] = c.a;
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        let x0 = (x.floor() as i32).clamp(0, self.width as i32) as usize;
        let y0 = (y.floor() as i32).clamp(0, self.height as i32) as usize;
        let x1 = ((x + w).ceil() as i32).clamp(0, self.width as i32) as usize;
        let y1 = ((y + h).ceil() as i32).clamp(0, self.height as i32) as usize;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        // span writes per row instead of per-pixel bounds checks
        for py in y0..y1 {
            let row = py * self.width;
            let span = &mut self.data[(row + x0) * 4..(row + x1) * 4];
            for px in span.chunks_exact_mut(4) {
                px[0] = c.r;
                px[1] = c.g;
                px[2] = c.b;
                px[3] = c.a;
            }
        }
    }

    /// Border: four rects, widths top/right/bottom/left.
    pub fn stroke_border(&mut self, x: f32, y: f32, w: f32, h: f32, widths: [f32; 4], c: Color) {
        let (t, r, b, l) = (widths[0], widths[1], widths[2], widths[3]);
        if t > 0.0 {
            self.fill_rect(x, y, w, t, c);
        }
        if b > 0.0 {
            self.fill_rect(x, y + h - b, w, b, c);
        }
        if l > 0.0 {
            self.fill_rect(x, y, l, h, c);
        }
        if r > 0.0 {
            self.fill_rect(x + w - r, y, r, h, c);
        }
    }

    pub fn draw_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, c: Color) {
        let scale = font::scale_for(font_size).max(1);
        let mut cx = x;
        for ch in text.chars() {
            let g = font::glyph(ch);
            let gx0 = cx.round() as i32;
            let gy0 = y.round() as i32;
            for row in 0..font::GLYPH_H as usize {
                let bits = g[row];
                for col in 0..font::GLYPH_W as usize {
                    if bits & (1 << col) != 0 {
                        let px = gx0 + (col as u32 * scale) as i32;
                        let py = gy0 + (row as u32 * scale) as i32;
                        for dy in 0..scale as i32 {
                            for dx in 0..scale as i32 {
                                self.set_pixel(px + dx, py + dy, c);
                            }
                        }
                    }
                }
            }
            cx += font::char_advance(ch, scale);
        }
    }
}

/// Render a layout tree into a new framebuffer.
/// `ds` is the device scale: all coordinates and glyphs are multiplied by it.
pub fn render(root: &LayoutBox, width: usize, height: usize, scroll_y: f32, ds: u32) -> Framebuffer {
    let mut fb = Framebuffer::new(width, height);
    fb.clear(Color::rgb(255, 255, 255));
    let dsf = ds as f32;
    walk(root, 0.0, -scroll_y, &mut fb, dsf, ds);
    fb
}

fn walk(b: &LayoutBox, dx: f32, dy: f32, fb: &mut Framebuffer, dsf: f32, ds: u32) {
    // Cull subtrees fully outside the viewport (vertical axis) — this is what
    // keeps painting snappy on very tall pages.
    let y_dev = (b.y + dy) * dsf;
    if y_dev >= fb.height as f32 || y_dev + b.h * dsf <= 0.0 {
        return;
    }
    match b.kind {
        BoxKind::Block | BoxKind::Anon | BoxKind::Inline => {
            let x = (b.x + dx) * dsf;
            let y = (b.y + dy) * dsf;
            let w = b.w * dsf;
            let h = b.h * dsf;
            if let Some(bg) = b.style.background {
                fb.fill_rect(x, y, w, h, bg);
            }
            fb.stroke_border(x, y, w, h, b.style.border_width.map(|v| v * dsf), b.style.border_color);
            for c in &b.children {
                walk(c, dx, dy, fb, dsf, ds);
            }
        }
        BoxKind::Rect => {
            if let Some(bg) = b.style.background {
                fb.fill_rect((b.x + dx) * dsf, (b.y + dy) * dsf, b.w * dsf, b.h * dsf, bg);
            }
        }
        BoxKind::Text => {
            if b.text.is_empty() {
                return;
            }
            let scale = font::scale_for(b.style.font_size) * ds;
            let mut cx = (b.x + dx) * dsf;
            let y = (b.y + dy) * dsf;
            for ch in b.text.chars() {
                draw_glyph(fb, cx, y, ch, scale, b.style.color);
                cx += font::char_advance(ch, scale);
            }
        }
        BoxKind::Br => {}
    }
}

fn draw_glyph(fb: &mut Framebuffer, x: f32, y: f32, ch: char, scale: u32, c: Color) {
    let g = font::glyph(ch);
    let gx0 = x.round() as i32;
    let gy0 = y.round() as i32;
    let s = scale as f32;
    for row in 0..font::GLYPH_H as usize {
        let bits = g[row];
        for col in 0..font::GLYPH_W as usize {
            if bits & (1 << col) != 0 {
                fb.fill_rect(
                    (gx0 + (col as u32 * scale) as i32) as f32,
                    (gy0 + (row as u32 * scale) as i32) as f32,
                    s,
                    s,
                    c,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_and_clip() {
        let mut fb = Framebuffer::new(10, 10);
        fb.clear(Color::rgb(0, 0, 0));
        fb.fill_rect(-5.0, -5.0, 8.0, 8.0, Color::rgb(255, 0, 0));
        assert_eq!(&fb.data[0..4], &[255, 0, 0, 255]);
        // pixel at (9,9) untouched
        let i = (9 * 10 + 9) * 4;
        assert_eq!(&fb.data[i..i + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn draw_text_sets_pixels() {
        let mut fb = Framebuffer::new(40, 12);
        fb.clear(Color::rgb(255, 255, 255));
        fb.draw_text(1.0, 1.0, "A", 16.0, Color::rgb(0, 0, 0));
        let dark = fb.data.chunks_exact(4).filter(|p| p[0] == 0 && p[1] == 0 && p[2] == 0).count();
        assert!(dark > 5, "dark pixels: {}", dark);
    }
}
