use serde_json::{json, Value};
use std::process::Command;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let disconnected = json!({
        "connected": false,
        "type": "",
        "ssid": "",
        "signal": 0,
        "ip": "",
    });

    let Ok(dev_out) = Command::new("nmcli")
        .args(["-t", "-f", "TYPE,STATE,CONNECTION", "device"])
        .output()
    else {
        return disconnected;
    };
    let dev_str = String::from_utf8_lossy(&dev_out.stdout);

    // Check wifi first
    let wifi = dev_str
        .lines()
        .find(|l| l.starts_with("wifi:connected:"));

    if let Some(line) = wifi {
        let ssid = line.splitn(3, ':').nth(2).unwrap_or("").to_string();
        let signal = get_wifi_signal();
        let ip = get_ip();
        return json!({
            "connected": true,
            "type": "wifi",
            "ssid": ssid,
            "signal": signal,
            "ip": ip,
        });
    }

    // Check ethernet
    let eth = dev_str
        .lines()
        .any(|l| l.starts_with("ethernet:connected"));

    if eth {
        let ip = get_ip();
        return json!({
            "connected": true,
            "type": "ethernet",
            "ssid": "",
            "signal": 100,
            "ip": ip,
        });
    }

    disconnected
}

fn get_wifi_signal() -> i64 {
    Command::new("nmcli")
        .args(["-t", "-f", "IN-USE,SIGNAL", "device", "wifi", "list"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            text.lines()
                .find(|l| l.starts_with('*'))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|s| s.trim().parse::<i64>().ok())
        })
        .unwrap_or(0)
}

fn get_ip() -> String {
    Command::new("nmcli")
        .args(["-t", "-f", "IP4.ADDRESS", "device", "show"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            text.lines()
                .find(|l| l.contains("IP4.ADDRESS"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.split('/').next().unwrap_or("").to_string())
        })
        .unwrap_or_default()
}
