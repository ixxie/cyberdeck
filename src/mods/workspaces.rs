use serde_json::{json, Value};
use std::io::BufRead;
use std::process::Command;

use smithay_client_toolkit::reexports::calloop::channel::Sender;

pub fn subscribe(
    params: serde_json::Map<String, Value>,
    sender: Sender<(String, Value)>,
    id: String,
) {
    loop {
        let child = Command::new("niri")
            .args(["msg", "--json", "event-stream"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();

        let Ok(mut child) = child else {
            log::error!("failed to spawn niri event-stream");
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        };

        let stdout = child.stdout.take().unwrap();
        let reader = std::io::BufReader::new(stdout);

        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.contains("WorkspacesChanged") || line.contains("WindowsChanged") {
                let val = poll(&params);
                if sender.send((id.clone(), val)).is_err() {
                    let _ = child.kill();
                    return;
                }
            }
        }

        let _ = child.wait();
        log::warn!("niri event-stream exited, restarting...");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let ws = get_workspaces();
    let wins = get_windows();
    json!({ "workspaces": ws, "windows": wins })
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
            let focused = ws.get("is_focused").and_then(|v| v.as_bool()).unwrap_or(false);
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
                "focused": focused,
                "output": output,
                "active_window": active_window,
            })
        })
        .collect();

    result.sort_by_key(|w| w.get("idx").and_then(|v| v.as_i64()).unwrap_or(0));
    result
}

fn get_windows() -> Vec<Value> {
    let out = Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output();

    let Ok(out) = out else { return vec![] };
    if !out.status.success() {
        return vec![];
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let Ok(win_list) = serde_json::from_str::<Vec<Value>>(text.trim()) else {
        return vec![];
    };

    win_list.iter().map(|w| {
        let ws_id = w.get("workspace_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let focused = w.get("is_focused").and_then(|v| v.as_bool()).unwrap_or(false);
        let floating = w.get("is_floating").and_then(|v| v.as_bool()).unwrap_or(false);

        let layout = w.get("layout");
        let tile_w = layout.and_then(|l| l.get("tile_size"))
            .and_then(|s| s.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_f64())
            .unwrap_or(400.0);
        let tile_h = layout.and_then(|l| l.get("tile_size"))
            .and_then(|s| s.as_array())
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_f64())
            .unwrap_or(300.0);
        let col = layout.and_then(|l| l.get("pos_in_scrolling_layout"))
            .and_then(|p| p.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_i64())
            .unwrap_or(1);
        let row = layout.and_then(|l| l.get("pos_in_scrolling_layout"))
            .and_then(|p| p.as_array())
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_i64())
            .unwrap_or(1);

        json!({
            "workspace_id": ws_id,
            "focused": focused,
            "floating": floating,
            "col": col,
            "row": row,
            "w": tile_w,
            "h": tile_h,
        })
    }).collect()
}

fn fallback() -> Vec<Value> {
    vec![json!({"id": 1, "idx": 1, "name": "1", "active": true, "focused": false, "output": "", "active_window": null})]
}
