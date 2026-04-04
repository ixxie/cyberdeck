use vello::Scene;
use vello::kurbo::{Affine, RoundedRect};
use vello::peniko::Fill;

use crate::color::Rgba;
use crate::layout::Frame;
use crate::icons::IconSet;
use crate::render::Renderer;

fn rgba_brush(c: Rgba, opacity: f32) -> vello::peniko::Brush {
    let a = c.a as f32 / 255.0 * opacity;
    // Pass sRGB values directly — matching old renderer's sRGB blending behavior
    vello::peniko::Brush::Solid(vello::peniko::color::AlphaColor::new(
        [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, a],
    ))
}

/// Build a vello Scene from a computed Frame.
pub fn build_scene(
    scene: &mut Scene,
    frame: &Frame,
    renderer: &mut Renderer,
    icons: &IconSet,
    bg: Rgba,
    track_radius: f32,
    bar_w: u32,
    bar_h: u32,
    scale: i32,
    output_mul: f32,
) -> vello::peniko::color::AlphaColor<vello::peniko::color::Srgb> {
    scene.reset();
    let sf = scale as f64;
    let w = bar_w as f64 * sf;
    let h = bar_h as f64 * sf;

    // When track has rounded corners, clear to transparent and draw a rounded rect.
    // Otherwise use bg as base_color (avoids anti-aliasing fringe at pill edges).
    let base = if track_radius > 0.0 {
        // Rounded solid track: clear to transparent, draw rounded rect
        let r = track_radius as f64 * output_mul as f64 * sf;
        let rrect = RoundedRect::new(0.0, 0.0, w, h, r);
        let brush = rgba_brush(bg, 1.0);
        scene.fill(Fill::NonZero, Affine::IDENTITY, &brush, None, &rrect);
        vello::peniko::color::AlphaColor::new([0.0, 0.0, 0.0, 0.0])
    } else {
        // Flat solid track: use as clear color
        vello::peniko::color::AlphaColor::new(
            [bg.r as f32 / 255.0, bg.g as f32 / 255.0, bg.b as f32 / 255.0, bg.a as f32 / 255.0],
        )
    };

    for fspan in &frame.spans {
        let sx = fspan.rect.x as f64 * sf;
        let sy = fspan.rect.y as f64 * sf;
        let sw = fspan.rect.w as f64 * sf;
        let sh = fspan.rect.h as f64 * sf;
        let opacity = fspan.opacity;

        if opacity <= 0.0 {
            continue;
        }

        // Rounded-rect background
        if let Some(sbg) = fspan.bg {
            let bg_a = sbg.a as f32 * opacity / 255.0;
            if bg_a > 0.0 {
                let radius = fspan.radius as f64 * sf;
                let rrect = RoundedRect::new(sx, sy, sx + sw, sy + sh, radius);
                let brush = rgba_brush(sbg, opacity);
                scene.fill(Fill::NonZero, Affine::IDENTITY, &brush, None, &rrect);
            }
        }

        // Text and icons
        let cell_w = renderer.cell_w as f64 * sf * output_mul as f64;
        let cell_h = renderer.cell_h as f64 * sf * output_mul as f64;
        let text_y = sy + (sh - cell_h) / 2.0;
        let clip_right = (fspan.rect.x + fspan.rect.w) as f64 * sf;
        let content_overflows = (fspan.content_w as f64 * sf) > sw + 0.5;
        let fade_w = if content_overflows { cell_w * 2.0 } else { 0.0 };

        for felem in &fspan.elems {
            let elem_x = felem.rect.x as f64 * sf;
            let elem_y = text_y + felem.y_offset as f64 * sf * output_mul as f64;
            let font_size_f = renderer.font_size * scale as f32 * output_mul * felem.font_scale;
            let elem_cell_h = cell_h * felem.font_scale as f64;

            let mut fg = felem.fg;
            fg.a = (fg.a as f32 * opacity) as u8;
            if fg.a == 0 {
                continue;
            }

            let mut cx = elem_x;

            // Inline icon (icons are alpha masks — tint with fg color)
            if let Some(ref icon_pm) = felem.icon {
                let icon_w = icon_pm.width() as f64;
                let icon_h = icon_pm.height() as f64;
                let dy = elem_y + (cell_h - icon_h) / 2.0;
                let img = tinted_icon(icon_pm, fg);
                scene.draw_image(&img, Affine::translate((cx, dy)));
                let icon_gap = if felem.font_scale < 1.0 { cell_w * 0.15 } else { cell_w };
                cx += icon_w + icon_gap;
            }

            // Text: split into icon chars and text runs
            let mut dim = false;
            let mut run = String::new();
            let mut run_start = cx;
            let mut run_fg = fg;
            let mut run_is_emoji = false;

            for ch in felem.text.chars() {
                if ch == '\x01' { dim = true; continue; }
                if ch == '\x02' { dim = false; continue; }

                let mut char_fg = fg;
                if dim {
                    char_fg.a = (char_fg.a as f32 * 0.615) as u8;
                }

                if IconSet::is_icon_char(ch) {
                    // Flush text run
                    if !run.is_empty() {
                        let run_w = render_text_run(
                            scene, renderer, &run, run_start, elem_y,
                            font_size_f, elem_cell_h as f32, run_fg, run_is_emoji,
                            clip_right as f32, fade_w as f32,
                        );
                        cx = run_start + run_w as f64;
                        run.clear();
                    }
                    // Draw icon
                    if let Some(icon_pm) = icons.icon_for_char(ch) {
                        let icon_w = icon_pm.width() as f64;
                        let icon_h = icon_pm.height() as f64;
                        let dy = elem_y + (elem_cell_h - icon_h) / 2.0;
                        let img = tinted_icon(icon_pm, char_fg);
                        scene.draw_image(&img, Affine::translate((cx, dy)));
                        cx += icon_w;
                    } else {
                        cx += cell_w;
                    }
                    run_start = cx;
                    run_fg = char_fg;
                    run_is_emoji = false;
                } else {
                    let ch_emoji = crate::render::is_emoji(ch);
                    let needs_flush = if run.is_empty() {
                        false
                    } else {
                        char_fg != run_fg || ch_emoji != run_is_emoji
                    };

                    if needs_flush {
                        let run_w = render_text_run(
                            scene, renderer, &run, run_start, elem_y,
                            font_size_f, elem_cell_h as f32, run_fg, run_is_emoji,
                            clip_right as f32, fade_w as f32,
                        );
                        cx = run_start + run_w as f64;
                        run.clear();
                    }

                    if run.is_empty() {
                        run_start = cx;
                        run_fg = char_fg;
                        run_is_emoji = ch_emoji;
                    }
                    run.push(ch);
                    cx += cell_w * felem.font_scale as f64
                        * unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as f64;
                }
            }
            // Flush remaining
            if !run.is_empty() {
                render_text_run(
                    scene, renderer, &run, run_start, elem_y,
                    font_size_f, elem_cell_h as f32, run_fg, run_is_emoji,
                    clip_right as f32, fade_w as f32,
                );
            }
        }
    }
    base
}

/// Render a shaped text run into a temporary RGBA buffer and draw as a vello image.
/// Returns the shaped width.
fn render_text_run(
    scene: &mut Scene,
    renderer: &mut Renderer,
    text: &str,
    x: f64, y: f64,
    font_size: f32, cell_h: f32,
    fg: Rgba, emoji: bool,
    clip_right: f32, fade_w: f32,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }

    // Use the existing renderer to rasterize the text run into a small pixmap
    let (shaped_w, pixmap) = renderer.rasterize_text_run(
        text, font_size, cell_h, fg, emoji, clip_right - x as f32, fade_w,
    );

    if let Some(pm) = pixmap {
        if pm.width() > 0 && pm.height() > 0 {
            let img = pixmap_to_image(&pm);
            scene.draw_image(&img, Affine::translate((x, y)));
        }
    }

    shaped_w
}

/// Create a tinted icon image: replace RGB with fg color (linear), keep alpha.
fn tinted_icon(pm: &tiny_skia::Pixmap, fg: Rgba) -> vello::peniko::ImageBrush {
    let w = pm.width();
    let h = pm.height();
    let src = pm.data();
    let mut data = vec![0u8; (w * h * 4) as usize];
    for i in (0..data.len()).step_by(4) {
        let a = src[i + 3] as f32 / 255.0;
        let fg_a = fg.a as f32 / 255.0;
        let alpha = (a * fg_a * 255.0) as u8;
        data[i] = fg.r;
        data[i + 1] = fg.g;
        data[i + 2] = fg.b;
        data[i + 3] = alpha;
    }
    let image_data = vello::peniko::ImageData {
        data: vello::peniko::Blob::new(std::sync::Arc::new(data)),
        format: vello::peniko::ImageFormat::Rgba8,
        alpha_type: vello::peniko::ImageAlphaType::Alpha,
        width: w,
        height: h,
    };
    vello::peniko::ImageBrush::new(image_data)
}

/// Convert a tiny_skia::Pixmap to a vello ImageBrush.
fn pixmap_to_image(pm: &tiny_skia::Pixmap) -> vello::peniko::ImageBrush {
    let w = pm.width();
    let h = pm.height();
    let data = pm.data().to_vec();
    let image_data = vello::peniko::ImageData {
        data: vello::peniko::Blob::new(std::sync::Arc::new(data)),
        format: vello::peniko::ImageFormat::Rgba8,
        alpha_type: vello::peniko::ImageAlphaType::Alpha,
        width: w,
        height: h,
    };
    vello::peniko::ImageBrush::new(image_data)
}

