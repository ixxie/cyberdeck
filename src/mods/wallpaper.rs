use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif",
];

fn state_path() -> PathBuf {
    let cache = std::env::var("XDG_CACHE_HOME")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.cache")
        });
    PathBuf::from(cache).join("cyberdeck/wallpaper/state.json")
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix('~') {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}{rest}")
    } else {
        s.to_string()
    }
}

fn get_param<'a>(params: &'a serde_json::Map<String, Value>, key: &str, default: &'a str) -> &'a str {
    params.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
}

fn wallpaper_dir(params: &serde_json::Map<String, Value>) -> String {
    expand_tilde(get_param(params, "dir", "~/Pictures/Wallpapers"))
}

fn read_state() -> Value {
    let path = state_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_state(current: &str, group: &str) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let state = json!({"current": current, "group": group});
    let _ = fs::write(&path, state.to_string());
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.iter().any(|ext| e.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn find_images(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_images(dir, recursive, &mut result);
    result.sort();
    result
}

fn collect_images(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_image(&path) {
            out.push(path);
        } else if recursive && path.is_dir() {
            collect_images(&path, true, out);
        }
    }
}

fn list_groups(dir: &Path) -> Vec<String> {
    let mut groups = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return groups,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                groups.push(name.to_string());
            }
        }
    }
    groups.sort();
    groups
}

fn pseudo_random(n: usize) -> usize {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    (nanos as usize) % n
}

pub fn poll(params: &serde_json::Map<String, Value>) -> Value {
    let dir = wallpaper_dir(params);
    let dir_path = Path::new(&dir);
    let state = read_state();
    let current = state.get("current").and_then(|v| v.as_str()).unwrap_or("");
    let group = state.get("group").and_then(|v| v.as_str()).unwrap_or("");

    let mut entries = vec![json!({"name": "all", "exec": "cyberdeck wallpaper shuffle"})];
    for g in list_groups(dir_path) {
        entries.push(json!({"name": g, "exec": format!("cyberdeck wallpaper shuffle {g}")}));
    }

    json!({
        "current": current,
        "group": group,
        "entries": entries,
    })
}

pub fn shuffle(params: &serde_json::Map<String, Value>, group: Option<&str>) {
    let dir = wallpaper_dir(params);
    let dir_path = Path::new(&dir);
    let fill = get_param(params, "fill", "crop");
    let transition = get_param(params, "transition", "fade");
    let duration = get_param(params, "transition-duration", "1");

    // Reuse last group from state if none specified
    let state = read_state();
    let cached_group = state.get("group").and_then(|v| v.as_str()).unwrap_or("");
    let effective = group.unwrap_or(cached_group);

    let (search_dir, group_name) = if !effective.is_empty() {
        let gdir = dir_path.join(effective);
        if gdir.is_dir() {
            (gdir, effective.to_string())
        } else {
            (dir_path.to_path_buf(), String::new())
        }
    } else {
        (dir_path.to_path_buf(), String::new())
    };

    let recursive = group_name.is_empty();
    let images = find_images(&search_dir, recursive);
    if images.is_empty() {
        eprintln!("no images found in {}", search_dir.display());
        return;
    }

    let idx = pseudo_random(images.len());
    let picked = &images[idx];
    let relative = picked.strip_prefix(dir_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| picked.to_string_lossy().to_string());

    let status = Command::new("awww")
        .args([
            "img",
            &picked.to_string_lossy(),
            "--resize", fill,
            "--transition-type", transition,
            "--transition-duration", duration,
        ])
        .status();

    if let Err(e) = status {
        eprintln!("failed to run swww: {e}");
        return;
    }

    write_state(&relative, &group_name);
}

pub fn init(params: &serde_json::Map<String, Value>) {
    let dir = wallpaper_dir(params);
    let fill = get_param(params, "fill", "crop");
    let transition = get_param(params, "transition", "fade");
    let duration = get_param(params, "transition-duration", "1");

    let state = read_state();
    let current = match state.get("current").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => return,
    };

    let full_path = Path::new(&dir).join(&current);
    if !full_path.is_file() {
        return;
    }

    let _ = Command::new("awww")
        .args([
            "img",
            &full_path.to_string_lossy(),
            "--resize", fill,
            "--transition-type", transition,
            "--transition-duration", duration,
        ])
        .status();
}
