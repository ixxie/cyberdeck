use serde_json::{json, Value};
use std::process::Command;

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::RenderedWidget;
use crate::mods::InteractiveModule;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let powered = get_powered();
    let devices = get_devices();

    json!({
        "powered": powered,
        "devices": devices,
    })
}

fn get_powered() -> bool {
    Command::new("bluetoothctl")
        .arg("show")
        .output()
        .ok()
        .map(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            text.lines()
                .find(|l| l.contains("Powered:"))
                .map(|l| l.contains("yes"))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn get_devices() -> Vec<Value> {
    let out = match Command::new("bluetoothctl")
        .arg("devices")
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let text = String::from_utf8_lossy(&out.stdout);

    text.lines()
        .filter_map(|line| {
            // Format: "Device AA:BB:CC:DD:EE:FF Some Name"
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() < 3 {
                return None;
            }
            let mac = parts[1];
            let name = parts[2];

            let connected = Command::new("bluetoothctl")
                .args(["info", mac])
                .output()
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .contains("Connected: yes")
                })
                .unwrap_or(false);

            Some(json!({
                "mac": mac,
                "name": name,
                "connected": connected,
            }))
        })
        .collect()
}

// --- Deep module ---

pub struct BluetoothDeep {
    cursor: usize,
}

impl BluetoothDeep {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    fn device_count(data: &Value) -> usize {
        data.get("devices")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    }
}

impl InteractiveModule for BluetoothDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<RenderedWidget> {
        let highlight_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8); // active
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);     // idle

        let powered = data.get("powered").and_then(|v| v.as_bool()).unwrap_or(false);
        if !powered {
            return vec![RenderedWidget::new("off".into()).with_fg(idle_fg)];
        }

        let devices = data.get("devices").and_then(|v| v.as_array());
        let devices = match devices {
            Some(d) if !d.is_empty() => d,
            _ => return vec![RenderedWidget::new("no devices".into()).with_fg(idle_fg)],
        };

        let mut widgets = Vec::new();
        for (i, dev) in devices.iter().enumerate() {
            let name = dev.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let connected = dev.get("connected").and_then(|v| v.as_bool()).unwrap_or(false);
            let dev_fg = if i == self.cursor {
                fg
            } else if connected {
                highlight_fg
            } else {
                idle_fg
            };
            let prefix = if connected { "●" } else { "○" };
            widgets.push(RenderedWidget::new(format!("{prefix} {name}")).with_fg(dev_fg));
        }

        widgets
    }

    fn breadcrumb(&self) -> Vec<String> {
        vec!["Bluetooth".into()]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "toggle".into(), icon: None },
            KeyHintDef { key: "s".into(), action: String::new(), label: "scan".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> bool {
        let count = Self::device_count(data);

        match event.keysym {
            Keysym::Left => {
                if count > 0 {
                    self.cursor = self.cursor.checked_sub(1).unwrap_or(count - 1);
                }
                true
            }
            Keysym::Right => {
                if count > 0 {
                    self.cursor = (self.cursor + 1) % count;
                }
                true
            }
            Keysym::Return => {
                // Toggle connect/disconnect for selected device
                if let Some(devices) = data.get("devices").and_then(|v| v.as_array()) {
                    if let Some(dev) = devices.get(self.cursor) {
                        let mac = dev.get("mac").and_then(|v| v.as_str()).unwrap_or("");
                        let connected = dev.get("connected").and_then(|v| v.as_bool()).unwrap_or(false);
                        if !mac.is_empty() {
                            let action = if connected { "disconnect" } else { "connect" };
                            BarApp::spawn_command(&format!("bluetoothctl {action} {mac}"));
                        }
                    }
                }
                true
            }
            _ if event.utf8.as_deref() == Some("s") => {
                BarApp::spawn_command("bluetoothctl --timeout 10 scan on");
                true
            }
            _ => false,
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}
