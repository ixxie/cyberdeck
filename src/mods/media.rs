use serde_json::{json, Value};
use std::io::BufRead;
use std::process::Command;

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

pub fn subscribe(
    _params: serde_json::Map<String, Value>,
    sender: Sender<(String, Value)>,
    id: String,
) {
    let fmt = r#"{"player":"{{playerName}}","title":"{{title}}","artist":"{{artist}}","album":"{{album}}","status":"{{status}}"}"#;

    loop {
        let child = Command::new("playerctl")
            .args(["metadata", "--follow", "--format", fmt])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();

        let Ok(mut child) = child else {
            log::error!("failed to spawn playerctl --follow");
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        };

        let mut stdout = child.stdout.take().unwrap();
        let mut buf = [0u8; 4096];
        let mut partial = String::new();

        loop {
            let n = match std::io::Read::read(&mut stdout, &mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            partial.push_str(&String::from_utf8_lossy(&buf[..n]));

            while let Some(pos) = partial.find('\n') {
                let line = partial[..pos].trim().to_string();
                partial = partial[pos + 1..].to_string();

                if line.is_empty() { continue; }
                let val = serde_json::from_str::<Value>(&line)
                    .unwrap_or_else(|_| defaults());
                if sender.send((id.clone(), val)).is_err() {
                    let _ = child.kill();
                    return;
                }
            }
        }

        let _ = child.wait();
        log::warn!("playerctl --follow exited, restarting...");
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let out = Command::new("playerctl")
        .args([
            "metadata",
            "--format",
            r#"{"player":"{{playerName}}","title":"{{title}}","artist":"{{artist}}","album":"{{album}}","status":"{{status}}"}"#,
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
    })
}

// --- Deep module ---

pub struct MediaDeep;

impl MediaDeep {
    pub fn new() -> Self {
        Self
    }
}

impl InteractiveModule for MediaDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let artist = data.get("artist").and_then(|v| v.as_str()).unwrap_or("");
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("Stopped");

        if title.is_empty() {
            return vec![vec![Elem::text("no media").fg(idle_fg)]];
        }

        let icon = if status == "Playing" { "▶" } else { "⏸" };
        let label = if !artist.is_empty() {
            format!("{icon} {artist} — {title}")
        } else {
            format!("{icon} {title}")
        };

        vec![vec![Elem::text(label).fg(fg)]]
    }

    fn breadcrumb(&self) -> Vec<String> {
        vec![]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "p".into(), action: String::new(), label: "play/pause".into(), icon: None },
            KeyHintDef { key: "[".into(), action: String::new(), label: "prev".into(), icon: None },
            KeyHintDef { key: "]".into(), action: String::new(), label: "next".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, _data: &Value) -> KeyResult {
        match event.keysym {
            _ if event.utf8.as_deref() == Some("p") => {
                BarApp::spawn_command("playerctl play-pause");
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("[") => {
                BarApp::spawn_command("playerctl previous");
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("]") => {
                BarApp::spawn_command("playerctl next");
                KeyResult::Action
            }
            Keysym::Left => {
                BarApp::spawn_command("playerctl previous");
                KeyResult::Action
            }
            Keysym::Right => {
                BarApp::spawn_command("playerctl next");
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn reset(&mut self) {}
}
