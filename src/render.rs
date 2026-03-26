use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use unicode_width::UnicodeWidthChar;

use crate::layout::Frame;
use crate::icons::IconSet;

use crate::config::Theme;

pub struct Renderer {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub cell_w: f32,
    pub cell_h: f32,
    pub track_pad_x: f32,
    pub track_pad_y: f32,
    pub pill_pad_y: f32,
    pub theme: Theme,
    pub pill_opacity: f32,
    pub font_size: f32,
    pub font_family: String,
    shaped_cache: HashMap<(String, i32), Buffer>,
}

impl Renderer {
    pub fn new(font_family: &str, font_size: f32, settings: &crate::config::Settings) -> Self {
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

        log::info!("renderer: font={font_family} size={font_size} cell={cell_w}x{cell_h}");

        Self {
            font_system,
            swash_cache,
            cell_w,
            cell_h,
            track_pad_x: settings.track_pad_x(),
            track_pad_y: settings.track_pad_y(),
            pill_pad_y: settings.pill_pad_y(),
            theme: settings.theme,
            pill_opacity: settings.pill_opacity,
            font_size,
            font_family: font_family.to_string(),
            shaped_cache: HashMap::new(),
        }
    }

    pub fn bar_height(&self) -> u32 {
        (self.cell_h + 2.0 * self.pill_pad_y + 2.0 * self.track_pad_y).ceil() as u32
    }

    pub fn render_frame(
        &mut self, frame: &Frame, pixmap: &mut Pixmap,
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
        let track_pad = self.track_pad_x * sf * mul;
        let font_size = self.font_size * sf * mul;

        // Render each span
        for fspan in &frame.spans {
            let span_x = track_pad + fspan.rect.x * sf;
            let span_y = fspan.rect.y * sf;
            let span_w = fspan.rect.w * sf;
            let span_h = fspan.rect.h * sf;

            let span_opacity = fspan.opacity;

            // Draw rounded rect background
            if let Some(sbg) = fspan.bg {
                let radius = fspan.radius * sf;
                let bg_a = (sbg.a as f32 * span_opacity) as u8;
                if bg_a > 0 {
                    if let Some(path) = rounded_rect_path(span_x, span_y, span_w, span_h, radius) {
                        let mut paint = Paint::default();
                        paint.set_color_rgba8(sbg.r, sbg.g, sbg.b, bg_a);
                        paint.anti_alias = true;
                        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

                        // Neumorphic: soft offset shadows for raised pill
                        if matches!(self.theme, Theme::Neumorphic) {
                            let spread = (6.0 * sf).ceil() as i32;
                            let offset = (2.0 * sf).ceil() as i32;

                            // Dark shadow: offset down-right, soft spread
                            if let Some(shadow_path) = rounded_rect_path(
                                span_x + offset as f32, span_y + offset as f32,
                                span_w, span_h, radius,
                            ) {
                                for d in 1..=spread {
                                    let t = 1.0 - d as f32 / (spread + 1) as f32;
                                    let a = (0.25 * t * t * span_opacity * 255.0) as u8;
                                    if a == 0 { continue; }
                                    if let Some(p) = rounded_rect_path(
                                        span_x + (offset + d) as f32 * 0.5,
                                        span_y + (offset + d) as f32 * 0.5,
                                        span_w + d as f32 * 0.3,
                                        span_h + d as f32 * 0.3,
                                        radius + d as f32 * 0.2,
                                    ) {
                                        let mut sp = Paint::default();
                                        sp.set_color_rgba8(0, 0, 0, a);
                                        sp.anti_alias = true;
                                        let stroke = tiny_skia::Stroke { width: 1.5, ..Default::default() };
                                        pixmap.stroke_path(&p, &sp, &stroke, Transform::identity(), None);
                                    }
                                }
                            }

                            // Light highlight: offset up-left, soft spread
                            for d in 1..=spread {
                                let t = 1.0 - d as f32 / (spread + 1) as f32;
                                let a = (0.2 * t * t * span_opacity * 255.0) as u8;
                                if a == 0 { continue; }
                                if let Some(p) = rounded_rect_path(
                                    span_x - (offset + d) as f32 * 0.5,
                                    span_y - (offset + d) as f32 * 0.5,
                                    span_w + d as f32 * 0.3,
                                    span_h + d as f32 * 0.3,
                                    radius + d as f32 * 0.2,
                                ) {
                                    let mut sp = Paint::default();
                                    sp.set_color_rgba8(255, 255, 255, a);
                                    sp.anti_alias = true;
                                    let stroke = tiny_skia::Stroke { width: 1.5, ..Default::default() };
                                    pixmap.stroke_path(&p, &sp, &stroke, Transform::identity(), None);
                                }
                            }

                            // Redraw pill on top to cover shadow overlap
                            let mut paint = Paint::default();
                            paint.set_color_rgba8(sbg.r, sbg.g, sbg.b, bg_a);
                            paint.anti_alias = true;
                            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                        }

                        // Glass: specular highlight, inner shadow, edge glow
                        if matches!(self.theme, Theme::Glass) {
                            let ix0 = span_x as i32;
                            let ix1 = (span_x + span_w) as i32;
                            let iy0 = span_y as i32;
                            let iy1 = (span_y + span_h) as i32;

                            // Edge glow: 1px bright border around pill
                            if let Some(border) = rounded_rect_path(
                                span_x + 0.5, span_y + 0.5,
                                span_w - 1.0, span_h - 1.0,
                                (radius - 0.5).max(0.0),
                            ) {
                                let mut edge_paint = Paint::default();
                                let edge_a = (0.2 * 255.0 * span_opacity) as u8;
                                edge_paint.set_color_rgba8(255, 255, 255, edge_a);
                                edge_paint.anti_alias = true;
                                let stroke = tiny_skia::Stroke {
                                    width: 1.0,
                                    ..Default::default()
                                };
                                pixmap.stroke_path(&border, &edge_paint, &stroke, Transform::identity(), None);
                            }

                            // Specular highlight: bright gradient at top (20% of pill height)
                            let hi_h = ((span_h * 0.2).ceil() as i32).max(2);
                            let r_inset = (radius * 0.4) as i32;
                            for dy in 0..hi_h {
                                let t = 1.0 - (dy as f32 / hi_h as f32);
                                let a = 0.3 * t * t * span_opacity;
                                let py = iy0 + 1 + dy;
                                for px in (ix0 + r_inset)..(ix1 - r_inset) {
                                    Self::blend_pixel(pixmap.data_mut(), w, h, px, py, 255.0, 255.0, 255.0, a);
                                }
                            }

                            // Inner shadow at bottom (depth separation)
                            let sh_h = ((span_h * 0.15).ceil() as i32).max(2);
                            for dy in 0..sh_h {
                                let t = 1.0 - (dy as f32 / sh_h as f32);
                                let a = 0.12 * t * span_opacity;
                                let py = iy1 - 2 - dy;
                                for px in (ix0 + r_inset)..(ix1 - r_inset) {
                                    Self::blend_pixel(pixmap.data_mut(), w, h, px, py, 0.0, 0.0, 0.0, a);
                                }
                            }
                        }
                    }
                }
            }

            if span_opacity <= 0.0 { continue; }

            // Text y centered within span
            let text_y = span_y + (span_h - cell_h) / 2.0;

            // Render each element within the span
            for felem in &fspan.elems {
                let elem_x = track_pad + felem.rect.x * sf;
                let elem_y = text_y;

                // Apply span opacity to element fg
                let mut fg = felem.fg;
                fg.a = (fg.a as f32 * span_opacity) as u8;
                if fg.a == 0 { continue; }

                let mut cx = elem_x;

                // Render inline icon pixmap
                if let Some(ref icon_pm) = felem.icon {
                    let icon_w = icon_pm.width() as f32;
                    let icon_h = icon_pm.height() as f32;
                    let dy = elem_y + (cell_h - icon_h) / 2.0;
                    Self::composite_icon_at(
                        pixmap.data_mut(), w, h,
                        icon_pm, cx as i32, dy as i32, fg,
                    );
                    cx += icon_w + cell_w * 0.5;
                }

                // Walk text characters
                let mut dim = false;
                let mut text_run = String::new();
                let mut text_run_start = cx;
                let mut text_fg = fg;

                for ch in felem.text.chars() {
                    if ch == '\x01' { dim = true; continue; }
                    if ch == '\x02' { dim = false; continue; }

                    let mut char_fg = fg;
                    if dim {
                        char_fg.a = (char_fg.a as f32 * 0.615) as u8;
                    }

                    if IconSet::is_icon_char(ch) {
                        let run_w = self.draw_text_run(pixmap, &text_run, text_run_start, elem_y, font_size, cell_h, text_fg);
                        cx = text_run_start + run_w;
                        text_run.clear();

                        if let Some(icon_pm) = icons.icon_for_char(ch) {
                            let icon_w = icon_pm.width() as f32;
                            let icon_h = icon_pm.height() as f32;
                            let dy = elem_y + (cell_h - icon_h) / 2.0;
                            Self::composite_icon_at(
                                pixmap.data_mut(), w, h,
                                icon_pm, cx as i32, dy as i32, char_fg,
                            );
                            cx += icon_w;
                        } else {
                            cx += cell_w;
                        }
                        text_run_start = cx;
                        text_fg = char_fg;
                    } else {
                        if text_run.is_empty() {
                            text_run_start = cx;
                            text_fg = char_fg;
                        } else if char_fg != text_fg {
                            let run_w = self.draw_text_run(pixmap, &text_run, text_run_start, elem_y, font_size, cell_h, text_fg);
                            cx = text_run_start + run_w;
                            text_run.clear();
                            text_run_start = cx;
                            text_fg = char_fg;
                        }
                        text_run.push(ch);
                        cx += cell_w * ch.width().unwrap_or(1) as f32;
                    }
                }
                // Flush remaining text
                self.draw_text_run(pixmap, &text_run, text_run_start, elem_y, font_size, cell_h, text_fg);
            }
        }
    }

    fn draw_text_run(
        &mut self, pixmap: &mut Pixmap, text: &str, x: f32, y: f32,
        font_size: f32, cell_h: f32, fg: crate::color::Rgba,
    ) -> f32 {
        if text.is_empty() { return 0.0; }

        let scale = (font_size * 100.0) as i32;
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

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    if r <= 0.0 {
        // Simple rect
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        return pb.finish();
    }

    // Kappa for circular arcs approximated with cubic beziers
    let k = 0.5522847498;
    let kr = k * r;

    let mut pb = PathBuilder::new();
    // Start at top-left after the corner radius
    pb.move_to(x + r, y);
    // Top edge
    pb.line_to(x + w - r, y);
    // Top-right corner
    pb.cubic_to(x + w - r + kr, y, x + w, y + r - kr, x + w, y + r);
    // Right edge
    pb.line_to(x + w, y + h - r);
    // Bottom-right corner
    pb.cubic_to(x + w, y + h - r + kr, x + w - r + kr, y + h, x + w - r, y + h);
    // Bottom edge
    pb.line_to(x + r, y + h);
    // Bottom-left corner
    pb.cubic_to(x + r - kr, y + h, x, y + h - r + kr, x, y + h - r);
    // Left edge
    pb.line_to(x, y + r);
    // Top-left corner
    pb.cubic_to(x, y + r - kr, x + r - kr, y, x + r, y);
    pb.close();
    pb.finish()
}
