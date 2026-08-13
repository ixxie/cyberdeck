//! Airplane mode — the kernel's rfkill switches.
//!
//! rfkill blocks are sticky in a way that surprises people: systemd-rfkill
//! persists them across reboots, and unlike wifi (which NetworkManager
//! unblocks when you re-enable wireless) nothing ever clears a bluetooth
//! block on its own — BlueZ treats a kill switch as deliberate intent and
//! refuses to override it. A stray airplane-mode keypress can therefore take
//! bluetooth out for days while every UI simply reports it as "off".
//!
//! So this module reports the state per radio, makes a partial block (some
//! radios down, some up) as visible as a full one, and can clear blocks —
//! /dev/rfkill carries a uaccess ACL for the logged-in user, so no
//! privileged helper is needed.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use serde_json::{json, Value};

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

const RFKILL_DEV: &str = "/dev/rfkill";
const RFKILL_SYS: &str = "/sys/class/rfkill";

// linux/rfkill.h
const OP_CHANGE: u8 = 2;
const OP_CHANGE_ALL: u8 = 3;
const TYPE_ALL: u8 = 0;
const EVENT_SIZE_V1: usize = 8;

#[derive(Clone)]
struct Radio {
    idx: u32,
    kind: String,
    name: String,
    soft: bool,
    hard: bool,
}

impl Radio {
    fn blocked(&self) -> bool {
        self.soft || self.hard
    }
}

fn read_flag(dir: &Path, file: &str) -> bool {
    fs::read_to_string(dir.join(file))
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn read_radios() -> Vec<Radio> {
    let Ok(entries) = fs::read_dir(RFKILL_SYS) else {
        return Vec::new();
    };
    let mut radios: Vec<Radio> = entries
        .filter_map(|e| {
            let dir = e.ok()?.path();
            let idx = fs::read_to_string(dir.join("index")).ok()?.trim().parse().ok()?;
            let kind = fs::read_to_string(dir.join("type")).ok()?.trim().to_string();
            let name = fs::read_to_string(dir.join("name"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            Some(Radio {
                idx,
                kind,
                name,
                soft: read_flag(&dir, "soft"),
                hard: read_flag(&dir, "hard"),
            })
        })
        .collect();
    radios.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.idx.cmp(&b.idx)));
    radios
}

fn write_event(idx: u32, kind: u8, op: u8, soft: bool) -> Result<(), String> {
    // struct rfkill_event { u32 idx; u8 type, op, soft, hard; } — packed.
    // Writing the v1 size keeps this valid across kernel versions that have
    // since grown the struct.
    let mut buf = [0u8; EVENT_SIZE_V1];
    buf[0..4].copy_from_slice(&idx.to_ne_bytes());
    buf[4] = kind;
    buf[5] = op;
    buf[6] = u8::from(soft);

    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(RFKILL_DEV)
        .map_err(|e| format!("cannot open {RFKILL_DEV}: {e}"))?;
    f.write_all(&buf).map_err(|e| format!("rfkill write failed: {e}"))
}

fn kind_id(kind: &str) -> u8 {
    match kind {
        "wlan" => 1,
        "bluetooth" => 2,
        "uwb" => 3,
        "wimax" => 4,
        "wwan" => 5,
        "gps" => 6,
        "fm" => 7,
        "nfc" => 8,
        _ => TYPE_ALL,
    }
}

// ---- Public API ----

/// Block or unblock every radio.
pub fn set_all(blocked: bool) -> Result<(), String> {
    write_event(0, TYPE_ALL, OP_CHANGE_ALL, blocked)
}

/// Block or unblock every radio of one kind (`wlan`, `bluetooth`, …).
pub fn set_kind(kind: &str, blocked: bool) -> Result<(), String> {
    write_event(0, kind_id(kind), OP_CHANGE_ALL, blocked)
}

/// Is every switch of this kind soft-blocked? False when none exist.
pub fn kind_blocked(kind: &str) -> bool {
    let radios: Vec<Radio> = read_radios().into_iter().filter(|r| r.kind == kind).collect();
    !radios.is_empty() && radios.iter().all(Radio::blocked)
}

/// Is every radio on the machine blocked — i.e. airplane mode proper?
pub fn all_blocked() -> bool {
    let radios = read_radios();
    !radios.is_empty() && radios.iter().all(Radio::blocked)
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let radios = read_radios();
    let blocked: Vec<&Radio> = radios.iter().filter(|r| r.blocked()).collect();
    let enabled = !radios.is_empty() && blocked.len() == radios.len();
    let partial = !blocked.is_empty() && !enabled;

    // Kinds rather than switch names: "bluetooth" reads better than "hci0".
    let mut blocked_kinds: Vec<String> = blocked.iter().map(|r| r.kind.clone()).collect();
    blocked_kinds.dedup();

    json!({
        "enabled": enabled,
        "partial": partial,
        "blocked_kinds": blocked_kinds,
        "blocked_count": blocked.len(),
        "total": radios.len(),
        "radios": radios.iter().map(|r| json!({
            "idx": r.idx,
            "kind": r.kind,
            "name": r.name,
            "blocked": r.blocked(),
            "hard": r.hard,
        })).collect::<Vec<_>>(),
    })
}

/// /dev/rfkill emits an event on every state change (and one per existing
/// switch at open), so a blocking read is the whole subscription.
pub fn subscribe(
    params: serde_json::Map<String, Value>,
    sender: Sender<(String, Value)>,
    id: String,
) {
    let Ok(mut dev) = fs::File::open(RFKILL_DEV) else {
        log::warn!("airplane: cannot open {RFKILL_DEV}, falling back to polling");
        let mut last = Value::Null;
        loop {
            let val = poll(&params);
            if val != last {
                last = val.clone();
                if sender.send((id.clone(), val)).is_err() {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    };

    let mut buf = [0u8; 64];
    let mut last = Value::Null;
    loop {
        match dev.read(&mut buf) {
            Ok(0) => return,
            Ok(_) => {
                let val = poll(&params);
                if val != last {
                    last = val.clone();
                    if sender.send((id.clone(), val)).is_err() {
                        return;
                    }
                }
            }
            Err(e) => {
                log::warn!("airplane: rfkill read failed: {e}");
                return;
            }
        }
    }
}

/// CLI/IPC action dispatcher. Returns toast text describing what happened.
pub fn cli_action(name: &str, args: &[String]) -> Result<String, String> {
    let apply = |blocked: bool, label: &str| -> Result<String, String> {
        set_all(blocked).map(|()| label.to_string())
    };
    match name {
        "on" => apply(true, "airplane mode on"),
        "off" => apply(false, "airplane mode off"),
        "toggle" => {
            let on = all_blocked();
            apply(!on, if on { "airplane mode off" } else { "airplane mode on" })
        }
        "block" | "unblock" => {
            let blocked = name == "block";
            let kind = args.first().ok_or("usage: airplane block|unblock <wlan|bluetooth|…>")?;
            set_kind(kind, blocked).map(|()| format!("{kind} {name}ed"))
        }
        other => Err(format!("unknown airplane action: {other}")),
    }
}

// ---- Deep module ----

pub struct AirplaneDeep {
    cursor: usize,
    icon_airplane: String,
    icon_wifi: String,
    icon_wifi_off: String,
    icon_bt: String,
    icon_bt_off: String,
    icon_radio: String,
}

impl AirplaneDeep {
    pub fn new(icon_resolver: &dyn Fn(&str) -> String) -> Self {
        Self {
            cursor: 0,
            icon_airplane: icon_resolver("airplane-tilt"),
            icon_wifi: icon_resolver("wifi-high"),
            icon_wifi_off: icon_resolver("wifi-slash"),
            icon_bt: icon_resolver("bluetooth"),
            icon_bt_off: icon_resolver("bluetooth-slash"),
            icon_radio: icon_resolver("broadcast"),
        }
    }

    fn radios(data: &Value) -> Vec<&Value> {
        data.get("radios")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().collect())
            .unwrap_or_default()
    }

    fn icon_for(&self, kind: &str, blocked: bool) -> &str {
        match (kind, blocked) {
            ("wlan", false) => &self.icon_wifi,
            ("wlan", true) => &self.icon_wifi_off,
            ("bluetooth", false) => &self.icon_bt,
            ("bluetooth", true) => &self.icon_bt_off,
            _ => &self.icon_radio,
        }
    }
}

impl InteractiveModule for AirplaneDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);

        let radios = Self::radios(data);
        if radios.is_empty() {
            return vec![vec![Elem::text("no radios").fg(idle_fg)]];
        }

        let enabled = data.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut rows: Vec<Vec<Elem>> = vec![vec![
            Elem::text(format!(
                "{} airplane mode {}",
                self.icon_airplane,
                if enabled { "on" } else { "off" }
            ))
            .fg(if enabled { fg } else { idle_fg }),
        ]];

        for (i, r) in radios.iter().enumerate() {
            let kind = r.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
            let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let blocked = r.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false);
            let hard = r.get("hard").and_then(|v| v.as_bool()).unwrap_or(false);

            // A hard block is a physical switch — say so, since no amount of
            // pressing enter here will clear it.
            let state = if hard {
                "hard blocked"
            } else if blocked {
                "blocked"
            } else {
                "on"
            };
            let label = if name.is_empty() {
                format!("{kind} {state}")
            } else {
                format!("{kind} ({name}) {state}")
            };
            let row_fg = if i == self.cursor {
                fg
            } else if blocked {
                idle_fg
            } else {
                active_fg
            };
            rows.push(vec![
                Elem::text(format!("{} {label}", self.icon_for(kind, blocked))).fg(row_fg),
            ]);
        }
        rows
    }

    fn cursor(&self) -> Option<usize> {
        // Row 0 is the summary; the cursor indexes radios after it.
        Some(self.cursor + 1)
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "toggle radio".into(), icon: None },
            KeyHintDef { key: "a".into(), action: String::new(), label: "toggle all".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let radios = Self::radios(data);
        let count = radios.len();

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
                let Some(r) = radios.get(self.cursor) else {
                    return KeyResult::Handled;
                };
                let idx = r.get("idx").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let blocked = r.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false);
                if r.get("hard").and_then(|v| v.as_bool()).unwrap_or(false) {
                    crate::ipc::notify_bar("airplane", "hard blocked — use the physical switch");
                    return KeyResult::Action;
                }
                if let Err(e) = write_event(idx, TYPE_ALL, OP_CHANGE, !blocked) {
                    crate::ipc::notify_bar("airplane", &e);
                }
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("a") => {
                let enabled = data.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                if let Err(e) = set_all(!enabled) {
                    crate::ipc::notify_bar("airplane", &e);
                }
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }

    fn exec_action(&mut self, name: &str, args: &[String], _data: &Value) -> Option<String> {
        Some(match cli_action(name, args) {
            Ok(msg) | Err(msg) => msg,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_agrees_with_sysfs() {
        let val = poll(&serde_json::Map::new());
        let radios = val["radios"].as_array().unwrap();
        if radios.is_empty() {
            return; // no radios here (VM/CI) — nothing to check
        }
        let blocked = radios.iter().filter(|r| r["blocked"].as_bool().unwrap()).count();
        assert_eq!(val["blocked_count"], blocked);
        assert_eq!(val["total"], radios.len());
        assert_eq!(val["enabled"], blocked == radios.len());
        assert_eq!(val["partial"], blocked > 0 && blocked < radios.len());
        for r in radios {
            assert!(!r["kind"].as_str().unwrap().is_empty());
        }
    }

    /// The event struct is written raw to the kernel, so a layout mistake
    /// would surface as EINVAL. Unblocking is the safe direction to prove it
    /// with: it can only ever turn radios on.
    #[test]
    fn kernel_accepts_event_layout() {
        if !Path::new(RFKILL_DEV).exists() {
            return;
        }
        match set_all(false) {
            Ok(()) => {}
            Err(e) if e.contains("Permission denied") => {} // no uaccess ACL here
            Err(e) => panic!("kernel rejected rfkill event: {e}"),
        }
    }
}
