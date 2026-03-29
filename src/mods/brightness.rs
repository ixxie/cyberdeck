use serde_json::{json, Value};
use std::fs;
use std::path::Path;

use smithay_client_toolkit::reexports::calloop::channel::Sender;

pub fn subscribe(
    params: serde_json::Map<String, Value>,
    sender: Sender<(String, Value)>,
    id: String,
) {
    // Fast poll loop: check every 100ms, only send on change
    let mut last = u64::MAX;
    loop {
        let val = poll(&params);
        let cur = val.get("raw").and_then(|v| v.as_u64()).unwrap_or(0);
        if cur != last {
            last = cur;
            if sender.send((id.clone(), val)).is_err() {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

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
