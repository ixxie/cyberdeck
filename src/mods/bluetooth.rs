use serde_json::{json, Value};
use std::collections::HashMap;

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

// -- D-Bus proxies for BlueZ --

#[zbus::proxy(
    interface = "org.bluez.Adapter1",
    default_service = "org.bluez"
)]
trait Adapter1 {
    #[zbus(property)]
    fn powered(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_powered(&self, value: bool) -> zbus::Result<()>;

    fn start_discovery(&self) -> zbus::Result<()>;
    fn stop_discovery(&self) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.bluez.Device1",
    default_service = "org.bluez"
)]
trait Device1 {
    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn address(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn connected(&self) -> zbus::Result<bool>;

    fn connect(&self) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.DBus.ObjectManager",
    default_service = "org.bluez",
    default_path = "/"
)]
trait ObjectManager {
    fn get_managed_objects(
        &self,
    ) -> zbus::Result<HashMap<
        zbus::zvariant::OwnedObjectPath,
        HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>,
    >>;

    #[zbus(signal)]
    fn interfaces_added(
        &self,
        path: zbus::zvariant::OwnedObjectPath,
        ifaces: HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    fn interfaces_removed(
        &self,
        path: zbus::zvariant::OwnedObjectPath,
        ifaces: Vec<String>,
    ) -> zbus::Result<()>;
}

// -- Snapshot --

fn empty() -> Value {
    json!({ "powered": false, "devices": [] })
}

async fn snapshot(conn: &zbus::Connection) -> Value {
    match snapshot_inner(conn).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("bluetooth snapshot failed: {e}");
            empty()
        }
    }
}

async fn snapshot_inner(conn: &zbus::Connection) -> zbus::Result<Value> {
    let om = ObjectManagerProxy::new(conn).await?;
    let objects = om.get_managed_objects().await?;

    let mut powered = false;
    let mut devices: Vec<Value> = Vec::new();

    for (path, ifaces) in &objects {
        if ifaces.contains_key("org.bluez.Adapter1") {
            if let Ok(adapter) = Adapter1Proxy::builder(conn).path(path.as_ref())?.build().await {
                if adapter.powered().await.unwrap_or(false) {
                    powered = true;
                }
            }
        }
        if ifaces.contains_key("org.bluez.Device1") {
            if let Ok(dev) = Device1Proxy::builder(conn).path(path.as_ref())?.build().await {
                let name = dev.name().await.unwrap_or_default();
                let mac = dev.address().await.unwrap_or_default();
                let connected = dev.connected().await.unwrap_or(false);
                if name.is_empty() && mac.is_empty() {
                    continue;
                }
                devices.push(json!({
                    "mac": mac,
                    "name": name,
                    "connected": connected,
                    "path": path.as_str(),
                }));
            }
        }
    }

    // Connected first, then alphabetical
    devices.sort_by(|a, b| {
        let ac = a.get("connected").and_then(|v| v.as_bool()).unwrap_or(false);
        let bc = b.get("connected").and_then(|v| v.as_bool()).unwrap_or(false);
        bc.cmp(&ac).then_with(|| {
            let an = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let bn = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            an.cmp(bn)
        })
    });

    Ok(json!({
        "powered": powered,
        "devices": devices,
    }))
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    async_io::block_on(async {
        let conn = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("bluetooth poll: failed to connect to system bus: {e}");
                return empty();
            }
        };
        snapshot(&conn).await
    })
}

// -- Subscribe: event-driven updates via PropertiesChanged + InterfacesAdded/Removed --

pub fn subscribe(
    _params: serde_json::Map<String, Value>,
    sender: Sender<(String, Value)>,
    id: String,
) {
    use futures_util::StreamExt;

    async_io::block_on(async {
        let conn = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("bluetooth: failed to connect to system bus: {e}");
                return;
            }
        };

        // Initial snapshot
        let val = snapshot(&conn).await;
        if sender.send((id.clone(), val)).is_err() {
            return;
        }

        let om = match ObjectManagerProxy::new(&conn).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("bluetooth: failed to create ObjectManager proxy: {e}");
                return;
            }
        };

        let mut added = match om.receive_interfaces_added().await {
            Ok(s) => s.fuse(),
            Err(e) => {
                log::error!("bluetooth: failed to subscribe to InterfacesAdded: {e}");
                return;
            }
        };
        let mut removed = match om.receive_interfaces_removed().await {
            Ok(s) => s.fuse(),
            Err(e) => {
                log::error!("bluetooth: failed to subscribe to InterfacesRemoved: {e}");
                return;
            }
        };

        // Watch PropertiesChanged on all org.bluez paths.
        // dbus match rule: type=signal, sender=org.bluez, interface=org.freedesktop.DBus.Properties, member=PropertiesChanged
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.bluez").unwrap()
            .interface("org.freedesktop.DBus.Properties").unwrap()
            .member("PropertiesChanged").unwrap()
            .build();
        let mut props = match zbus::MessageStream::for_match_rule(rule, &conn, None).await {
            Ok(s) => s.fuse(),
            Err(e) => {
                log::error!("bluetooth: failed to subscribe to PropertiesChanged: {e}");
                return;
            }
        };

        loop {
            futures_util::select! {
                _ = added.next() => {}
                _ = removed.next() => {}
                _ = props.next() => {}
            };

            // Debounce — bluez often emits clusters of property changes
            async_io::Timer::after(std::time::Duration::from_millis(150)).await;

            let val = snapshot(&conn).await;
            if sender.send((id.clone(), val)).is_err() {
                return;
            }
        }
    });
}

// -- Action helpers (run off the bar thread; D-Bus calls can block on real BT IO) --

fn spawn_action<F>(label: &'static str, f: F)
where
    F: FnOnce(zbus::Connection) -> std::pin::Pin<Box<dyn std::future::Future<Output = zbus::Result<()>> + Send>>
        + Send
        + 'static,
{
    std::thread::spawn(move || {
        async_io::block_on(async move {
            let conn = match zbus::Connection::system().await {
                Ok(c) => c,
                Err(e) => {
                    log::error!("bluetooth {label}: connect failed: {e}");
                    return;
                }
            };
            if let Err(e) = f(conn).await {
                log::error!("bluetooth {label} failed: {e}");
            }
        });
    });
}

fn connect_device(path: String) {
    log::info!("bluetooth: connect {path}");
    spawn_action("connect", |conn| Box::pin(async move {
        let dev = Device1Proxy::builder(&conn)
            .path(path.as_str())?
            .build()
            .await?;
        dev.connect().await
    }));
}

fn disconnect_device(path: String) {
    log::info!("bluetooth: disconnect {path}");
    spawn_action("disconnect", |conn| Box::pin(async move {
        let dev = Device1Proxy::builder(&conn)
            .path(path.as_str())?
            .build()
            .await?;
        dev.disconnect().await
    }));
}

fn start_discovery() {
    log::info!("bluetooth: scan");
    spawn_action("scan", |conn| Box::pin(async move {
        let adapter = Adapter1Proxy::builder(&conn)
            .path("/org/bluez/hci0")?
            .build()
            .await?;
        adapter.start_discovery().await?;
        // BlueZ ties the discovery session to this D-Bus connection and kills
        // it the moment the connection drops, so hold it open for the scan
        // window before stopping cleanly.
        async_io::Timer::after(std::time::Duration::from_secs(60)).await;
        adapter.stop_discovery().await
    }));
}

fn set_powered(on: bool) {
    log::info!("bluetooth: power {}", if on { "on" } else { "off" });
    spawn_action(if on { "power on" } else { "power off" }, move |conn| Box::pin(async move {
        let adapter = Adapter1Proxy::builder(&conn)
            .path("/org/bluez/hci0")?
            .build()
            .await?;
        adapter.set_powered(on).await
    }));
}

/// Disconnect every currently-connected device.
fn disconnect_all() {
    log::info!("bluetooth: disconnect all");
    spawn_action("disconnect all", |conn| Box::pin(async move {
        let om = ObjectManagerProxy::new(&conn).await?;
        let objects = om.get_managed_objects().await?;
        for (path, ifaces) in &objects {
            if !ifaces.contains_key("org.bluez.Device1") { continue; }
            if let Ok(dev) = Device1Proxy::builder(&conn).path(path.as_ref())?.build().await {
                if dev.connected().await.unwrap_or(false) {
                    if let Err(e) = dev.disconnect().await {
                        log::warn!("bluetooth: disconnect {} failed: {e}", path.as_str());
                    }
                }
            }
        }
        Ok(())
    }));
}

/// Look up a device path by MAC address (case-insensitive). Synchronous wrapper
/// around the async D-Bus query — returns the first matching path or None.
fn path_for_address(addr: &str) -> Option<String> {
    let addr_norm = addr.to_uppercase();
    async_io::block_on(async {
        let conn = zbus::Connection::system().await.ok()?;
        let om = ObjectManagerProxy::new(&conn).await.ok()?;
        let objects = om.get_managed_objects().await.ok()?;
        for (path, ifaces) in &objects {
            if !ifaces.contains_key("org.bluez.Device1") { continue; }
            if let Ok(dev) = Device1Proxy::builder(&conn).path(path.as_ref()).ok()?.build().await {
                if dev.address().await.ok().map(|a| a.to_uppercase()) == Some(addr_norm.clone()) {
                    return Some(path.as_str().to_string());
                }
            }
        }
        None
    })
}

/// CLI/IPC action dispatcher. Returns toast text describing what happened.
pub fn cli_action(name: &str, args: &[String]) -> Result<String, String> {
    match name {
        "on" => { set_powered(true); Ok("power on".into()) }
        "off" => { set_powered(false); Ok("power off".into()) }
        "scan" => { start_discovery(); Ok("scanning".into()) }
        "disconnect" => match args.first() {
            None => { disconnect_all(); Ok("disconnect all".into()) }
            Some(addr) => match path_for_address(addr) {
                Some(p) => { disconnect_device(p); Ok(format!("disconnect {addr}")) }
                None => Err(format!("device not found: {addr}")),
            },
        },
        "connect" => match args.first() {
            None => Err("connect requires a MAC address".into()),
            Some(addr) => match path_for_address(addr) {
                Some(p) => { connect_device(p); Ok(format!("connect {addr}")) }
                None => Err(format!("device not found: {addr}")),
            },
        },
        other => Err(format!("unknown bluetooth action: {other}")),
    }
}

// -- Deep module --

pub struct BluetoothDeep {
    cursor: usize,
}

impl BluetoothDeep {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    fn device_count(data: &Value) -> usize {
        data.get("devices")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    }
}

impl InteractiveModule for BluetoothDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let highlight_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let powered = data.get("powered").and_then(|v| v.as_bool()).unwrap_or(false);
        if !powered {
            return vec![vec![Elem::text("off").fg(idle_fg)]];
        }

        let devices = data.get("devices").and_then(|v| v.as_array());
        let devices = match devices {
            Some(d) if !d.is_empty() => d,
            _ => return vec![vec![Elem::text("no devices").fg(idle_fg)]],
        };

        devices.iter().enumerate().map(|(i, dev)| {
            let name = dev.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let connected = dev.get("connected").and_then(|v| v.as_bool()).unwrap_or(false);
            let dev_fg = if i == self.cursor {
                fg
            } else if connected {
                highlight_fg
            } else {
                idle_fg
            };
            let prefix = if connected { "●" } else { "○" };
            vec![Elem::text(format!("{prefix} {name}")).fg(dev_fg)]
        }).collect()
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "(dis)connect".into(), icon: None },
            KeyHintDef { key: "s".into(), action: String::new(), label: "scan".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let count = Self::device_count(data);

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
                if let Some(devices) = data.get("devices").and_then(|v| v.as_array()) {
                    if let Some(dev) = devices.get(self.cursor) {
                        let path = dev.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let connected = dev.get("connected").and_then(|v| v.as_bool()).unwrap_or(false);
                        if !path.is_empty() {
                            if connected {
                                disconnect_device(path.to_string());
                            } else {
                                connect_device(path.to_string());
                            }
                        }
                    }
                }
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("s") => {
                start_discovery();
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn activate(&mut self, data: &Value, _sub_path: &[String]) {
        // Position cursor on the first connected device, falling back to the
        // first device. Without this the cursor sticks across deep-view
        // openings and may point at a stale (out-of-bounds) index after the
        // device list re-sorts on connection state changes.
        let count = Self::device_count(data);
        if count == 0 {
            self.cursor = 0;
            return;
        }
        let first_connected = data
            .get("devices")
            .and_then(|v| v.as_array())
            .and_then(|devs| {
                devs.iter().position(|d| {
                    d.get("connected").and_then(|v| v.as_bool()).unwrap_or(false)
                })
            });
        self.cursor = first_connected.unwrap_or(0).min(count - 1);
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}
