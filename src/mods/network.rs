use serde_json::{json, Value};
use std::process::Command;

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let disconnected = json!({
        "connected": false,
        "type": "",
        "ssid": "",
        "signal": 0,
        "ip": "",
        "networks": [],
    });

    let Ok(dev_out) = Command::new("nmcli")
        .args(["-t", "-f", "TYPE,STATE,CONNECTION", "device"])
        .output()
    else {
        return disconnected;
    };
    let dev_str = String::from_utf8_lossy(&dev_out.stdout);

    let networks = get_networks();

    // Check wifi first
    let wifi = dev_str
        .lines()
        .find(|l| l.starts_with("wifi:connected:"));

    if let Some(line) = wifi {
        let ssid = line.splitn(3, ':').nth(2).unwrap_or("").to_string();
        let signal = get_wifi_signal();
        let ip = get_ip();
        return json!({
            "connected": true,
            "type": "wifi",
            "ssid": ssid,
            "signal": signal,
            "ip": ip,
            "networks": networks,
        });
    }

    // Check ethernet
    let eth = dev_str
        .lines()
        .any(|l| l.starts_with("ethernet:connected"));

    if eth {
        let ip = get_ip();
        return json!({
            "connected": true,
            "type": "ethernet",
            "ssid": "",
            "signal": 100,
            "ip": ip,
            "networks": networks,
        });
    }

    json!({
        "connected": false,
        "type": "",
        "ssid": "",
        "signal": 0,
        "ip": "",
        "networks": networks,
    })
}

fn get_networks() -> Vec<Value> {
    let Ok(out) = Command::new("nmcli")
        .args(["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list"])
        .output()
    else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&out.stdout);

    let mut seen = std::collections::HashSet::new();
    text.lines()
        .filter_map(|line| {
            // Format: IN-USE:SSID:SIGNAL:SECURITY (colon-separated, IN-USE is * or empty)
            let in_use = line.starts_with('*');
            let rest = if in_use { &line[2..] } else { &line[1..] };
            let mut parts = rest.rsplitn(3, ':');
            let security = parts.next()?;
            let signal_str = parts.next()?;
            let ssid = parts.next()?;

            if ssid.is_empty() {
                return None;
            }
            // Deduplicate by SSID
            if !seen.insert(ssid.to_string()) {
                return None;
            }

            let signal = signal_str.trim().parse::<i64>().unwrap_or(0);
            Some(json!({
                "ssid": ssid,
                "signal": signal,
                "security": security,
                "in_use": in_use,
            }))
        })
        .collect()
}

fn get_wifi_signal() -> i64 {
    Command::new("nmcli")
        .args(["-t", "-f", "IN-USE,SIGNAL", "device", "wifi", "list"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            text.lines()
                .find(|l| l.starts_with('*'))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|s| s.trim().parse::<i64>().ok())
        })
        .unwrap_or(0)
}

fn get_ip() -> String {
    Command::new("nmcli")
        .args(["-t", "-f", "IP4.ADDRESS", "device", "show"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            text.lines()
                .find(|l| l.contains("IP4.ADDRESS"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.split('/').next().unwrap_or("").to_string())
        })
        .unwrap_or_default()
}

// --- Deep module ---

enum NetState {
    Browse,
    Password { ssid: String, input: String },
}

pub struct NetworkDeep {
    cursor: usize,
    state: NetState,
}

impl NetworkDeep {
    pub fn new() -> Self {
        Self { cursor: 0, state: NetState::Browse }
    }

    fn network_count(data: &Value) -> usize {
        data.get("networks")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    }

    fn is_known_network(ssid: &str) -> bool {
        Command::new("nmcli")
            .args(["-t", "-f", "NAME", "connection", "show"])
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l == ssid)
            })
            .unwrap_or(false)
    }
}

impl InteractiveModule for NetworkDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        match &self.state {
            NetState::Password { ssid, input } => {
                let dots = "•".repeat(input.len());
                return vec![vec![
                    Elem::text(format!("{ssid} password: {dots}▎")).fg(fg),
                ]];
            }
            NetState::Browse => {}
        }

        let networks = data.get("networks").and_then(|v| v.as_array());
        let networks = match networks {
            Some(n) if !n.is_empty() => n,
            _ => return vec![vec![Elem::text("no networks").fg(idle_fg)]],
        };

        networks.iter().enumerate().map(|(i, net)| {
            let ssid = net.get("ssid").and_then(|v| v.as_str()).unwrap_or("?");
            let signal = net.get("signal").and_then(|v| v.as_i64()).unwrap_or(0);
            let in_use = net.get("in_use").and_then(|v| v.as_bool()).unwrap_or(false);
            let secured = net.get("security").and_then(|v| v.as_str()).unwrap_or("") != "";

            let selected = i == self.cursor;
            let net_fg = if selected {
                fg
            } else if in_use {
                active_fg
            } else {
                idle_fg
            };

            let lock = if secured { "🔒" } else { "" };
            let prefix = if selected {
                "▸"
            } else if in_use {
                "●"
            } else {
                "○"
            };
            vec![Elem::text(format!("{prefix} {ssid} {signal}% {lock}")).fg(net_fg)]
        }).collect()
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }

    fn breadcrumb(&self) -> Vec<String> {
        match &self.state {
            NetState::Password { ssid, .. } => vec!["Network".into(), ssid.clone()],
            NetState::Browse => vec!["Network".into()],
        }
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        match &self.state {
            NetState::Password { .. } => vec![
                KeyHintDef { key: "⏎".into(), action: String::new(), label: "connect".into(), icon: None },
                KeyHintDef { key: "Esc".into(), action: String::new(), label: "cancel".into(), icon: None },
            ],
            NetState::Browse => vec![
                KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
                KeyHintDef { key: "⏎".into(), action: String::new(), label: "connect".into(), icon: None },
                KeyHintDef { key: "s".into(), action: String::new(), label: "scan".into(), icon: None },
                KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
            ],
        }
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        match &mut self.state {
            NetState::Password { ssid, input } => {
                match event.keysym {
                    Keysym::Escape => {
                        self.state = NetState::Browse;
                        KeyResult::Handled
                    }
                    Keysym::BackSpace => {
                        input.pop();
                        KeyResult::Handled
                    }
                    Keysym::Return => {
                        if !input.is_empty() {
                            let cmd = format!(
                                "nmcli device wifi connect '{}' password '{}'",
                                ssid.replace('\'', "'\\''"),
                                input.replace('\'', "'\\''"),
                            );
                            BarApp::spawn_command(&cmd);
                            self.state = NetState::Browse;
                        }
                        KeyResult::Action
                    }
                    _ => {
                        if let Some(s) = &event.utf8 {
                            if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                                input.push_str(s);
                            }
                        }
                        KeyResult::Handled
                    }
                }
            }
            NetState::Browse => {
                let count = Self::network_count(data);

                match event.keysym {
                    Keysym::Left => {
                        if count > 0 {
                            self.cursor = self.cursor.checked_sub(1).unwrap_or(count - 1);
                        }
                        KeyResult::Handled
                    }
                    Keysym::Right => {
                        if count > 0 {
                            self.cursor = (self.cursor + 1) % count;
                        }
                        KeyResult::Handled
                    }
                    Keysym::Return => {
                        if let Some(networks) = data.get("networks").and_then(|v| v.as_array()) {
                            if let Some(net) = networks.get(self.cursor) {
                                let ssid = net.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
                                let in_use = net.get("in_use").and_then(|v| v.as_bool()).unwrap_or(false);
                                let secured = net.get("security").and_then(|v| v.as_str()).unwrap_or("") != "";
                                if !ssid.is_empty() {
                                    if in_use {
                                        BarApp::spawn_command(&format!(
                                            "nmcli connection down id '{}'",
                                            ssid.replace('\'', "'\\''"),
                                        ));
                                    } else if secured && !Self::is_known_network(ssid) {
                                        self.state = NetState::Password {
                                            ssid: ssid.to_string(),
                                            input: String::new(),
                                        };
                                    } else {
                                        BarApp::spawn_command(&format!(
                                            "nmcli device wifi connect '{}'",
                                            ssid.replace('\'', "'\\''"),
                                        ));
                                    }
                                }
                            }
                        }
                        KeyResult::Action
                    }
                    _ if event.utf8.as_deref() == Some("s") => {
                        BarApp::spawn_command("nmcli device wifi rescan");
                        KeyResult::Action
                    }
                    _ => KeyResult::Ignored,
                }
            }
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
        self.state = NetState::Browse;
    }
}
