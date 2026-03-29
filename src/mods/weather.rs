use chrono::Timelike;
use serde_json::{json, Value};
use std::process::Command;

pub fn poll(params: &serde_json::Map<String, Value>) -> Value {
    let location = params.get("location").and_then(|v| v.as_str()).unwrap_or("");
    match fetch_weather(location) {
        Some(v) => v,
        None => default_weather(),
    }
}

fn fetch_weather(location: &str) -> Option<Value> {
    let (lat, lon, name) = geocode(location)?;

    let url = format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude={lat}&longitude={lon}\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m\
         &hourly=precipitation\
         &timezone=auto&forecast_days=2"
    );
    let output = Command::new("curl")
        .args(["-sf", &url])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let data: Value = serde_json::from_slice(&output.stdout).ok()?;
    let current = data.get("current")?;

    let code = current.get("weather_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let temp_c = current.get("temperature_2m")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .round() as i64;
    let feels_like = current.get("apparent_temperature")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .round() as i64;
    let humidity = current.get("relative_humidity_2m")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let wind_kph = current.get("wind_speed_10m")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .round() as i64;

    let rainfall = forecast_rainfall(&data);
    let rainfall_max = rainfall.iter().cloned().fold(0.0f64, f64::max);

    Some(json!({
        "temp_c": temp_c,
        "feels_like_c": feels_like,
        "humidity": humidity,
        "wind_kph": wind_kph,
        "code": code,
        "weather_icon": weather_icon(code),
        "weather_label": weather_label(code),
        "temp_icon": temp_icon(temp_c),
        "rainfall": rainfall,
        "rainfall_max": (rainfall_max * 10.0).round() / 10.0,
        "location": name,
    }))
}

fn geocode(location: &str) -> Option<(f64, f64, String)> {
    if location.is_empty() {
        return geolocate_ip();
    }

    // Try parsing as "lat,lon"
    if let Some((lat_s, lon_s)) = location.split_once(',') {
        if let (Ok(lat), Ok(lon)) = (lat_s.trim().parse::<f64>(), lon_s.trim().parse::<f64>()) {
            return Some((lat, lon, format!("{lat:.1},{lon:.1}")));
        }
    }

    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1",
        location
    );
    let output = Command::new("curl")
        .args(["-sf", &url])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let data: Value = serde_json::from_slice(&output.stdout).ok()?;
    let result = data.get("results")?.get(0)?;
    let lat = result.get("latitude")?.as_f64()?;
    let lon = result.get("longitude")?.as_f64()?;
    let name = result.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(location)
        .to_string();

    Some((lat, lon, name))
}

fn geolocate_ip() -> Option<(f64, f64, String)> {
    let output = Command::new("curl")
        .args(["-sf", "https://open-meteo.com/v1/forecast?current=temperature_2m"])
        .output()
        .ok()?;

    // Open-Meteo auto-resolves lat/lon from IP when none provided
    let data: Value = serde_json::from_slice(&output.stdout).ok()?;
    let lat = data.get("latitude")?.as_f64()?;
    let lon = data.get("longitude")?.as_f64()?;

    Some((lat, lon, String::new()))
}

fn weather_icon(code: i64) -> &'static str {
    match code {
        0 => "sun",
        1 | 2 | 3 => "cloud",
        45 | 48 => "cloud-fog",
        51 | 53 | 55 | 56 | 57 => "cloud-rain",
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => "cloud-rain",
        71 | 73 | 75 | 77 | 85 | 86 => "snowflake",
        95 | 96 | 99 => "cloud-lightning",
        _ => "cloud",
    }
}

fn weather_label(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Clear",
        2 => "Cloudy",
        3 => "Overcast",
        45 | 48 => "Foggy",
        51 | 53 | 55 | 56 | 57 => "Drizzle",
        61 | 63 | 65 | 66 | 67 => "Rain",
        71 | 73 | 75 | 77 => "Snow",
        80 | 81 | 82 => "Showers",
        85 | 86 => "Snow",
        95 | 96 | 99 => "Storm",
        _ => "Unknown",
    }
}

fn temp_icon(temp_c: i64) -> &'static str {
    if temp_c <= 0 {
        "thermometer-cold"
    } else if temp_c >= 30 {
        "thermometer-hot"
    } else {
        "thermometer-simple"
    }
}

fn forecast_rainfall(data: &Value) -> Vec<f64> {
    let precip = match data.get("hourly")
        .and_then(|h| h.get("precipitation"))
        .and_then(|p| p.as_array())
    {
        Some(p) => p,
        None => return Vec::new(),
    };

    let all_precip: Vec<f64> = precip.iter()
        .filter_map(|v| v.as_f64())
        .collect();

    // Skip past hours, take next 24
    let skip = chrono::Local::now().hour() as usize;
    all_precip.into_iter().skip(skip).take(24).collect()
}

fn default_weather() -> Value {
    json!({
        "temp_c": 0,
        "feels_like_c": 0,
        "humidity": 0,
        "wind_kph": 0,
        "code": 0,
        "weather_icon": "cloud",
        "weather_label": "Unknown",
        "temp_icon": "thermometer-simple",
        "rainfall": [],
        "rainfall_max": 0.0,
        "location": "",
    })
}
