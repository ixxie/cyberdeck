use serde_json::{json, Value};
use std::process::Command;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let out = Command::new("playerctl")
        .args([
            "metadata",
            "--format",
            r#"{"player":"{{playerName}}","title":"{{title}}","artist":"{{artist}}","album":"{{album}}","status":"{{status}}","position_us":{{position}}}"#,
        ])
        .output();

    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                    return v;
                }
            }
            defaults()
        }
        _ => defaults(),
    }
}

fn defaults() -> Value {
    json!({
        "player": "",
        "title": "",
        "artist": "",
        "album": "",
        "status": "Stopped",
        "position_us": 0,
    })
}
