use serde_json::{json, Value};

use smithay_client_toolkit::reexports::calloop::channel::Sender;

// ---- D-Bus proxies ----

#[zbus::proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
trait UPower {
    fn enumerate_devices(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}

// UPower device states (from upower-glib enum):
// 1 charging  2 discharging  3 empty  4 fully-charged  5 pending-charge  6 pending-discharge
#[zbus::proxy(
    interface = "org.freedesktop.UPower.Device",
    default_service = "org.freedesktop.UPower"
)]
trait UPowerDevice {
    #[zbus(property)]
    fn percentage(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn time_to_empty(&self) -> zbus::Result<i64>;
    #[zbus(property)]
    fn time_to_full(&self) -> zbus::Result<i64>;
    #[zbus(property, name = "Type")]
    fn device_type(&self) -> zbus::Result<u32>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    fn suspend(&self, interactive: bool) -> zbus::Result<()>;
    fn power_off(&self, interactive: bool) -> zbus::Result<()>;
    fn reboot(&self, interactive: bool) -> zbus::Result<()>;
}

const UPOWER_TYPE_BATTERY: u32 = 2;

fn state_label(state: u32) -> &'static str {
    match state {
        1 => "charging",
        2 => "discharging",
        3 => "empty",
        4 => "fully-charged",
        5 => "pending-charge",
        6 => "pending-discharge",
        _ => "unknown",
    }
}

fn empty() -> Value {
    json!({
        "capacity": 0,
        "status": "unknown",
        "charging": false,
        "time_remaining_secs": 0,
    })
}

async fn battery_path(conn: &zbus::Connection) -> zbus::Result<Option<zbus::zvariant::OwnedObjectPath>> {
    let up = UPowerProxy::new(conn).await?;
    let devs = up.enumerate_devices().await?;
    for path in devs {
        let dev = UPowerDeviceProxy::builder(conn).path(path.as_ref())?.build().await?;
        if dev.device_type().await.unwrap_or(0) == UPOWER_TYPE_BATTERY {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

async fn snapshot(conn: &zbus::Connection) -> Value {
    match snapshot_inner(conn).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("session snapshot failed: {e}");
            empty()
        }
    }
}

async fn snapshot_inner(conn: &zbus::Connection) -> zbus::Result<Value> {
    let Some(path) = battery_path(conn).await? else {
        return Ok(empty());
    };
    let dev = UPowerDeviceProxy::builder(conn).path(path.as_ref())?.build().await?;

    let pct = dev.percentage().await.unwrap_or(0.0).round() as i64;
    let state = dev.state().await.unwrap_or(0);
    let charging = matches!(state, 1 | 4 | 5);
    let time_secs = if charging {
        dev.time_to_full().await.unwrap_or(0)
    } else {
        dev.time_to_empty().await.unwrap_or(0)
    };

    Ok(json!({
        "capacity": pct,
        "status": state_label(state),
        "charging": charging,
        "time_remaining_secs": time_secs,
    }))
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    async_io::block_on(async {
        let conn = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("session poll: system bus connect failed: {e}");
                return empty();
            }
        };
        snapshot(&conn).await
    })
}

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
                log::error!("session subscribe: system bus connect failed: {e}");
                return;
            }
        };

        let val = snapshot(&conn).await;
        if sender.send((id.clone(), val)).is_err() {
            return;
        }

        let path = match battery_path(&conn).await {
            Ok(Some(p)) => p,
            _ => {
                // No battery — slow-poll fallback (e.g. desktop machines).
                loop {
                    async_io::Timer::after(std::time::Duration::from_secs(60)).await;
                    let val = snapshot(&conn).await;
                    if sender.send((id.clone(), val)).is_err() {
                        return;
                    }
                }
            }
        };

        let dev = match UPowerDeviceProxy::builder(&conn).path(path.as_ref()) {
            Ok(b) => match b.build().await {
                Ok(d) => d,
                Err(e) => {
                    log::error!("session subscribe: UPower device proxy failed: {e}");
                    return;
                }
            },
            Err(e) => {
                log::error!("session subscribe: UPower device path invalid: {e}");
                return;
            }
        };

        let mut pct = dev.receive_percentage_changed().await.fuse();
        let mut state = dev.receive_state_changed().await.fuse();
        let mut tte = dev.receive_time_to_empty_changed().await.fuse();
        let mut ttf = dev.receive_time_to_full_changed().await.fuse();

        loop {
            futures_util::select! {
                _ = pct.next() => {}
                _ = state.next() => {}
                _ = tte.next() => {}
                _ = ttf.next() => {}
            };
            async_io::Timer::after(std::time::Duration::from_millis(100)).await;
            let val = snapshot(&conn).await;
            if sender.send((id.clone(), val)).is_err() {
                return;
            }
        }
    });
}

// ---- Actions (logind) ----

fn spawn_login1<F>(label: &'static str, f: F)
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
                    log::error!("session {label}: connect failed: {e}");
                    return;
                }
            };
            if let Err(e) = f(conn).await {
                log::error!("session {label} failed: {e}");
            }
        });
    });
}

pub fn cli_action(name: &str, _args: &[String]) -> Result<String, String> {
    match name {
        "suspend" => {
            log::info!("session: suspend");
            spawn_login1("suspend", |conn| Box::pin(async move {
                let mgr = Login1ManagerProxy::new(&conn).await?;
                mgr.suspend(false).await
            }));
            Ok("suspending".into())
        }
        "shutdown" => {
            log::info!("session: shutdown");
            spawn_login1("shutdown", |conn| Box::pin(async move {
                let mgr = Login1ManagerProxy::new(&conn).await?;
                mgr.power_off(false).await
            }));
            Ok("shutting down".into())
        }
        "reboot" => {
            log::info!("session: reboot");
            spawn_login1("reboot", |conn| Box::pin(async move {
                let mgr = Login1ManagerProxy::new(&conn).await?;
                mgr.reboot(false).await
            }));
            Ok("rebooting".into())
        }
        other => Err(format!("unknown session action: {other}")),
    }
}
