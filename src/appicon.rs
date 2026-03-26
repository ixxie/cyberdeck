use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tiny_skia::Pixmap;

static CACHE: Mutex<Option<IconCache>> = Mutex::new(None);
static TARGET_H: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

struct IconCache {
    icons: HashMap<String, Option<Arc<Pixmap>>>,
}

/// Set the icon target height (call once from bar init with cell_h * scale).
pub fn set_target_height(h: u32) {
    TARGET_H.store(h, std::sync::atomic::Ordering::Relaxed);
}

fn target_height() -> u32 {
    let h = TARGET_H.load(std::sync::atomic::Ordering::Relaxed);
    if h > 0 { h } else { 32 } // fallback
}

/// Look up and process an app icon: decode, resize, grayscale, circle clip.
/// Returns a cached result. Uses the global target height set by `set_target_height`.
pub fn lookup(name: &str) -> Option<Arc<Pixmap>> {
    if name.is_empty() {
        return None;
    }

    let th = target_height();
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(|| IconCache {
        icons: HashMap::new(),
    });

    if let Some(cached) = cache.icons.get(name) {
        return cached.clone();
    }

    let result = load_and_process(name, th).map(Arc::new);
    cache.icons.insert(name.to_string(), result.clone());
    result
}

fn load_and_process(name: &str, target_h: u32) -> Option<Pixmap> {
    let path = find_icon(name);
    log::info!("appicon lookup '{}': {:?}", name, path);
    let path = path?;

    // Match Phosphor icon sizing: content at 55% of cell height, centered
    let content_h = (target_h as f32 * 0.55).round() as u32;
    let raw = load_image(&path, content_h);
    if raw.is_none() {
        log::warn!("appicon failed to load: {}", path.display());
        return None;
    }
    let raw = raw?;
    let clipped = grayscale_circle(raw);

    // Center in a target_h × content_h pixmap (same aspect as Phosphor icons)
    let out_w = clipped.width();
    let mut out = Pixmap::new(out_w, target_h)?;
    let pad_y = ((target_h - content_h) / 2) as i32;
    let src = clipped.data();
    let dst = out.data_mut();
    for y in 0..content_h {
        for x in 0..out_w {
            let si = (y * out_w + x) as usize * 4;
            let dy = y as i32 + pad_y;
            if dy >= 0 && (dy as u32) < target_h {
                let di = (dy as u32 * out_w + x) as usize * 4;
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
    log::info!("appicon loaded {}x{} (content {}px)", out_w, target_h, content_h);
    Some(out)
}

fn find_icon(name: &str) -> Option<PathBuf> {
    // file:// URI
    if let Some(path) = name.strip_prefix("file://") {
        let decoded = urldecode(path);
        let p = Path::new(&decoded);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }

    // Absolute path
    let p = Path::new(name);
    if p.is_absolute() && p.exists() {
        return Some(p.to_path_buf());
    }

    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/share:/usr/local/share".to_string());

    // Search hicolor theme at common sizes
    for dir in data_dirs.split(':') {
        let base = Path::new(dir).join("icons/hicolor");
        for size in &[48, 64, 128, 256, 32, 24, 96, 512] {
            let sized = base.join(format!("{size}x{size}/apps/{name}.png"));
            if sized.exists() {
                return Some(sized);
            }
        }
        // Scalable SVG
        let svg = base.join(format!("scalable/apps/{name}.svg"));
        if svg.exists() {
            return Some(svg);
        }
    }

    // Pixmaps fallback
    for dir in data_dirs.split(':') {
        let pixmap = Path::new(dir).join("pixmaps").join(format!("{name}.png"));
        if pixmap.exists() {
            return Some(pixmap);
        }
    }

    None
}

fn load_image(path: &Path, target_h: u32) -> Option<Pixmap> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "svg" => load_svg(path, target_h),
        _ => load_raster(path, target_h),
    }
}

fn load_raster(path: &Path, target_h: u32) -> Option<Pixmap> {
    let img = image::open(path).ok()?;

    // Crop to content bounds (strip transparent padding)
    let rgba_full = img.to_rgba8();
    let (cw, ch) = (rgba_full.width(), rgba_full.height());
    let mut min_x = cw;
    let mut min_y = ch;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for (x, y, px) in rgba_full.enumerate_pixels() {
        if px[3] > 0 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    let img = if max_x >= min_x && max_y >= min_y {
        image::DynamicImage::ImageRgba8(image::imageops::crop_imm(
            &rgba_full, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1,
        ).to_image())
    } else {
        img
    };

    let img = img.resize_exact(target_h, target_h, image::imageops::FilterType::Lanczos3);
    let rgba = img.to_rgba8();

    let mut pixmap = Pixmap::new(target_h, target_h)?;
    let data = pixmap.data_mut();
    for (i, pixel) in rgba.pixels().enumerate() {
        let idx = i * 4;
        if idx + 3 < data.len() {
            data[idx] = pixel[0];
            data[idx + 1] = pixel[1];
            data[idx + 2] = pixel[2];
            data[idx + 3] = pixel[3];
        }
    }
    Some(pixmap)
}

fn load_svg(path: &Path, target_h: u32) -> Option<Pixmap> {
    let data = std::fs::read(path).ok()?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &opt).ok()?;

    let size = tree.size();
    let scale = target_h as f32 / size.height();
    let w = (size.width() * scale).ceil() as u32;

    let mut pixmap = Pixmap::new(w.max(1), target_h.max(1))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(pixmap)
}

/// Convert to grayscale alpha mask and clip to circle.
fn grayscale_circle(mut pixmap: Pixmap) -> Pixmap {
    let w = pixmap.width();
    let h = pixmap.height();
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let r = cx.min(cy);
    let r2 = r * r;

    let data = pixmap.data_mut();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize * 4;
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist2 = dx * dx + dy * dy;

            if dist2 > r2 {
                // Outside circle
                data[idx + 3] = 0;
            } else {
                // Convert to luminance alpha mask
                let r_val = data[idx] as f32;
                let g_val = data[idx + 1] as f32;
                let b_val = data[idx + 2] as f32;
                let orig_a = data[idx + 3] as f32 / 255.0;
                let lum = 0.299 * r_val + 0.587 * g_val + 0.114 * b_val;
                let alpha = (lum * orig_a).round().min(255.0) as u8;

                // Anti-alias circle edge
                let edge_dist = r - dist2.sqrt();
                let edge_alpha = if edge_dist < 1.0 {
                    edge_dist.max(0.0)
                } else {
                    1.0
                };

                data[idx] = 255;
                data[idx + 1] = 255;
                data[idx + 2] = 255;
                data[idx + 3] = (alpha as f32 * edge_alpha).round() as u8;
            }
        }
    }
    pixmap
}

fn urldecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().and_then(|c| (c as char).to_digit(16));
            let lo = chars.next().and_then(|c| (c as char).to_digit(16));
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8 as char);
            }
        } else {
            out.push(b as char);
        }
    }
    out
}
