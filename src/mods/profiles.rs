// corpus:article/architecture

use serde_json::{json, Value};

use smithay_client_toolkit::reexports::calloop::channel::Sender;

#[zbus::proxy(
    interface = "org.freedesktop.UPower.PowerProfiles",
    default_service = "org.freedesktop.UPower.PowerProfiles",
    default_path = "/org/freedesktop/UPower/PowerProfiles"
)]
trait PowerProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_active_profile(&self, value: &str) -> zbus::Result<()>;
}

fn unknown() -> Value {
    json!({ "profile": "unknown" })
}

async fn snapshot(conn: &zbus::Connection) -> Value {
    match PowerProfilesProxy::new(conn).await {
        Ok(p) => match p.active_profile().await {
            Ok(s) => json!({ "profile": s }),
            Err(e) => {
                log::error!("profiles: read active failed: {e}");
                unknown()
            }
        },
        Err(e) => {
            log::error!("profiles: proxy failed: {e}");
            unknown()
        }
    }
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    async_io::block_on(async {
        let conn = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("profiles poll: system bus connect failed: {e}");
                return unknown();
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
                log::error!("profiles subscribe: system bus connect failed: {e}");
                return;
            }
        };

        let val = snapshot(&conn).await;
        if sender.send((id.clone(), val)).is_err() {
            return;
        }

        let proxy = match PowerProfilesProxy::new(&conn).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("profiles subscribe: proxy failed: {e}");
                return;
            }
        };

        let mut stream = proxy.receive_active_profile_changed().await;
        while stream.next().await.is_some() {
            let val = snapshot(&conn).await;
            if sender.send((id.clone(), val)).is_err() {
                return;
            }
        }
    });
}

pub fn cli_action(name: &str, _args: &[String]) -> Result<String, String> {
    let target = match name {
        "power-saver" | "saver" => "power-saver",
        "balanced" => "balanced",
        "performance" => "performance",
        other => return Err(format!("unknown profile: {other}")),
    };
    log::info!("profiles: set {target}");
    let target_owned = target.to_string();
    std::thread::spawn(move || {
        async_io::block_on(async move {
            let conn = match zbus::Connection::system().await {
                Ok(c) => c,
                Err(e) => {
                    log::error!("profiles set: connect failed: {e}");
                    return;
                }
            };
            let proxy = match PowerProfilesProxy::new(&conn).await {
                Ok(p) => p,
                Err(e) => {
                    log::error!("profiles set: proxy failed: {e}");
                    return;
                }
            };
            if let Err(e) = proxy.set_active_profile(&target_owned).await {
                log::error!("profiles set {target_owned} failed: {e}");
            }
        });
    });
    Ok(target.into())
}
