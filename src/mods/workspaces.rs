use serde_json::{json, Value};
use std::process::Command;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let ws = get_workspaces();
    json!({ "workspaces": ws })
}

fn get_workspaces() -> Vec<Value> {
    let out = Command::new("niri")
        .args(["msg", "--json", "workspaces"])
        .output();

    let Ok(out) = out else { return fallback() };
    if !out.status.success() {
        return fallback();
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let Ok(ws_list) = serde_json::from_str::<Vec<Value>>(text.trim()) else {
        return fallback();
    };

    let mut result: Vec<Value> = ws_list
        .iter()
        .map(|ws| {
            let id = ws.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let idx = ws.get("idx").and_then(|v| v.as_i64()).unwrap_or(id);
            let name = ws.get("name")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| idx.to_string());
            let active = ws.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
            let output = ws.get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let active_window = ws.get("active_window_id")
                .cloned()
                .unwrap_or(Value::Null);

            json!({
                "id": id,
                "idx": idx,
                "name": name,
                "active": active,
                "output": output,
                "active_window": active_window,
            })
        })
        .collect();

    result.sort_by_key(|w| w.get("idx").and_then(|v| v.as_i64()).unwrap_or(0));
    result
}

fn fallback() -> Vec<Value> {
    vec![json!({"id": 1, "idx": 1, "name": "1", "active": true, "output": "", "active_window": null})]
}
