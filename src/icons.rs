use std::collections::HashMap;
use std::path::Path;

use tiny_skia::Pixmap;

const PUA_START: u32 = 0xE000;
const PUA_END: u32 = 0xF8FF;

/// Discover all available icon names from the SVG directory.
/// Assigns a stable PUA codepoint to each name via hashing.
/// Returns a map of icon_name → PUA char string.
pub fn discover(icons_dir: Option<&str>) -> HashMap<String, String> {
    let Some(dir) = icons_dir else { return HashMap::new() };

    let mut names: Vec<String> = Vec::new();
    // Scan all weight subdirectories for SVG files
    let base = Path::new(dir);
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() { continue; }
            if let Ok(files) = std::fs::read_dir(entry.path()) {
                for file in files.flatten() {
                    let fname = file.file_name();
                    let fname = fname.to_string_lossy();
                    if !fname.ends_with(".svg") { continue; }
                    // Strip weight suffix to get canonical name
                    let stem = fname.trim_end_matches(".svg");
                    let canonical = strip_weight_suffix(stem);
                    if !names.contains(&canonical) {
                        names.push(canonical);
                    }
                }
            }
        }
    }
    names.sort();

    let pua_range = PUA_END - PUA_START;
    names.iter().filter_map(|name| {
        let cp = PUA_START + (hash_name(name) % pua_range);
        char::from_u32(cp).map(|ch| (name.clone(), ch.to_string()))
    }).collect()
}

fn strip_weight_suffix(name: &str) -> String {
    let suffixes = ["-fill", "-bold", "-thin", "-light", "-duotone", "-regular"];
    for s in &suffixes {
        if let Some(base) = name.strip_suffix(s) {
            return base.to_string();
        }
    }
    name.to_string()
}

fn hash_name(name: &str) -> u32 {
    // FNV-1a 32-bit
    let mut h: u32 = 0x811c9dc5;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

pub struct IconSet {
    char_to_name: HashMap<char, String>,
    rendered: HashMap<String, Pixmap>,
}

impl IconSet {
    pub fn load(
        icons_dir: Option<&str>,
        weight: &str,
        cell_h: f32,
        icon_map: &HashMap<String, String>,
    ) -> Self {
        let mut char_to_name = HashMap::new();
        for (name, ch_str) in icon_map {
            if let Some(ch) = ch_str.chars().next() {
                char_to_name.insert(ch, name.clone());
            }
        }

        let mut rendered = HashMap::new();
        if let Some(dir) = icons_dir {
            let target = cell_h.ceil() as u32;
            for name in char_to_name.values() {
                if let Some(pixmap) = Self::load_icon(dir, weight, name, target) {
                    rendered.insert(name.clone(), pixmap);
                }
            }
            log::info!("icons: loaded {}/{} SVGs from {} (weight={})", rendered.len(), char_to_name.len(), dir, weight);
        }

        Self { char_to_name, rendered }
    }

    fn load_icon(icons_dir: &str, default_weight: &str, name: &str, target_size: u32) -> Option<Pixmap> {
        let svg_path = Self::resolve_path(icons_dir, default_weight, name);
        let data = std::fs::read(&svg_path)
            .map_err(|e| log::debug!("icon {name}: {e}"))
            .ok()?;

        let opt = resvg::usvg::Options::default();
        let tree = resvg::usvg::Tree::from_data(&data, &opt)
            .map_err(|e| log::warn!("SVG parse error {}: {e}", svg_path.display()))
            .ok()?;

        let viewbox = tree.size();
        let pre_scale = 4.0;
        let pre_w = (viewbox.width() * pre_scale).ceil() as u32;
        let pre_h = (viewbox.height() * pre_scale).ceil() as u32;
        let mut pre_pm = Pixmap::new(pre_w.max(1), pre_h.max(1))?;
        resvg::render(&tree, tiny_skia::Transform::from_scale(pre_scale, pre_scale), &mut pre_pm.as_mut());

        let (cx, cy, cw, ch) = Self::content_bounds(&pre_pm);
        if cw == 0 || ch == 0 {
            return None;
        }

        let icon_h = (target_size as f32 * 0.55).round();
        let aspect = cw as f32 / ch as f32;
        let out_h = target_size;
        let out_w = (icon_h * aspect).ceil() as u32;

        let final_scale = icon_h / (ch as f32 / pre_scale);
        let pad_y = (out_h as f32 - icon_h) / 2.0;
        let tx = -(cx as f32 / pre_scale) * final_scale;
        let ty = -(cy as f32 / pre_scale) * final_scale + pad_y;

        let mut pixmap = Pixmap::new(out_w.max(1), out_h.max(1))?;
        let transform = tiny_skia::Transform::from_scale(final_scale, final_scale)
            .pre_translate(tx / final_scale, ty / final_scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        Some(pixmap)
    }

    fn content_bounds(pm: &Pixmap) -> (u32, u32, u32, u32) {
        let w = pm.width();
        let h = pm.height();
        let data = pm.data();
        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0u32;
        let mut max_y = 0u32;

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize * 4;
                if data[idx + 3] > 0 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }

        if max_x < min_x || max_y < min_y {
            return (0, 0, 0, 0);
        }
        (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
    }

    fn resolve_path(icons_dir: &str, default_weight: &str, name: &str) -> std::path::PathBuf {
        let weight_suffixes = ["-fill", "-bold", "-thin", "-light", "-duotone"];
        for suffix in &weight_suffixes {
            if name.ends_with(suffix) {
                let dir_name = &suffix[1..];
                return Path::new(icons_dir).join(dir_name).join(format!("{name}.svg"));
            }
        }
        if default_weight == "regular" {
            Path::new(icons_dir).join("regular").join(format!("{name}.svg"))
        } else {
            Path::new(icons_dir).join(default_weight).join(format!("{name}-{default_weight}.svg"))
        }
    }

    pub fn is_icon_char(ch: char) -> bool {
        let cp = ch as u32;
        cp >= PUA_START && cp <= PUA_END
    }

    pub fn icon_for_char(&self, ch: char) -> Option<&Pixmap> {
        let name = self.char_to_name.get(&ch)?;
        self.rendered.get(name)
    }
}
