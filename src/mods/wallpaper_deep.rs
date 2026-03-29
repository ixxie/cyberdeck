use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

pub struct WallpaperDeep {
    cursor: usize,
    params: serde_json::Map<String, serde_json::Value>,
}

impl WallpaperDeep {
    pub fn new(module: &crate::config::ModuleDef) -> Self {
        let params = module.params.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self { cursor: 0, params }
    }

    fn group_names(data: &serde_json::Value) -> Vec<String> {
        data.get("entries")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn group_exec(data: &serde_json::Value, idx: usize) -> Option<String> {
        data.get("entries")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(idx))
            .and_then(|e| e.get("exec"))
            .and_then(|v| v.as_str())
            .map(String::from)
    }
}

impl InteractiveModule for WallpaperDeep {
    fn render_center(&self, fg: Rgba, data: &serde_json::Value) -> Vec<Vec<Elem>> {
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);

        let groups = Self::group_names(data);
        if groups.is_empty() {
            return vec![vec![Elem::text("no groups").fg(idle_fg)]];
        }

        let current_group = data.get("group")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        groups.iter().enumerate().map(|(i, name)| {
            let is_active = (!current_group.is_empty() && name == current_group)
                || (current_group.is_empty() && name == "All");
            let item_fg = if i == self.cursor {
                fg
            } else if is_active {
                active_fg
            } else {
                idle_fg
            };
            vec![Elem::text(name.clone()).fg(item_fg)]
        }).collect()
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }

    fn breadcrumb(&self) -> Vec<String> {
        vec!["Wallpaper".into()]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "shuffle".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &serde_json::Value) -> KeyResult {
        let count = Self::group_names(data).len();
        if count == 0 { return KeyResult::Ignored; }

        match event.keysym {
            Keysym::Left => {
                self.cursor = self.cursor.checked_sub(1).unwrap_or(count - 1);
                KeyResult::Handled
            }
            Keysym::Right => {
                self.cursor = (self.cursor + 1) % count;
                KeyResult::Handled
            }
            Keysym::Return => {
                if let Some(exec) = Self::group_exec(data, self.cursor) {
                    BarApp::spawn_command(&exec);
                }
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }

    fn exec_action(&mut self, name: &str, args: &[String], _data: &serde_json::Value) -> Option<String> {
        match name {
            "shuffle" => {
                let group = args.first().map(|s| s.as_str());
                crate::mods::wallpaper::shuffle(&self.params, group);
                Some("shuffled".into())
            }
            "init" => {
                crate::mods::wallpaper::init(&self.params);
                Some("initialized".into())
            }
            _ => None,
        }
    }
}
