use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use unicode_width::UnicodeWidthChar;

use crate::layout::Frame;
use crate::icons::IconSet;

pub struct Renderer {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
    pub font_family: String,
    pub emoji_font: String,
    shaped_cache: HashMap<(String, i32, bool), Buffer>,
}

impl Renderer {
    pub fn new(font_family: &str, font_size: f32, settings: &crate::config::Settings) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

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

        log::info!("renderer: font={font_family} emoji={} size={font_size} cell={cell_w}x{cell_h}", settings.emoji_font);

        Self {
            font_system,
            swash_cache,
            cell_w,
            cell_h,
            font_size,
            font_family: font_family.to_string(),
            emoji_font: settings.emoji_font.clone(),
            shaped_cache: HashMap::new(),
        }
    }

    pub fn bar_height(&self, settings: &crate::config::Settings) -> u32 {
        let track = settings.resolve_track();
        let pill = settings.resolve_pill();
        (self.cell_h + 2.0 * pill.padding + 2.0 * track.padding).ceil() as u32
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
        let font_size = self.font_size * sf * mul;

        for fspan in &frame.spans {
            let span_x = fspan.rect.x * sf;
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
                    }
                }
            }

            if span_opacity <= 0.0 { continue; }

            // Text y centered within span
            let text_y = span_y + (span_h - cell_h) / 2.0;

            for felem in &fspan.elems {
                let elem_x = felem.rect.x * sf;
                let elem_y = text_y;

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
                        icon_pm, cx.round() as i32, dy.round() as i32, fg,
                    );
                    cx += icon_w + cell_w * 0.5;
                }

                // Walk text characters
                let mut dim = false;
                let mut text_run = String::new();
                let mut text_run_start = cx;
                let mut text_fg = fg;
                let mut run_is_emoji = false;

                for ch in felem.text.chars() {
                    if ch == '\x01' { dim = true; continue; }
                    if ch == '\x02' { dim = false; continue; }

                    let mut char_fg = fg;
                    if dim {
                        char_fg.a = (char_fg.a as f32 * 0.615) as u8;
                    }

                    if IconSet::is_icon_char(ch) {
                        let run_w = self.draw_text_run(pixmap, &text_run, text_run_start, elem_y, font_size, cell_h, text_fg, run_is_emoji);
                        cx = text_run_start + run_w;
                        text_run.clear();

                        if let Some(icon_pm) = icons.icon_for_char(ch) {
                            let icon_w = icon_pm.width() as f32;
                            let icon_h = icon_pm.height() as f32;
                            let dy = elem_y + (cell_h - icon_h) / 2.0;
                            Self::composite_icon_at(
                                pixmap.data_mut(), w, h,
                                icon_pm, cx.round() as i32, dy.round() as i32, char_fg,
                            );
                            cx += icon_w;
                        } else {
                            cx += cell_w;
                        }
                        text_run_start = cx;
                        text_fg = char_fg;
                        run_is_emoji = false;
                    } else {
                        let ch_emoji = is_emoji(ch);
                        let needs_flush = if text_run.is_empty() {
                            false
                        } else if char_fg != text_fg || ch_emoji != run_is_emoji {
                            true
                        } else {
                            false
                        };

                        if needs_flush {
                            let run_w = self.draw_text_run(pixmap, &text_run, text_run_start, elem_y, font_size, cell_h, text_fg, run_is_emoji);
                            cx = text_run_start + run_w;
                            text_run.clear();
                        }

                        if text_run.is_empty() {
                            text_run_start = cx;
                            text_fg = char_fg;
                            run_is_emoji = ch_emoji;
                        }
                        text_run.push(ch);
                        cx += cell_w * ch.width().unwrap_or(1) as f32;
                    }
                }
                // Flush remaining text
                self.draw_text_run(pixmap, &text_run, text_run_start, elem_y, font_size, cell_h, text_fg, run_is_emoji);
            }
        }
    }

    fn draw_text_run(
        &mut self, pixmap: &mut Pixmap, text: &str, x: f32, y: f32,
        font_size: f32, cell_h: f32, fg: crate::color::Rgba, emoji: bool,
    ) -> f32 {
        if text.is_empty() { return 0.0; }

        let scale = (font_size * 100.0) as i32;
        let buf = self.shaped_cache.entry((text.to_string(), scale, emoji)).or_insert_with(|| {
            let metrics = Metrics::new(font_size, cell_h);
            let mut buf = Buffer::new(&mut self.font_system, metrics);
            let family = if emoji {
                Family::Name(&self.emoji_font)
            } else {
                Family::Name(&self.font_family)
            };
            {
                let mut borrowed = buf.borrow_with(&mut self.font_system);
                borrowed.set_text(
                    &text,
                    Attrs::new().family(family),
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

fn is_emoji(ch: char) -> bool {
    let c = ch as u32;
    matches!(c,
        0x200D |              // ZWJ
        0x203C | 0x2049 |    // ‼ ⁉
        0x20E3 |              // Combining enclosing keycap
        0x2122 | 0x2139 |    // ™ ℹ
        0x2194..=0x2199 |    // Arrows
        0x21A9..=0x21AA |    // ↩ ↪
        0x231A..=0x231B |    // ⌚ ⌛
        0x2328 |              // ⌨
        0x23CF |              // ⏏
        0x23E9..=0x23FA |    // ⏩..⏺
        0x24C2 |              // Ⓜ
        0x25AA..=0x25AB |    // ▪ ▫
        0x25B6 | 0x25C0 |    // ▶ ◀
        0x25FB..=0x25FE |    // ◻..◾
        0x2600..=0x27BF |    // Misc Symbols & Dingbats
        0x2934..=0x2935 |    // ⤴ ⤵
        0x2B05..=0x2B07 |    // ⬅..⬇
        0x2B1B..=0x2B1C |    // ⬛ ⬜
        0x2B50..=0x2B55 |    // ⭐ ⭕
        0x3030 | 0x303D |    // 〰 〽
        0x3297 | 0x3299 |    // ㊗ ㊙
        0xFE00..=0xFE0F |    // Variation selectors
        0x1F000..=0x1FAFF |  // All SMP emoji blocks
        0x1FC00..=0x1FCFF |  // Symbols for Legacy Computing (some emoji)
        0xE0020..=0xE007F    // Tag characters (flag sequences)
    )
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    if r <= 0.0 {
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        return pb.finish();
    }

    let k = 0.5522847498;
    let kr = k * r;

    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + kr, y, x + w, y + r - kr, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + kr, x + w - r + kr, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - kr, y + h, x, y + h - r + kr, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - kr, x + r - kr, y, x + r, y);
    pb.close();
    pb.finish()
}
