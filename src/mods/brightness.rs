use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let bl_dir = Path::new("/sys/class/backlight");
    let (raw, max) = fs::read_dir(bl_dir)
        .ok()
        .and_then(|mut entries| entries.next())
        .and_then(|e| e.ok())
        .map(|entry| {
            let p = entry.path();
            let raw = read_int(&p.join("brightness")).unwrap_or(100);
            let max = read_int(&p.join("max_brightness")).unwrap_or(100);
            (raw, max)
        })
        .unwrap_or((100, 100));

    let percent = if max > 0 { raw * 100 / max } else { 100 };

    json!({
        "brightness": percent,
        "raw": raw,
        "max": max,
    })
}

fn read_int(path: &Path) -> Option<u64> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse()
        .ok()
}
