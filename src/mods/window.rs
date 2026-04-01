use serde_json::{json, Value};
use std::io::Read;
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

        let mut stdout = child.stdout.take().unwrap();
        let mut buf = [0u8; 4096];
        let mut partial = String::new();

        loop {
            let n = match stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            partial.push_str(&String::from_utf8_lossy(&buf[..n]));

            while let Some(pos) = partial.find('\n') {
                let line = partial[..pos].to_string();
                partial = partial[pos + 1..].to_string();

                if line.contains("Window") {
                    let val = poll(&params);
                    if sender.send((id.clone(), val)).is_err() {
                        let _ = child.kill();
                        return;
                    }
                }
            }
        }

        let _ = child.wait();
        log::warn!("niri event-stream exited (window), restarting...");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

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
