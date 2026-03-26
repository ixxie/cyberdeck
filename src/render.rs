use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use tiny_skia::Pixmap;

use unicode_width::UnicodeWidthChar;

use crate::layout::Layout;
use crate::icons::IconSet;

pub struct Renderer {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub cell_w: f32,
    pub cell_h: f32,
    pub padding: f32,
    pub pad_left: f32,
    pub pad_right: f32,
    pub font_size: f32,
    pub font_family: String,
    shaped_cache: HashMap<(String, i32), Buffer>,
}

impl Renderer {
    pub fn new(font_family: &str, font_size: f32, padding: f32, pad_left: Option<f32>, pad_right: Option<f32>) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        // Measure cell dimensions by shaping 'M'
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buf = Buffer::new(&mut font_system, metrics);
        {
            let mut buf = buf.borrow_with(&mut font_system);
            buf.set_text(
                "M",
                Attrs::new().family(Family::Name(font_family)),
                Shaping::Advanced,
            );
            buf.shape_until_scroll(true);
        }

        let cell_w = buf.layout_runs().next()
            .and_then(|run| run.glyphs.first())
            .map(|g| g.w)
            .unwrap_or(font_size * 0.6);
        let cell_h = font_size * 1.2;

        log::info!("renderer: font={font_family} size={font_size} cell={cell_w}x{cell_h} pad={padding}");

        Self {
            font_system,
            swash_cache,
            cell_w,
            cell_h,
            padding,
            pad_left: pad_left.unwrap_or(padding),
            pad_right: pad_right.unwrap_or(padding),
            font_size,
            font_family: font_family.to_string(),
            shaped_cache: HashMap::new(),
        }
    }

    pub fn bar_height(&self) -> u32 {
        (self.cell_h + 2.0 * self.padding).ceil() as u32
    }

    pub fn render_layout(
        &mut self, layout: &Layout, pixmap: &mut Pixmap,
        icons: &IconSet, bg: crate::color::Rgba, scale: i32, output_mul: f32,
    ) {
        // Fill background
        {
            let data = pixmap.data_mut();
            for i in (0..data.len()).step_by(4) {
                data[i] = bg.r;
                data[i + 1] = bg.g;
                data[i + 2] = bg.b;
                data[i + 3] = bg.a;
            }
        }

        if self.shaped_cache.len() > 256 {
            self.shaped_cache.clear();
        }

        let w = pixmap.width();
        let h = pixmap.height();
        let sf = scale as f32;
        let mul = output_mul;
        let cell_w = self.cell_w * sf * mul;
        let cell_h = self.cell_h * sf * mul;
        let pad_left = self.pad_left * sf * mul;
        let pad_v = self.padding * sf * mul;
        let font_size = self.font_size * sf * mul;

        // Render each layout item
        for item in &layout.items {
            let item_x = pad_left + item.x * sf;
            let item_y = pad_v;
            let item_w = item.width * sf;

            // Fill item background if set
            if let Some(ibg) = item.bg {
                let x0 = item_x as u32;
                let y0 = 0u32;
                let x1 = (item_x + item_w).ceil() as u32;
                let y1 = h;
                let data = pixmap.data_mut();
                for py in y0..y1.min(h) {
                    for px in x0..x1.min(w) {
                        let idx = (py * w + px) as usize * 4;
                        if idx + 3 < data.len() {
                            data[idx] = ibg.r;
                            data[idx + 1] = ibg.g;
                            data[idx + 2] = ibg.b;
                            data[idx + 3] = ibg.a;
                        }
                    }
                }
            }

            // Walk through the text, splitting into icon and text segments
            let mut cx = item_x;
            let mut dim = false;
            let mut text_run = String::new();
            let mut text_run_start = cx;
            let mut text_fg = item.fg;

            for ch in item.text.chars() {
                if ch == '\x01' { dim = true; continue; }
                if ch == '\x02' { dim = false; continue; }

                let mut fg = item.fg;
                if dim {
                    fg.a = (fg.a as f32 * 0.615) as u8; // dim to idle
                }

                if IconSet::is_icon_char(ch) {
                    // Flush pending text
                    let run_w = self.draw_text_run(pixmap, &text_run, text_run_start, item_y, font_size, cell_h, text_fg);
                    cx = text_run_start + run_w;
                    text_run.clear();

                    if let Some(icon_pm) = icons.icon_for_char(ch) {
                        let icon_w = icon_pm.width() as f32;
                        let icon_h = icon_pm.height() as f32;
                        let dy = item_y + (cell_h - icon_h) / 2.0;
                        Self::composite_icon_at(
                            pixmap.data_mut(), w, h,
                            icon_pm, cx as i32, dy as i32, fg,
                        );
                        cx += icon_w;
                    } else {
                        cx += cell_w;
                    }
                    text_run_start = cx;
                    text_fg = fg;
                } else {
                    if text_run.is_empty() {
                        text_run_start = cx;
                        text_fg = fg;
                    } else if fg != text_fg {
                        // fg changed, flush
                        let run_w = self.draw_text_run(pixmap, &text_run, text_run_start, item_y, font_size, cell_h, text_fg);
                        cx = text_run_start + run_w;
                        text_run.clear();
                        text_run_start = cx;
                        text_fg = fg;
                    }
                    text_run.push(ch);
                    cx += cell_w * ch.width().unwrap_or(1) as f32;
                }
            }
            // Flush remaining text
            self.draw_text_run(pixmap, &text_run, text_run_start, item_y, font_size, cell_h, text_fg);
        }
    }

    fn draw_text_run(
        &mut self, pixmap: &mut Pixmap, text: &str, x: f32, y: f32,
        font_size: f32, cell_h: f32, fg: crate::color::Rgba,
    ) -> f32 {
        if text.is_empty() { return 0.0; }

        let scale = (font_size * 100.0) as i32; // use font_size as proxy for scale
        let buf = self.shaped_cache.entry((text.to_string(), scale)).or_insert_with(|| {
            let metrics = Metrics::new(font_size, cell_h);
            let mut buf = Buffer::new(&mut self.font_system, metrics);
            {
                let mut borrowed = buf.borrow_with(&mut self.font_system);
                borrowed.set_text(
                    &text,
                    Attrs::new().family(Family::Name(&self.font_family)),
                    Shaping::Advanced,
                );
                borrowed.shape_until_scroll(true);
            }
            buf
        });

        // Compute actual shaped width from glyph layout
        let shaped_w: f32 = buf.layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .map(|g| g.w)
            .sum();

        let fg_alpha = fg.a as f32 / 255.0;
        let color = Color::rgba(fg.r, fg.g, fg.b, 255);
        let x_off = x as i32;
        let y_off = y as i32;
        let w = pixmap.width();
        let h = pixmap.height();
        let data = pixmap.data_mut();
        buf.draw(&mut self.font_system, &mut self.swash_cache, color, |gx, gy, _w, _h, color| {
            let alpha = color.a() as f32 / 255.0 * fg_alpha;
            if alpha > 0.0 {
                Self::blend_pixel(
                    data, w, h,
                    x_off + gx, y_off + gy,
                    fg.r as f32, fg.g as f32, fg.b as f32,
                    alpha,
                );
            }
        });
        shaped_w
    }

    // Icon compositing at exact pixel position, respecting fg alpha
    fn composite_icon_at(
        data: &mut [u8],
        buf_w: u32, buf_h: u32,
        icon: &Pixmap,
        x: i32, y: i32,
        fg: crate::color::Rgba,
    ) {
        let icon_w = icon.width() as i32;
        let icon_h = icon.height() as i32;
        let icon_data = icon.data();
        let fg_alpha = fg.a as f32 / 255.0;

        for iy in 0..icon_h {
            for ix in 0..icon_w {
                let px = x + ix;
                let py = y + iy;
                if px < 0 || py < 0 || (px as u32) >= buf_w || (py as u32) >= buf_h {
                    continue;
                }
                let src_idx = (iy * icon_w + ix) as usize * 4;
                let alpha = icon_data[src_idx + 3] as f32 / 255.0 * fg_alpha;
                if alpha > 0.0 {
                    Self::blend_pixel(
                        data, buf_w, buf_h,
                        px, py,
                        fg.r as f32, fg.g as f32, fg.b as f32,
                        alpha,
                    );
                }
            }
        }
    }

    fn blend_pixel(
        data: &mut [u8], w: u32, h: u32,
        px: i32, py: i32,
        sr: f32, sg: f32, sb: f32, alpha: f32,
    ) {
        if px < 0 || py < 0 || (px as u32) >= w || (py as u32) >= h {
            return;
        }
        let idx = (py as u32 * w + px as u32) as usize * 4;
        if idx + 3 >= data.len() {
            return;
        }
        let da = data[idx + 3] as f32 / 255.0;
        let out_a = alpha + da * (1.0 - alpha);
        if out_a > 0.0 {
            let inv = 1.0 / out_a;
            let dr = data[idx] as f32;
            let dg = data[idx + 1] as f32;
            let db = data[idx + 2] as f32;
            data[idx] = ((sr * alpha + dr * da * (1.0 - alpha)) * inv) as u8;
            data[idx + 1] = ((sg * alpha + dg * da * (1.0 - alpha)) * inv) as u8;
            data[idx + 2] = ((sb * alpha + db * da * (1.0 - alpha)) * inv) as u8;
            data[idx + 3] = (out_a * 255.0) as u8;
        }
    }

    // Copy RGBA pixmap → Wayland premultiplied ARGB8888 (BGRA byte order) canvas
    pub fn copy_to_wl_buffer(pixmap: &Pixmap, canvas: &mut [u8]) {
        let src = pixmap.data();
        let len = canvas.len().min(src.len());
        let mut i = 0;
        while i + 3 < len {
            let a = src[i + 3];
            if a == 255 {
                canvas[i] = src[i + 2];
                canvas[i + 1] = src[i + 1];
                canvas[i + 2] = src[i];
                canvas[i + 3] = 255;
            } else {
                let af = a as f32 / 255.0;
                canvas[i] = (src[i + 2] as f32 * af) as u8;
                canvas[i + 1] = (src[i + 1] as f32 * af) as u8;
                canvas[i + 2] = (src[i] as f32 * af) as u8;
                canvas[i + 3] = a;
            }
            i += 4;
        }
    }
}
