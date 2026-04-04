use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use smithay_client_toolkit::reexports::calloop::channel::{self, Sender, Channel};
use tiny_skia::Pixmap;

// Global store accessible by poll threads and the bar
pub static STORE: Mutex<NotificationStore> = Mutex::new(NotificationStore::new());

const DEFAULT_TIMEOUT_MS: i32 = 5000;

#[derive(Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    #[allow(dead_code)]
    pub icon: String,
    pub icon_pixmap: Option<Arc<Pixmap>>,
    pub timeout_ms: i32,
    pub timestamp: Option<Instant>,
    pub read: bool,
}

/// Per-app notification group summary
pub struct AppGroup {
    pub app_name: String,
    pub count: usize,
    pub unread: usize,
    pub icon_pixmap: Option<Arc<Pixmap>>,
    pub muted: bool,
}

pub struct NotificationStore {
    notifications: VecDeque<Notification>,
    muted_apps: Vec<String>,
    next_id: u32,
}

// Event sent from D-Bus thread to main event loop
#[derive(Clone)]
pub enum NotifyEvent {
    New(Notification),
    Close(u32),
}

impl NotificationStore {
    const MAX: usize = 100;

    const fn new() -> Self {
        Self {
            notifications: VecDeque::new(),
            muted_apps: Vec::new(),
            next_id: 1,
        }
    }

    pub fn push(&mut self, mut n: Notification) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        n.id = id;
        n.timestamp = Some(Instant::now());
        self.notifications.push_front(n);
        if self.notifications.len() > Self::MAX {
            self.notifications.pop_back();
        }
        id
    }

    pub fn is_muted(&self, app_name: &str) -> bool {
        self.muted_apps.iter().any(|a| a == app_name)
    }

    pub fn dismiss(&mut self, id: u32) {
        self.notifications.retain(|n| n.id != id);
    }

    pub fn dismiss_app(&mut self, app_name: &str) {
        self.notifications.retain(|n| n.app_name != app_name);
    }

    pub fn mute(&mut self, app_name: &str) {
        if !self.muted_apps.iter().any(|a| a == app_name) {
            self.muted_apps.push(app_name.to_string());
        }
    }

    pub fn unmute(&mut self, app_name: &str) {
        self.muted_apps.retain(|a| a != app_name);
    }

    pub fn muted_apps(&self) -> &[String] {
        &self.muted_apps
    }

    pub fn clear_all(&mut self) {
        self.notifications.clear();
    }

    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    pub fn all(&self) -> Vec<Notification> {
        self.notifications.iter().cloned().collect()
    }

    /// Group notifications by app, ordered by most recent first
    pub fn by_app(&self) -> Vec<AppGroup> {
        let mut groups: HashMap<&str, (usize, usize, Option<Arc<Pixmap>>)> = HashMap::new();
        let mut order: Vec<&str> = Vec::new();

        for n in &self.notifications {
            let entry = groups.entry(&n.app_name).or_insert((0, 0, None));
            entry.0 += 1;
            if !n.read { entry.1 += 1; }
            if entry.2.is_none() {
                entry.2 = n.icon_pixmap.clone();
            }
            if !order.contains(&&*n.app_name) {
                order.push(&n.app_name);
            }
        }

        order.into_iter().map(|name| {
            let (count, unread, icon_pixmap) = groups.remove(name).unwrap();
            AppGroup {
                app_name: name.to_string(),
                count,
                unread,
                icon_pixmap,
                muted: self.muted_apps.iter().any(|a| a == name),
            }
        }).collect()
    }

    pub fn for_app(&self, app_name: &str) -> Vec<Notification> {
        self.notifications.iter()
            .filter(|n| n.app_name == app_name)
            .cloned()
            .collect()
    }
}

// D-Bus interface implementation
struct NotificationDaemon {
    sender: Sender<NotifyEvent>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotificationDaemon {
    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".into(),
            "persistence".into(),
        ]
    }

    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        _actions: Vec<String>,
        _hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let mut store = STORE.lock().unwrap();

        if replaces_id > 0 {
            store.dismiss(replaces_id);
        }

        let timeout_ms = if expire_timeout < 0 {
            DEFAULT_TIMEOUT_MS
        } else {
            expire_timeout
        };

        // Log hint keys for debugging
        let hint_keys: Vec<&str> = _hints.keys().map(|k| k.as_str()).collect();
        log::info!("notify hints: {:?}", hint_keys);

        // Resolve app icon: check app_icon param, then hints
        let icon_name = if !app_icon.is_empty() {
            app_icon.to_string()
        } else if let Some(v) = _hints.get("image-path") {
            v.try_clone().ok()
                .and_then(|v| <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(v).ok())
                .unwrap_or_default()
        } else {
            String::new()
        };
        log::info!("notify: app={} icon='{}' summary='{}'", app_name, icon_name, summary);
        let icon_pixmap = crate::appicon::lookup(&icon_name);

        let n = Notification {
            id: 0,
            app_name: app_name.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            icon: app_icon.to_string(),
            icon_pixmap,
            timeout_ms,
            timestamp: None,
            read: false,
        };
        let id = store.push(n.clone());
        drop(store);

        let mut sent = n;
        sent.id = id;
        let _ = self.sender.send(NotifyEvent::New(sent));
        id
    }

    fn close_notification(&self, id: u32) {
        STORE.lock().unwrap().dismiss(id);
        let _ = self.sender.send(NotifyEvent::Close(id));
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "cyberdeck".into(),
            "cyberdeck".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }
}

/// Spawn the D-Bus notification daemon on a background thread.
/// Returns a calloop Channel receiver for the main event loop.
pub fn spawn_daemon() -> Channel<NotifyEvent> {
    let (sender, receiver) = channel::channel();

    std::thread::Builder::new()
        .name("notif-dbus".into())
        .spawn(move || {
            async_io::block_on(async {
                let _conn = match zbus::connection::Builder::session()
                    .expect("failed to create D-Bus session builder")
                    .name("org.freedesktop.Notifications")
                    .expect("failed to request D-Bus name")
                    .serve_at(
                        "/org/freedesktop/Notifications",
                        NotificationDaemon { sender },
                    )
                    .expect("failed to serve D-Bus interface")
                    .build()
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("failed to connect to D-Bus session bus: {e}");
                        return;
                    }
                };

                log::info!("notification daemon started on D-Bus");

                // Keep connection alive
                std::future::pending::<()>().await;
            });
        })
        .expect("failed to spawn notification daemon thread");

    receiver
}
