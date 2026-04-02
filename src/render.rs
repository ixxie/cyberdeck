use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use tiny_skia::Pixmap;

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

    /// Shape text and return its rendered width at the given scale.
    pub fn measure_text(&mut self, text: &str, icons: &IconSet, scale: f32, font_scale: f32) -> f32 {
        let font_size = self.font_size * font_scale;
        let cell_h = self.cell_h * font_scale;
        let mut total_w = 0.0;

        let mut run = String::new();
        let mut run_is_emoji = false;

        for ch in text.chars() {
            if ch == '\x01' || ch == '\x02' { continue; }

            if IconSet::is_icon_char(ch) {
                if !run.is_empty() {
                    total_w += self.shape_width(&run, font_size, cell_h, run_is_emoji);
                    run.clear();
                }
                if let Some(pm) = icons.icon_for_char(ch) {
                    total_w += pm.width() as f32 / scale;
                }
            } else {
                let ch_emoji = is_emoji(ch);
                if !run.is_empty() && ch_emoji != run_is_emoji {
                    total_w += self.shape_width(&run, font_size, cell_h, run_is_emoji);
                    run.clear();
                }
                if run.is_empty() {
                    run_is_emoji = ch_emoji;
                }
                run.push(ch);
            }
        }
        if !run.is_empty() {
            total_w += self.shape_width(&run, font_size, cell_h, run_is_emoji);
        }

        total_w
    }

    fn shape_width(&mut self, text: &str, font_size: f32, cell_h: f32, emoji: bool) -> f32 {
        if text.is_empty() { return 0.0; }
        let scale_key = (font_size * 100.0) as i32;
        let buf = self.shaped_cache.entry((text.to_string(), scale_key, emoji)).or_insert_with(|| {
            let metrics = Metrics::new(font_size, cell_h);
            let mut buf = Buffer::new(&mut self.font_system, metrics);
            let family = if emoji {
                Family::Name(&self.emoji_font)
            } else {
                Family::Name(&self.font_family)
            };
            {
                let mut borrowed = buf.borrow_with(&mut self.font_system);
                borrowed.set_text(text, Attrs::new().family(family), Shaping::Advanced);
                borrowed.shape_until_scroll(true);
            }
            buf
        });
        buf.layout_runs().flat_map(|run| run.glyphs.iter()).map(|g| g.w).sum()
    }

    pub fn bar_height(&self, settings: &crate::config::Settings) -> u32 {
        let track = settings.resolve_track();
        let pill = settings.resolve_pill();
        (self.cell_h + 2.0 * pill.padding + 2.0 * track.padding).ceil() as u32
    }

    /// Rasterize a text run into a standalone RGBA pixmap for GPU compositing.
    pub fn rasterize_text_run(
        &mut self, text: &str,
        font_size: f32, cell_h: f32,
        fg: crate::color::Rgba, emoji: bool,
        avail_w: f32, fade_w: f32,
    ) -> (f32, Option<Pixmap>) {
        if text.is_empty() {
            return (0.0, None);
        }

        if self.shaped_cache.len() > 256 {
            self.shaped_cache.clear();
        }

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
                borrowed.set_text(text, Attrs::new().family(family), Shaping::Advanced);
                borrowed.shape_until_scroll(true);
            }
            buf
        });

        let shaped_w: f32 = buf.layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .map(|g| g.w)
            .sum();

        let pm_w = (shaped_w.ceil() as u32).max(1).min(4096);
        let pm_h = (cell_h.ceil() as u32).max(1);
        let Some(mut pixmap) = Pixmap::new(pm_w, pm_h) else {
            return (shaped_w, None);
        };

        let fg_alpha = fg.a as f32 / 255.0;
        let color = Color::rgba(fg.r, fg.g, fg.b, 255);
        let clip_right = avail_w;
        let w = pixmap.width();
        let h = pixmap.height();
        let data = pixmap.data_mut();
        buf.draw(&mut self.font_system, &mut self.swash_cache, color, |gx, gy, _w, _h, color| {
            let px_x = gx as f32;
            let clip_a = clip_alpha(px_x, clip_right, fade_w);
            let alpha = color.a() as f32 / 255.0 * fg_alpha * clip_a;
            if alpha > 0.0 {
                blend_pixel(data, w, h, gx, gy, fg.r as f32, fg.g as f32, fg.b as f32, alpha);
            }
        });

        (shaped_w, Some(pixmap))
    }
}

fn clip_alpha(px: f32, clip_right: f32, fade_w: f32) -> f32 {
    if px >= clip_right { return 0.0; }
    if fade_w > 0.0 && px >= clip_right - fade_w {
        return (clip_right - px) / fade_w;
    }
    1.0
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

pub fn is_emoji(ch: char) -> bool {
    let c = ch as u32;
    matches!(c,
        0x200D |
        0x203C | 0x2049 |
        0x20E3 |
        0x2122 | 0x2139 |
        0x2194..=0x2199 |
        0x21A9..=0x21AA |
        0x231A..=0x231B |
        0x2328 |
        0x23CF |
        0x23E9..=0x23FA |
        0x24C2 |
        0x25AA..=0x25AB |
        0x25B6 | 0x25C0 |
        0x25FB..=0x25FE |
        0x2600..=0x27BF |
        0x2934..=0x2935 |
        0x2B05..=0x2B07 |
        0x2B1B..=0x2B1C |
        0x2B50..=0x2B55 |
        0x3030 | 0x303D |
        0x3297 | 0x3299 |
        0xFE00..=0xFE0F |
        0x1F000..=0x1FAFF |
        0x1FC00..=0x1FCFF |
        0xE0020..=0xE007F
    )
}
