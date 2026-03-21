use serde_json::{json, Value};
use std::process::Command;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    match fetch_weather() {
        Some(v) => v,
        None => default_weather(),
    }
}

fn fetch_weather() -> Option<Value> {
    let output = Command::new("curl")
        .args(["-sf", "wttr.in/?format=j1"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let data: Value = serde_json::from_slice(&output.stdout).ok()?;
    let cc = data.get("current_condition")?.get(0)?;
    let area = data.get("nearest_area")
        .and_then(|a| a.get(0))
        .and_then(|a| a.get("areaName"))
        .and_then(|a| a.get(0))
        .and_then(|a| a.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Some(json!({
        "temp_c": parse_int(cc, "temp_C"),
        "feels_like_c": parse_int(cc, "FeelsLikeC"),
        "humidity": parse_int(cc, "humidity"),
        "desc": cc.get("weatherDesc")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        "wind_kph": parse_int(cc, "windspeedKmph"),
        "code": parse_int(cc, "weatherCode"),
        "location": area,
    }))
}

fn parse_int(obj: &Value, key: &str) -> i64 {
    obj.get(key)
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

fn default_weather() -> Value {
    json!({
        "temp_c": 0,
        "feels_like_c": 0,
        "humidity": 0,
        "desc": "unavailable",
        "wind_kph": 0,
        "code": 0,
        "location": "",
    })
}
