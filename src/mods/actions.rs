use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::RenderedWidget;
use crate::mods::InteractiveModule;

struct Action {
    key: String,
    label: String,
    command: String,
    icon_char: Option<String>,
}

pub struct ActionPalette {
    name: String,
    actions: Vec<Action>,
    cursor: usize,
}

impl ActionPalette {
    pub fn new(name: &str, hints: Vec<KeyHintDef>, icon_resolver: &dyn Fn(&str) -> String) -> Self {
        let actions: Vec<Action> = hints.into_iter()
            .filter(|h| h.action != "back")
            .map(|h| {
                let icon_char = h.icon.as_deref().map(icon_resolver);
                Action {
                    key: h.key,
                    label: if h.label.is_empty() { h.action.clone() } else { h.label },
                    command: h.action,
                    icon_char,
                }
            })
            .collect();
        Self {
            name: name.to_string(),
            actions,
            cursor: 0,
        }
    }
}

impl InteractiveModule for ActionPalette {
    fn render_center(&self, fg: Rgba, _data: &serde_json::Value) -> Vec<RenderedWidget> {
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        self.actions.iter().enumerate().map(|(i, action)| {
            let item_fg = if i == self.cursor { fg } else { idle_fg };
            let mut text = String::new();
            if let Some(icon) = &action.icon_char {
                text.push_str(icon);
                text.push(' ');
            }
            text.push_str(&format!("{} ({})", action.label, action.key));
            RenderedWidget::new(text).with_fg(item_fg)
        }).collect()
    }

    fn breadcrumb(&self) -> Vec<String> {
        vec![self.name.clone()]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "run".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, _data: &serde_json::Value) -> bool {
        let count = self.actions.len();
        if count == 0 { return false; }

        match event.keysym {
            Keysym::Left => {
                self.cursor = self.cursor.checked_sub(1).unwrap_or(count - 1);
                true
            }
            Keysym::Right => {
                self.cursor = (self.cursor + 1) % count;
                true
            }
            Keysym::Return => {
                if let Some(action) = self.actions.get(self.cursor) {
                    BarApp::spawn_command(&action.command);
                }
                true
            }
            _ => {
                if let Some(ch) = event.utf8.as_deref() {
                    if let Some(action) = self.actions.iter().find(|a| a.key == ch) {
                        BarApp::spawn_command(&action.command);
                        return true;
                    }
                }
                false
            }
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}
