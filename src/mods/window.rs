use serde_json::{json, Value};
use std::process::Command;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let out = Command::new("niri")
        .args(["msg", "--json", "focused-window"])
        .output();

    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
                let title = v.get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let app_id = v.get("app_id")
                    .and_then(|a| a.as_str())
                    .unwrap_or("");
                return json!({ "title": title, "app_id": app_id });
            }
            defaults()
        }
        _ => defaults(),
    }
}

fn defaults() -> Value {
    json!({ "title": "", "app_id": "" })
}
