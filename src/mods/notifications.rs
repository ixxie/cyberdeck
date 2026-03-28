use serde_json::{json, Value};

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};
use crate::notifications;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let store = notifications::STORE.lock().unwrap();
    let count = store.unread_count();
    let all = store.all();
    drop(store);

    let notifications: Vec<Value> = all.iter().map(|n| {
        let ago = n.timestamp
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        json!({
            "id": n.id,
            "app": n.app_name,
            "summary": n.summary,
            "body": n.body,
            "read": n.read,
            "ago": ago,
        })
    }).collect();

    json!({
        "count": count,
        "notifications": notifications,
    })
}

// --- Deep module ---

pub struct NotificationsDeep {
    cursor: usize,
}

impl NotificationsDeep {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    fn notifs<'a>(&self, data: &'a Value) -> Option<&'a Vec<Value>> {
        data.get("notifications").and_then(|v| v.as_array())
    }

    fn count(&self, data: &Value) -> usize {
        self.notifs(data).map(|a| a.len()).unwrap_or(0)
    }

    fn selected_id(&self, data: &Value) -> Option<u32> {
        self.notifs(data)
            .and_then(|ns| ns.get(self.cursor))
            .and_then(|n| n.get("id"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
    }
}

impl InteractiveModule for NotificationsDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let notifs = match self.notifs(data) {
            Some(n) if !n.is_empty() => n,
            _ => return vec![vec![Elem::text("no notifications").fg(idle_fg)]],
        };

        let store = notifications::STORE.lock().unwrap();
        let stored = store.all();
        drop(store);

        let mut items = Vec::new();
        for (i, n) in notifs.iter().enumerate() {
            let summary = n.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            let body = n.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let read = n.get("read").and_then(|v| v.as_bool()).unwrap_or(false);
            let id = n.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

            let nfg = if i == self.cursor {
                fg
            } else if !read {
                active_fg
            } else {
                idle_fg
            };

            let label = if !body.is_empty() {
                format!("{summary} — {body}")
            } else {
                summary.to_string()
            };
            let mut widget = Elem::text(label).fg(nfg);

            if let Some(sn) = stored.iter().find(|s| s.id == id) {
                if let Some(ref pm) = sn.icon_pixmap {
                    widget = widget.icon(pm.clone());
                }
            }

            items.push(vec![widget]);
        }

        items
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }

    fn breadcrumb(&self) -> Vec<String> {
        vec![]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "scroll".into(), icon: None },
            KeyHintDef { key: "d".into(), action: String::new(), label: "dismiss".into(), icon: None },
            KeyHintDef { key: "c".into(), action: String::new(), label: "clear".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let count = self.count(data);

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
            _ if event.utf8.as_deref() == Some("d") => {
                if let Some(id) = self.selected_id(data) {
                    notifications::STORE.lock().unwrap().dismiss(id);
                    if self.cursor > 0 && self.cursor >= count.saturating_sub(1) {
                        self.cursor = count.saturating_sub(2);
                    }
                }
                KeyResult::Handled
            }
            _ if event.utf8.as_deref() == Some("c") => {
                notifications::STORE.lock().unwrap().clear_all();
                self.cursor = 0;
                KeyResult::Handled
            }
            _ => KeyResult::Ignored,
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}
