use serde_json::{json, Value};
use std::fs;
use std::process::Command;

fn read_sysfs_battery() -> (i64, String, i64) {
    let bat_dirs: Vec<_> = fs::read_dir("/sys/class/power_supply/")
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("BAT")
        })
        .collect();

    let Some(bat) = bat_dirs.first() else {
        return (0, "Unknown".into(), 0);
    };

    let path = bat.path();
    let capacity = fs::read_to_string(path.join("capacity"))
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let status = fs::read_to_string(path.join("status"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "Unknown".into());

    // Estimate time remaining from energy_now and power_now
    let energy_now = fs::read_to_string(path.join("energy_now"))
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok());
    let power_now = fs::read_to_string(path.join("power_now"))
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok());

    let time_secs = match (energy_now, power_now) {
        (Some(e), Some(p)) if p > 0.0 => ((e / p) * 3600.0) as i64,
        _ => 0,
    };

    (capacity, status, time_secs)
}

fn upower_battery() -> Option<(i64, String, i64)> {
    let list = Command::new("upower").arg("-e").output().ok()?;
    let list_str = String::from_utf8_lossy(&list.stdout);
    let bat = list_str.lines().find(|l| l.contains("battery"))?;

    let info = Command::new("upower").arg("-i").arg(bat).output().ok()?;
    let info_str = String::from_utf8_lossy(&info.stdout);

    let capacity = info_str
        .lines()
        .find(|l| l.contains("percentage"))
        .and_then(|l| {
            l.chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<i64>()
                .ok()
        })
        .unwrap_or(0);

    let status = info_str
        .lines()
        .find(|l| l.contains("state:"))
        .map(|l| l.split_whitespace().last().unwrap_or("Unknown").to_string())
        .unwrap_or_else(|| "Unknown".into());

    // Parse "time to empty/full" line
    let time_secs = info_str
        .lines()
        .find(|l| l.contains("time to"))
        .map(|l| {
            let mut secs: i64 = 0;
            let parts: Vec<&str> = l.split_whitespace().collect();
            for w in parts.windows(2) {
                if let Ok(v) = w[0].parse::<f64>() {
                    if w[1].starts_with("hour") {
                        secs += (v * 3600.0) as i64;
                    } else if w[1].starts_with("minute") {
                        secs += (v * 60.0) as i64;
                    }
                }
            }
            secs
        })
        .unwrap_or(0);

    Some((capacity, status, time_secs))
}

fn get_profile() -> String {
    Command::new("powerprofilesctl")
        .arg("get")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let (capacity, status, time_secs) = upower_battery()
        .unwrap_or_else(|| read_sysfs_battery());

    let charging = status == "charging" || status == "fully-charged";
    let profile = get_profile();

    json!({
        "capacity": capacity,
        "status": status,
        "charging": charging,
        "time_remaining_secs": time_secs,
        "profile": profile,
    })
}
