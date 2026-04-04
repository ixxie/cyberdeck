use serde_json::{json, Value};
use std::collections::HashSet;
use std::process::Command;
use std::time::Instant;

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

// -- D-Bus proxy definitions for NetworkManager --

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn active_connections(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;

    fn get_devices(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmDevice {
    #[zbus(property)]
    fn device_type(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn ip4_config(&self) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    #[zbus(property)]
    fn active_connection(&self) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmWireless {
    #[zbus(property)]
    fn access_points(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;

    #[zbus(property)]
    fn last_scan(&self) -> zbus::Result<i64>;

    fn request_scan(
        &self,
        options: std::collections::HashMap<String, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.AccessPoint",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmAccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;

    #[zbus(property)]
    fn strength(&self) -> zbus::Result<u8>;

    #[zbus(property)]
    fn wpa_flags(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn rsn_flags(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn frequency(&self) -> zbus::Result<u32>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Connection.Active",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmActiveConnection {
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn r#type(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.IP4Config",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmIp4Config {
    #[zbus(property)]
    fn address_data(&self) -> zbus::Result<Vec<std::collections::HashMap<String, zbus::zvariant::OwnedValue>>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
trait NmSettings {
    fn list_connections(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager"
)]
trait NmSettingsConnection {
    fn get_settings(
        &self,
    ) -> zbus::Result<std::collections::HashMap<String, std::collections::HashMap<String, zbus::zvariant::OwnedValue>>>;
}

// NM device type constants
const NM_DEVICE_TYPE_ETHERNET: u32 = 1;
const NM_DEVICE_TYPE_WIFI: u32 = 2;

// NM device state: activated
const NM_DEVICE_STATE_ACTIVATED: u32 = 100;

// -- Snapshot: build full network state from D-Bus --

async fn snapshot(conn: &zbus::Connection) -> Value {
    match snapshot_inner(conn).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("network snapshot failed: {e}");
            disconnected()
        }
    }
}

fn disconnected() -> Value {
    json!({
        "connected": false,
        "type": "",
        "ssid": "",
        "signal": 0,
        "ip": "",
        "networks": [],
    })
}

async fn snapshot_inner(conn: &zbus::Connection) -> zbus::Result<Value> {
    let nm = NetworkManagerProxy::new(conn).await?;
    let devices = nm.get_devices().await?;

    let mut conn_type = "";
    let mut ssid = String::new();
    let mut signal: u8 = 0;
    let mut ip = String::new();
    let mut networks: Vec<Value> = Vec::new();
    let mut connected = false;
    let mut active_ssid = String::new();

    for dev_path in &devices {
        let dev = NmDeviceProxy::builder(conn)
            .path(dev_path.as_ref())?
            .build()
            .await?;

        let dev_type = dev.device_type().await.unwrap_or(0);
        let dev_state = dev.state().await.unwrap_or(0);

        match dev_type {
            NM_DEVICE_TYPE_WIFI => {
                // Gather access points regardless of connection state
                let wireless = NmWirelessProxy::builder(conn)
                    .path(dev_path.as_ref())?
                    .build()
                    .await?;

                let ap_paths = wireless.access_points().await.unwrap_or_default();
                let mut seen = HashSet::new();

                for ap_path in &ap_paths {
                    let ap = NmAccessPointProxy::builder(conn)
                        .path(ap_path.as_ref())?
                        .build()
                        .await?;

                    let ssid_bytes = ap.ssid().await.unwrap_or_default();
                    let ap_ssid = String::from_utf8_lossy(&ssid_bytes).to_string();
                    if ap_ssid.is_empty() || !seen.insert(ap_ssid.clone()) {
                        continue;
                    }

                    let ap_signal = ap.strength().await.unwrap_or(0);
                    // Skip stale APs (signal 0 = no longer reachable)
                    if ap_signal == 0 {
                        continue;
                    }
                    let wpa = ap.wpa_flags().await.unwrap_or(0);
                    let rsn = ap.rsn_flags().await.unwrap_or(0);
                    let secured = wpa != 0 || rsn != 0;
                    let in_use = !active_ssid.is_empty() && ap_ssid == active_ssid;

                    networks.push(json!({
                        "ssid": ap_ssid,
                        "signal": ap_signal,
                        "security": if secured { "secured" } else { "" },
                        "in_use": in_use,
                    }));
                }

                if dev_state == NM_DEVICE_STATE_ACTIVATED {
                    connected = true;
                    conn_type = "wifi";

                    // Get active connection info
                    if let Ok(ac_path) = dev.active_connection().await {
                        if let Ok(ac) = NmActiveConnectionProxy::builder(conn)
                            .path(ac_path.as_ref())?
                            .build()
                            .await
                        {
                            ssid = ac.id().await.unwrap_or_default();
                            active_ssid = ssid.clone();
                        }
                    }

                    // Get signal for active AP
                    if let Ok(wireless2) = NmWirelessProxy::builder(conn)
                        .path(dev_path.as_ref())?
                        .build()
                        .await
                    {
                        // Find the AP matching our SSID
                        for ap_path in &ap_paths {
                            if let Ok(ap) = NmAccessPointProxy::builder(conn)
                                .path(ap_path.as_ref())?
                                .build()
                                .await
                            {
                                let ab = ap.ssid().await.unwrap_or_default();
                                let s = String::from_utf8_lossy(&ab);
                                if s == ssid {
                                    signal = ap.strength().await.unwrap_or(0);
                                    break;
                                }
                            }
                        }
                        let _ = wireless2;
                    }

                    // Get IP
                    if let Ok(ip_path) = dev.ip4_config().await {
                        ip = get_ip_from_config(conn, &ip_path).await;
                    }
                }
            }
            NM_DEVICE_TYPE_ETHERNET if dev_state == NM_DEVICE_STATE_ACTIVATED => {
                connected = true;
                conn_type = "ethernet";
                signal = 100;

                if let Ok(ip_path) = dev.ip4_config().await {
                    ip = get_ip_from_config(conn, &ip_path).await;
                }
            }
            _ => {}
        }
    }

    // Fix in_use flags now that we know the active SSID
    if !active_ssid.is_empty() {
        for net in &mut networks {
            if let Some(s) = net.get("ssid").and_then(|v| v.as_str()) {
                if s == active_ssid {
                    net.as_object_mut().unwrap().insert("in_use".into(), json!(true));
                }
            }
        }
    }

    // Sort: in_use first, then by signal descending
    networks.sort_by(|a, b| {
        let a_use = a.get("in_use").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_use = b.get("in_use").and_then(|v| v.as_bool()).unwrap_or(false);
        b_use.cmp(&a_use).then_with(|| {
            let a_sig = a.get("signal").and_then(|v| v.as_u64()).unwrap_or(0);
            let b_sig = b.get("signal").and_then(|v| v.as_u64()).unwrap_or(0);
            b_sig.cmp(&a_sig)
        })
    });

    Ok(json!({
        "connected": connected,
        "type": conn_type,
        "ssid": ssid,
        "signal": signal,
        "ip": ip,
        "networks": networks,
    }))
}

async fn get_ip_from_config(
    conn: &zbus::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> String {
    let inner = async {
        let proxy = NmIp4ConfigProxy::builder(conn)
            .path(path.as_ref())?
            .build()
            .await?;
        let addrs = proxy.address_data().await?;
        if let Some(first) = addrs.first() {
            if let Some(addr_val) = first.get("address") {
                if let Ok(cloned) = addr_val.try_clone() {
                    if let Ok(s) = <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(cloned) {
                        return Ok::<String, zbus::Error>(s);
                    }
                }
            }
        }
        Ok(String::new())
    };
    inner.await.unwrap_or_default()
}

async fn is_known_network_dbus(conn: &zbus::Connection, target_ssid: &str) -> bool {
    let Ok(settings) = NmSettingsProxy::new(conn).await else {
        return false;
    };
    let Ok(conns) = settings.list_connections().await else {
        return false;
    };

    for conn_path in &conns {
        let Ok(builder) = NmSettingsConnectionProxy::builder(conn)
            .path(conn_path.as_ref())
        else {
            continue;
        };
        let Ok(sc) = builder.build().await else {
            continue;
        };
        let Ok(settings_map) = sc.get_settings().await else {
            continue;
        };
        if let Some(wifi_settings) = settings_map.get("802-11-wireless") {
            if let Some(ssid_val) = wifi_settings.get("ssid") {
                if let Ok(cloned) = ssid_val.try_clone() {
                    if let Ok(ssid_bytes) = <Vec<u8> as TryFrom<zbus::zvariant::OwnedValue>>::try_from(cloned) {
                        let s = String::from_utf8_lossy(&ssid_bytes);
                        if s == target_ssid {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// -- Subscribe: event-driven updates via D-Bus property watching --

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
                log::error!("network: failed to connect to system bus: {e}");
                return;
            }
        };

        // Initial snapshot
        let val = snapshot(&conn).await;
        if sender.send((id.clone(), val)).is_err() {
            return;
        }

        let nm = match NetworkManagerProxy::new(&conn).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("network: failed to create NM proxy: {e}");
                return;
            }
        };

        // Watch NM state and active connections changes
        let mut state_stream = nm.receive_state_changed().await.fuse();
        let mut ac_stream = nm.receive_active_connections_changed().await.fuse();

        // Find wifi device and watch LastScan (fires on scan completion / new APs)
        let wifi_path: Option<zbus::zvariant::OwnedObjectPath> = {
            let mut found = None;
            if let Ok(devs) = nm.get_devices().await {
                for dev_path in devs {
                    if let Ok(b) = NmDeviceProxy::builder(&conn).path(dev_path.as_ref()) {
                        if let Ok(dev) = b.build().await {
                            if dev.device_type().await.unwrap_or(0) == NM_DEVICE_TYPE_WIFI {
                                found = Some(dev_path);
                                break;
                            }
                        }
                    }
                }
            }
            found
        };
        let wifi_device = match &wifi_path {
            Some(p) => {
                match NmWirelessProxy::builder(&conn).path(p.as_ref()) {
                    Ok(b) => b.build().await.ok(),
                    Err(_) => None,
                }
            }
            None => None,
        };

        // last_scan stream is optional (no wifi device = no stream)
        let mut scan_stream = match &wifi_device {
            Some(w) => Some(w.receive_last_scan_changed().await.fuse()),
            None => None,
        };

        loop {
            // Wait for any property change, then re-snapshot
            match &mut scan_stream {
                Some(ss) => {
                    futures_util::select! {
                        _ = state_stream.next() => {}
                        _ = ac_stream.next() => {}
                        _ = ss.next() => {}
                    };
                }
                None => {
                    futures_util::select! {
                        _ = state_stream.next() => {}
                        _ = ac_stream.next() => {}
                    };
                }
            }

            // Small debounce — multiple signals often fire together
            async_io::Timer::after(std::time::Duration::from_millis(100)).await;

            let val = snapshot(&conn).await;
            if sender.send((id.clone(), val)).is_err() {
                return;
            }
        }
    });
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    // Synchronous poll used for initial seed; does a blocking D-Bus snapshot
    async_io::block_on(async {
        let conn = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("network poll: failed to connect to system bus: {e}");
                return disconnected();
            }
        };
        snapshot(&conn).await
    })
}

// -- Deep module --

enum NetState {
    Browse,
    Connecting { ssid: String },
    Password { ssid: String, input: String },
}

const SCAN_TIMEOUT_SECS: u64 = 10;

pub struct NetworkDeep {
    cursor: usize,
    state: NetState,
    scan_started: Option<Instant>,
}

impl NetworkDeep {
    pub fn new() -> Self {
        Self {
            cursor: 0,
            state: NetState::Browse,
            scan_started: None,
        }
    }

    fn network_count(data: &Value) -> usize {
        data.get("networks")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    }

    fn is_known_network(ssid: &str) -> bool {
        // Synchronous check via D-Bus
        async_io::block_on(async {
            let conn = match zbus::Connection::system().await {
                Ok(c) => c,
                Err(_) => return false,
            };
            is_known_network_dbus(&conn, ssid).await
        })
    }
}

impl InteractiveModule for NetworkDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        if let NetState::Password { ssid, input } = &self.state {
            let dots = "\u{2022}".repeat(input.len());
            return vec![vec![
                Elem::text(format!("{ssid} password: {dots}\u{258e}")).fg(fg),
            ]];
        }

        // Connecting: block interaction until done
        if let NetState::Connecting { ssid } = &self.state {
            let connected = data
                .get("networks")
                .and_then(|v| v.as_array())
                .and_then(|nets| {
                    nets.iter().find(|n| {
                        n.get("ssid").and_then(|v| v.as_str()) == Some(ssid.as_str())
                            && n.get("in_use").and_then(|v| v.as_bool()).unwrap_or(false)
                    })
                })
                .is_some();
            if !connected {
                return vec![vec![
                    Elem::text(format!("connecting to {ssid}...")).fg(active_fg),
                ]];
            }
        }

        let scanning = self.scan_started
            .map(|t| t.elapsed().as_secs() < SCAN_TIMEOUT_SECS)
            .unwrap_or(false);

        let mut rows: Vec<Vec<Elem>> = Vec::new();

        let networks = data.get("networks").and_then(|v| v.as_array());
        let networks = match networks {
            Some(n) if !n.is_empty() => n,
            _ => {
                if rows.is_empty() {
                    rows.push(vec![Elem::text("no networks").fg(idle_fg)]);
                }
                return rows;
            }
        };

        for (i, net) in networks.iter().enumerate() {
            let ssid = net.get("ssid").and_then(|v| v.as_str()).unwrap_or("?");
            let signal = net.get("signal").and_then(|v| v.as_u64()).unwrap_or(0);
            let in_use = net
                .get("in_use")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let secured = net
                .get("security")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                != "";

            let selected = i == self.cursor;
            let net_fg = if selected {
                fg
            } else if in_use {
                active_fg
            } else {
                idle_fg
            };

            let lock = if secured { "\u{1f512}" } else { "" };
            let prefix = if selected {
                "\u{25b8}"
            } else if in_use {
                "\u{25cf}"
            } else {
                "\u{25cb}"
            };
            rows.push(vec![Elem::text(format!("{prefix} {ssid} {signal}% {lock}")).fg(net_fg)]);
        }

        if scanning {
            rows.push(vec![Elem::text("scanning...").fg(idle_fg)]);
        }

        rows
    }

    fn cursor(&self) -> Option<usize> {
        Some(self.cursor)
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        match &self.state {
            NetState::Password { .. } => vec![KeyHintDef {
                key: "Esc".into(),
                action: String::new(),
                label: "cancel".into(),
                icon: None,
            }],
            NetState::Connecting { .. } => vec![],
            NetState::Browse => vec![KeyHintDef {
                key: "s".into(),
                action: String::new(),
                label: "scan".into(),
                icon: None,
            }],
        }
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        match &mut self.state {
            NetState::Password { ssid, input } => match event.keysym {
                Keysym::Escape => {
                    self.state = NetState::Browse;
                    KeyResult::Handled
                }
                Keysym::BackSpace => {
                    input.pop();
                    KeyResult::Handled
                }
                Keysym::Return => {
                    if !input.is_empty() {
                        let s = ssid.clone();
                        let p = input.clone();
                        // Safe argument passing — no shell interpolation
                        std::thread::spawn(move || {
                            let _ = Command::new("nmcli")
                                .args(["device", "wifi", "connect", &s, "password", &p])
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status();
                        });
                        let connecting_ssid = ssid.clone();
                        self.state = NetState::Connecting {
                            ssid: connecting_ssid,
                        };
                    }
                    KeyResult::Action
                }
                _ => {
                    if let Some(s) = &event.utf8 {
                        if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                            input.push_str(s);
                        }
                    }
                    KeyResult::Handled
                }
            },
            NetState::Connecting { .. } => {
                if event.keysym == Keysym::Escape {
                    self.state = NetState::Browse;
                    return KeyResult::Handled;
                }
                KeyResult::Ignored
            }
            NetState::Browse => {
                let count = Self::network_count(data);

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
                        if let Some(networks) = data.get("networks").and_then(|v| v.as_array()) {
                            if let Some(net) = networks.get(self.cursor) {
                                let ssid =
                                    net.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
                                let in_use = net
                                    .get("in_use")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let secured = net
                                    .get("security")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    != "";

                                if !ssid.is_empty() {
                                    if in_use {
                                        // Disconnect — safe args
                                        let s = ssid.to_string();
                                        std::thread::spawn(move || {
                                            let _ = Command::new("nmcli")
                                                .args(["connection", "down", "id", &s])
                                                .stdin(std::process::Stdio::null())
                                                .stdout(std::process::Stdio::null())
                                                .stderr(std::process::Stdio::null())
                                                .status();
                                        });
                                    } else if secured && !Self::is_known_network(ssid) {
                                        self.state = NetState::Password {
                                            ssid: ssid.to_string(),
                                            input: String::new(),
                                        };
                                    } else {
                                        // Connect to known or open network — safe args
                                        let s = ssid.to_string();
                                        std::thread::spawn(move || {
                                            let _ = Command::new("nmcli")
                                                .args([
                                                    "device", "wifi", "connect", &s,
                                                ])
                                                .stdin(std::process::Stdio::null())
                                                .stdout(std::process::Stdio::null())
                                                .stderr(std::process::Stdio::null())
                                                .status();
                                        });
                                        self.state = NetState::Connecting {
                                            ssid: ssid.to_string(),
                                        };
                                    }
                                }
                            }
                        }
                        KeyResult::Action
                    }
                    _ if event.utf8.as_deref() == Some("s") => {
                        self.scan_started = Some(Instant::now());
                        std::thread::spawn(|| {
                            let _ = Command::new("nmcli")
                                .args(["device", "wifi", "rescan"])
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status();
                        });
                        KeyResult::Action
                    }
                    _ => KeyResult::Ignored,
                }
            }
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
        self.state = NetState::Browse;
        self.scan_started = None;
    }
}
