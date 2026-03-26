use serde_json::{json, Value};

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};
use crate::pipewire;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let pw = pipewire::query();

    let default = pw.outputs.iter().find(|d| d.is_default);
    let volume = default.map(|d| d.volume).unwrap_or(0);
    let muted = default.map(|d| d.muted).unwrap_or(false);
    let sink_name = default.map(|d| d.name.as_str()).unwrap_or("unknown");

    let outputs: Vec<Value> = pw.outputs.iter().map(|d| json!({
        "id": d.id,
        "name": d.name,
        "volume": d.volume,
        "muted": d.muted,
        "default": d.is_default,
    })).collect();

    json!({
        "volume": volume,
        "muted": muted,
        "sink": sink_name,
        "outputs": outputs,
    })
}

// --- Deep module ---

pub struct OutputsDeep;

impl OutputsDeep {
    pub fn new() -> Self {
        Self
    }

    fn outputs<'a>(&self, data: &'a Value) -> Option<&'a Vec<Value>> {
        data.get("outputs").and_then(|v| v.as_array())
    }

    fn default_id(&self, data: &Value) -> Option<u32> {
        self.outputs(data)
            .and_then(|devs| devs.iter().find(|d| {
                d.get("default").and_then(|v| v.as_bool()).unwrap_or(false)
            }))
            .and_then(|d| d.get("id"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
    }

    fn cycle_device(&self, data: &Value, forward: bool) {
        let outputs = match self.outputs(data) {
            Some(o) if o.len() > 1 => o,
            _ => return,
        };

        let cur_idx = outputs.iter().position(|d| {
            d.get("default").and_then(|v| v.as_bool()).unwrap_or(false)
        }).unwrap_or(0);

        let next_idx = if forward {
            (cur_idx + 1) % outputs.len()
        } else {
            cur_idx.checked_sub(1).unwrap_or(outputs.len() - 1)
        };

        if let Some(id) = outputs.get(next_idx)
            .and_then(|d| d.get("id"))
            .and_then(|v| v.as_u64())
        {
            BarApp::spawn_command(&format!("wpctl set-default {id}"));
        }
    }
}

impl InteractiveModule for OutputsDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Elem> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let sink = data.get("sink").and_then(|v| v.as_str()).unwrap_or("unknown");
        let vol = data.get("volume").and_then(|v| v.as_i64()).unwrap_or(0);
        let muted = data.get("muted").and_then(|v| v.as_bool()).unwrap_or(false);
        let vol_str = if muted { "muted".to_string() } else { format!("{vol}%") };

        let outputs = self.outputs(data);
        if outputs.map(|o| o.is_empty()).unwrap_or(true) {
            return vec![Elem::text("no outputs").fg(idle_fg)];
        }

        vec![
            Elem::text(format!("● {sink} {vol_str}")).fg(active_fg),
        ]
    }

    fn breadcrumb(&self) -> Vec<String> {
        vec![]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "↑↓".into(), action: String::new(), label: "vol".into(), icon: None },
            KeyHintDef { key: "m".into(), action: String::new(), label: "mute".into(), icon: None },
            KeyHintDef { key: "Tab".into(), action: String::new(), label: "device".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        match event.keysym {
            Keysym::Up => {
                if let Some(id) = self.default_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-volume {id} 5%+"));
                }
                KeyResult::Action
            }
            Keysym::Down => {
                if let Some(id) = self.default_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-volume {id} 5%-"));
                }
                KeyResult::Action
            }
            Keysym::Tab => {
                self.cycle_device(data, true);
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("m") => {
                if let Some(id) = self.default_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-mute {id} toggle"));
                }
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn reset(&mut self) {}
}
