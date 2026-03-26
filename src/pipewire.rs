use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

const CACHE_TTL: Duration = Duration::from_millis(500);

static CACHE: Mutex<Option<(Instant, PwState)>> = Mutex::new(None);

#[derive(Clone, Debug)]
pub struct PwDevice {
    pub id: u32,
    pub name: String,
    pub volume: i64,
    pub muted: bool,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct PwState {
    pub outputs: Vec<PwDevice>,
    pub inputs: Vec<PwDevice>,
    pub denoise: bool,
}

pub fn invalidate() {
    *CACHE.lock().unwrap() = None;
}

pub fn query() -> PwState {
    let mut cache = CACHE.lock().unwrap();
    if let Some((t, ref state)) = *cache {
        if t.elapsed() < CACHE_TTL {
            return state.clone();
        }
    }
    let state = do_query();
    *cache = Some((Instant::now(), state.clone()));
    state
}

fn do_query() -> PwState {
    let dump = pw_dump();

    let default_sink = default_device_name(&dump, "default.audio.sink");
    let default_source = default_device_name(&dump, "default.audio.source");

    let mut outputs = Vec::new();
    let mut inputs = Vec::new();

    for obj in dump.as_array().unwrap_or(&vec![]) {
        if obj.get("type").and_then(|v| v.as_str()) != Some("PipeWire:Interface:Node") {
            continue;
        }
        let props = match obj.pointer("/info/props") {
            Some(p) => p,
            None => continue,
        };
        let media_class = props.get("media.class").and_then(|v| v.as_str()).unwrap_or("");
        if media_class != "Audio/Sink" && media_class != "Audio/Source" {
            continue;
        }

        let id = obj.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let name = props.get("node.description")
            .or_else(|| props.get("node.nick"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let node_name = props.get("node.name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let (vol, muted) = extract_volume(obj);
        let is_sink = media_class == "Audio/Sink";
        let is_default = if is_sink {
            default_sink.as_deref() == Some(node_name.as_str())
        } else {
            default_source.as_deref() == Some(node_name.as_str())
        };

        let device = PwDevice { id, name, volume: vol, muted, is_default };

        if is_sink {
            outputs.push(device);
        } else {
            inputs.push(device);
        }
    }

    let denoise = dump.as_array().unwrap_or(&vec![]).iter().any(|obj| {
        obj.get("type").and_then(|v| v.as_str()) == Some("PipeWire:Interface:Node")
            && obj.pointer("/info/props/node.name")
                .and_then(|v| v.as_str()) == Some("rnnoise_source")
    });

    PwState { outputs, inputs, denoise }
}

fn pw_dump() -> Value {
    let Ok(out) = Command::new("pw-dump").output() else {
        return Value::Array(vec![]);
    };
    serde_json::from_slice(&out.stdout).unwrap_or(Value::Array(vec![]))
}

fn default_device_name(dump: &Value, key: &str) -> Option<String> {
    for obj in dump.as_array()? {
        if obj.get("type").and_then(|v| v.as_str()) != Some("PipeWire:Interface:Metadata") {
            continue;
        }
        let metadata = obj.get("metadata").and_then(|v| v.as_array())?;
        for entry in metadata {
            if entry.get("key").and_then(|v| v.as_str()) == Some(key) {
                return entry.pointer("/value/name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
        }
    }
    None
}

fn extract_volume(node: &Value) -> (i64, bool) {
    let props_array = match node.pointer("/info/params/Props") {
        Some(Value::Array(arr)) => arr,
        _ => return (0, false),
    };

    for props in props_array {
        if let (Some(vols), Some(mute)) = (
            props.get("channelVolumes").and_then(|v| v.as_array()),
            props.get("mute").and_then(|v| v.as_bool()),
        ) {
            let avg = if vols.is_empty() {
                0.0
            } else {
                vols.iter()
                    .filter_map(|v| v.as_f64())
                    .sum::<f64>() / vols.len() as f64
            };
            let pct = (avg.cbrt() * 100.0).round() as i64;
            return (pct.clamp(0, 150), mute);
        }
    }

    (0, false)
}
