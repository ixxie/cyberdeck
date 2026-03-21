use serde_json::{json, Value};
use std::process::Command;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let (volume, muted) = get_volume();
    let sink = get_sink_name();

    json!({
        "volume": volume,
        "muted": muted,
        "sink": sink,
    })
}

fn get_volume() -> (i64, bool) {
    let out = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output();

    let Ok(out) = out else { return (0, false) };
    let text = String::from_utf8_lossy(&out.stdout);

    // Format: "Volume: 0.72" or "Volume: 0.72 [MUTED]"
    let muted = text.contains("MUTED");
    let volume = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| (v * 100.0) as i64)
        .unwrap_or(0);

    (volume, muted)
}

fn get_sink_name() -> String {
    let out = Command::new("wpctl")
        .args(["inspect", "@DEFAULT_AUDIO_SINK@"])
        .output();

    let Ok(out) = out else { return "unknown".into() };
    let text = String::from_utf8_lossy(&out.stdout);

    text.lines()
        .find(|l| l.contains("node.description"))
        .and_then(|l| {
            let start = l.find('"')? + 1;
            let end = l.rfind('"')?;
            if start < end {
                Some(l[start..end].to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".into())
}
