use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::{ActionDef, KeyHintDef};
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

struct Action {
    key: String,
    name: String,
    label: String,
    run: String,
    icon_char: Option<String>,
}

pub struct ActionPalette {
    _module_name: String,
    actions: Vec<Action>,
    cursor: usize,
}

impl ActionPalette {
    pub fn new(name: &str, action_defs: &[ActionDef], icon_resolver: &dyn Fn(&str) -> String) -> Self {
        let actions: Vec<Action> = action_defs.iter()
            .map(|a| {
                let icon_char = a.icon.as_deref().map(icon_resolver);
                let key = a.key.clone().unwrap_or_default();
                Action {
                    key,
                    name: a.name.clone(),
                    label: if a.label.is_empty() { a.name.clone() } else { a.label.clone() },
                    run: a.run.clone(),
                    icon_char,
                }
            })
            .collect();
        Self {
            _module_name: name.to_string(),
            actions,
            cursor: 0,
        }
    }
}

impl ActionPalette {
    fn active_index(&self, data: &serde_json::Value) -> Option<usize> {
        if let serde_json::Value::Object(map) = data {
            for val in map.values() {
                if let serde_json::Value::String(s) = val {
                    if let Some(i) = self.actions.iter().position(|a| a.name == *s) {
                        return Some(i);
                    }
                }
            }
        }
        None
    }
}

impl InteractiveModule for ActionPalette {
    fn render_center(&self, fg: Rgba, data: &serde_json::Value) -> Vec<Vec<Elem>> {
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let active = self.active_index(data);

        self.actions.iter().enumerate().map(|(i, action)| {
            let item_fg = if i == self.cursor {
                fg
            } else if Some(i) == active {
                active_fg
            } else {
                idle_fg
            };
            let mut text = String::new();
            if let Some(icon) = &action.icon_char {
                text.push_str(icon);
                text.push(' ');
            }
            if action.key.is_empty() {
                text.push_str(&action.label);
            } else {
                text.push_str(&format!("{} ({})", action.label, action.key));
            }
            vec![Elem::text(text).fg(item_fg)]
        }).collect()
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }



    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, _data: &serde_json::Value) -> KeyResult {
        let count = self.actions.len();
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
                if let Some(action) = self.actions.get(self.cursor) {
                    if action.run != "native" {
                        BarApp::spawn_command(&action.run);
                    }
                }
                KeyResult::Action
            }
            _ => {
                if let Some(ch) = event.utf8.as_deref() {
                    if let Some(action) = self.actions.iter().find(|a| a.key == ch) {
                        if action.run != "native" {
                            BarApp::spawn_command(&action.run);
                        }
                        return KeyResult::Action;
                    }
                }
                KeyResult::Ignored
            }
        }
    }

    fn activate(&mut self, data: &serde_json::Value, _sub_path: &[String]) {
        self.cursor = self.active_index(data).unwrap_or(0);
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}
