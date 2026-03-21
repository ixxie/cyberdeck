use serde_json::{json, Value};
use std::process::Command;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let count = get_count();
    let dnd = get_dnd();

    json!({
        "count": count,
        "dnd": dnd,
        "latest": null,
    })
}

fn get_count() -> i64 {
    let out = Command::new("swaync-client").arg("-c").output();
    let Ok(out) = out else { return 0 };
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim().parse().unwrap_or(0)
}

fn get_dnd() -> bool {
    let out = Command::new("swaync-client").arg("-D").output();
    let Ok(out) = out else { return false };
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim() == "true"
}
