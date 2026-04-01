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
    let groups = store.by_app();
    let muted: Vec<String> = store.muted_apps().iter().cloned().collect();
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

    let apps: Vec<Value> = groups.iter().map(|g| {
        json!({
            "app": g.app_name,
            "count": g.count,
            "unread": g.unread,
            "muted": g.muted,
        })
    }).collect();

    json!({
        "count": count,
        "notifications": notifications,
        "apps": apps,
        "muted": muted,
    })
}

// --- Deep module ---

#[derive(Debug, Clone, PartialEq)]
enum View {
    AppList,
    Detail { app: Option<String> },
}

pub struct NotificationsDeep {
    cursor: usize,
    view: View,
}

impl NotificationsDeep {
    pub fn new() -> Self {
        Self { cursor: 0, view: View::AppList }
    }

    fn apps(&self, data: &Value) -> Vec<Value> {
        data.get("apps").and_then(|v| v.as_array()).cloned().unwrap_or_default()
    }

    fn app_list_entries(&self, data: &Value) -> Vec<AppEntry> {
        let apps = self.apps(data);
        let total: usize = apps.iter()
            .map(|a| a.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize)
            .sum();

        let mut entries = Vec::new();
        if total > 0 {
            entries.push(AppEntry { name: "all".to_string(), count: total, muted: false });
        }
        for a in &apps {
            let name = a.get("app").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let count = a.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let muted = a.get("muted").and_then(|v| v.as_bool()).unwrap_or(false);
            if !name.is_empty() {
                entries.push(AppEntry { name, count, muted });
            }
        }

        // Include muted apps with zero notifications so they can be unmuted
        let store = notifications::STORE.lock().unwrap();
        for muted_app in store.muted_apps() {
            if !entries.iter().any(|e| e.name == *muted_app) {
                entries.push(AppEntry { name: muted_app.clone(), count: 0, muted: true });
            }
        }
        drop(store);

        entries
    }

    fn filtered_notifs(&self, data: &Value) -> Vec<Value> {
        let all = data.get("notifications").and_then(|v| v.as_array());
        match &self.view {
            View::Detail { app: Some(app) } => {
                all.map(|ns| ns.iter()
                    .filter(|n| n.get("app").and_then(|v| v.as_str()) == Some(app))
                    .cloned()
                    .collect())
                    .unwrap_or_default()
            }
            View::Detail { app: None } => all.cloned().unwrap_or_default(),
            View::AppList => Vec::new(),
        }
    }

    fn detail_count(&self, data: &Value) -> usize {
        self.filtered_notifs(data).len()
    }

    fn selected_id(&self, data: &Value) -> Option<u32> {
        self.filtered_notifs(data)
            .get(self.cursor)
            .and_then(|n| n.get("id"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
    }

    fn selected_app_entry<'a>(&self, entries: &'a [AppEntry]) -> Option<&'a AppEntry> {
        entries.get(self.cursor)
    }
}

struct AppEntry {
    name: String,
    count: usize,
    muted: bool,
}

impl InteractiveModule for NotificationsDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        match &self.view {
            View::AppList => {
                let entries = self.app_list_entries(data);
                if entries.is_empty() {
                    return vec![vec![Elem::text("no notifications").fg(idle_fg)]];
                }

                let store = notifications::STORE.lock().unwrap();
                let groups = store.by_app();
                drop(store);

                entries.iter().enumerate().map(|(i, entry)| {
                    let is_sel = i == self.cursor;
                    let base_fg = if entry.muted {
                        idle_fg
                    } else if is_sel {
                        fg
                    } else {
                        active_fg
                    };

                    let mut elems = Vec::new();

                    // App icon
                    if entry.name != "all" {
                        if let Some(g) = groups.iter().find(|g| g.app_name == entry.name) {
                            if let Some(ref pm) = g.icon_pixmap {
                                elems.push(Elem::text(String::new()).icon(pm.clone()).fg(base_fg));
                            }
                        }
                    }

                    // Label with count
                    let label = if entry.count > 0 {
                        format!("{} ({})", entry.name, entry.count)
                    } else {
                        format!("{} (muted)", entry.name)
                    };
                    elems.push(Elem::text(label).fg(base_fg));

                    elems
                }).collect()
            }
            View::Detail { app } => {
                let notifs = self.filtered_notifs(data);
                if notifs.is_empty() {
                    let label = match app {
                        Some(a) => format!("no notifications from {a}"),
                        None => "no notifications".to_string(),
                    };
                    return vec![vec![Elem::text(label).fg(idle_fg)]];
                }

                let store = notifications::STORE.lock().unwrap();
                let stored = store.all();
                drop(store);

                notifs.iter().enumerate().map(|(i, n)| {
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

                    vec![widget]
                }).collect()
            }
        }
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }

    fn breadcrumb(&self) -> Vec<String> {
        vec![]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        match &self.view {
            View::AppList => {
                let entries = vec![
                    KeyHintDef { key: "←→".into(), action: String::new(), label: "scroll".into(), icon: None },
                    KeyHintDef { key: "Enter".into(), action: String::new(), label: "open".into(), icon: None },
                    KeyHintDef { key: "m".into(), action: String::new(), label: "mute/unmute".into(), icon: None },
                    KeyHintDef { key: "d".into(), action: String::new(), label: "dismiss app".into(), icon: None },
                    KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
                ];
                entries
            }
            View::Detail { .. } => {
                vec![
                    KeyHintDef { key: "←→".into(), action: String::new(), label: "scroll".into(), icon: None },
                    KeyHintDef { key: "d".into(), action: String::new(), label: "dismiss".into(), icon: None },
                    KeyHintDef { key: "c".into(), action: String::new(), label: "clear".into(), icon: None },
                    KeyHintDef { key: "Esc".into(), action: String::new(), label: "back".into(), icon: None },
                ]
            }
        }
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        match &self.view {
            View::AppList => self.handle_app_list_key(event, data),
            View::Detail { .. } => self.handle_detail_key(event, data),
        }
    }

    fn activate(&mut self, data: &serde_json::Value, sub_path: &[String]) {
        if let Some(app) = sub_path.first() {
            // Direct navigation to an app's notifications
            self.view = View::Detail { app: Some(app.clone()) };
            let notifs = self.filtered_notifs(data);
            self.cursor = notifs.iter().position(|n| {
                n.get("read").and_then(|v| v.as_bool()) == Some(false)
            }).unwrap_or(0);
        } else {
            self.view = View::AppList;
            self.cursor = 0;
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
        self.view = View::AppList;
    }

    fn exec_action(&mut self, name: &str, _args: &[String], _data: &serde_json::Value) -> Option<String> {
        match name {
            "clear" => {
                crate::notifications::STORE.lock().unwrap().clear_all();
                self.cursor = 0;
                Some("cleared".into())
            }
            _ => None,
        }
    }
}

impl NotificationsDeep {
    fn handle_app_list_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let entries = self.app_list_entries(data);
        let count = entries.len();

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
                if let Some(entry) = self.selected_app_entry(&entries) {
                    let app = if entry.name == "all" { None } else { Some(entry.name.clone()) };
                    self.view = View::Detail { app };
                    self.cursor = 0;
                }
                KeyResult::Handled
            }
            _ if event.utf8.as_deref() == Some("m") => {
                if let Some(entry) = self.selected_app_entry(&entries) {
                    if entry.name != "all" {
                        let mut store = notifications::STORE.lock().unwrap();
                        if entry.muted {
                            store.unmute(&entry.name);
                        } else {
                            store.mute(&entry.name);
                        }
                    }
                }
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("d") => {
                if let Some(entry) = self.selected_app_entry(&entries) {
                    if entry.name == "all" {
                        notifications::STORE.lock().unwrap().clear_all();
                        self.cursor = 0;
                    } else {
                        notifications::STORE.lock().unwrap().dismiss_app(&entry.name);
                        if self.cursor > 0 && self.cursor >= count.saturating_sub(1) {
                            self.cursor = count.saturating_sub(2);
                        }
                    }
                }
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn handle_detail_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let count = self.detail_count(data);

        match event.keysym {
            Keysym::Escape => {
                self.view = View::AppList;
                self.cursor = 0;
                return KeyResult::Handled;
            }
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
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("c") => {
                match &self.view {
                    View::Detail { app: Some(app) } => {
                        notifications::STORE.lock().unwrap().dismiss_app(app);
                    }
                    _ => {
                        notifications::STORE.lock().unwrap().clear_all();
                    }
                }
                self.cursor = 0;
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }
}
