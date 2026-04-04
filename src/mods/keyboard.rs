use serde_json::Value;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

pub struct KeyboardDeep {
    cursor: usize,
}

impl KeyboardDeep {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    fn layouts(data: &Value) -> Vec<String> {
        data.get("layouts")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn active_idx(data: &Value) -> usize {
        data.get("layout_idx")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize
    }
}

impl InteractiveModule for KeyboardDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let layouts = Self::layouts(data);
        if layouts.is_empty() {
            return vec![vec![Elem::text("no layouts").fg(idle_fg)]];
        }

        let active = Self::active_idx(data);
        layouts.iter().enumerate().map(|(i, name)| {
            let item_fg = if i == self.cursor {
                fg
            } else if i == active {
                active_fg
            } else {
                idle_fg
            };
            let prefix = if i == active { "●" } else { "○" };
            vec![Elem::text(format!("{prefix} {name}")).fg(item_fg)]
        }).collect()
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }



    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let count = Self::layouts(data).len();
        if count == 0 {
            return KeyResult::Ignored;
        }

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
                BarApp::spawn_command(
                    &format!("swaymsg input type:keyboard xkb_switch_layout {}", self.cursor),
                );
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}
