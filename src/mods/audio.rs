use serde_json::{json, Value};
use std::process::Command;

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::RenderedWidget;
use crate::mods::InteractiveModule;

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let dump = pw_dump();

    let default_sink = default_device_name(&dump, "default.audio.sink");
    let default_source = default_device_name(&dump, "default.audio.source");

    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    let mut volume = 0i64;
    let mut muted = false;
    let mut input_volume = 0i64;
    let mut input_muted = false;

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
            .unwrap_or("Unknown");
        let node_name = props.get("node.name").and_then(|v| v.as_str()).unwrap_or("");

        // Volume from Props params
        let (dev_vol, dev_muted) = extract_volume(obj);

        let is_sink = media_class == "Audio/Sink";
        let is_default = if is_sink {
            default_sink.as_deref() == Some(node_name)
        } else {
            default_source.as_deref() == Some(node_name)
        };

        if is_default {
            if is_sink {
                volume = dev_vol;
                muted = dev_muted;
            } else {
                input_volume = dev_vol;
                input_muted = dev_muted;
            }
        }

        let device = json!({
            "id": id,
            "name": name,
            "volume": dev_vol,
            "muted": dev_muted,
            "default": is_default,
        });

        if is_sink {
            outputs.push(device);
        } else {
            inputs.push(device);
        }
    }

    // Detect if denoise filter is active (rnnoise node exists)
    let denoise = dump.as_array().unwrap_or(&vec![]).iter().any(|obj| {
        obj.get("type").and_then(|v| v.as_str()) == Some("PipeWire:Interface:Node")
            && obj.pointer("/info/props/node.name")
                .and_then(|v| v.as_str()) == Some("rnnoise_source")
    });

    json!({
        "volume": volume,
        "muted": muted,
        "input_volume": input_volume,
        "input_muted": input_muted,
        "denoise": denoise,
        "sink": outputs.iter()
            .find(|o| o.get("default").and_then(|v| v.as_bool()).unwrap_or(false))
            .and_then(|o| o.get("name").and_then(|v| v.as_str()))
            .unwrap_or("unknown"),
        "outputs": outputs,
        "inputs": inputs,
    })
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
            // Average channel volumes, convert from linear to percentage
            let avg = if vols.is_empty() {
                0.0
            } else {
                vols.iter()
                    .filter_map(|v| v.as_f64())
                    .sum::<f64>() / vols.len() as f64
            };
            // Convert cubic volume to percentage (PipeWire uses cubic scale)
            let pct = (avg.cbrt() * 100.0).round() as i64;
            return (pct.clamp(0, 150), mute);
        }
    }

    (0, false)
}

// --- Denoise ---

fn find_rnnoise_library() -> Option<String> {
    let lib_name = "librnnoise_ladspa.so";

    // Check LADSPA_PATH
    if let Ok(ladspa_path) = std::env::var("LADSPA_PATH") {
        for dir in ladspa_path.split(':') {
            let path = format!("{dir}/{lib_name}");
            if std::path::Path::new(&path).exists() {
                return Some(path);
            }
        }
    }

    // Check lib/ladspa/ relative to PATH entries (works with nix wrappers)
    if let Ok(path_var) = std::env::var("PATH") {
        for bin_dir in path_var.split(':') {
            let lib_dir = std::path::Path::new(bin_dir)
                .parent()
                .map(|p| p.join("lib/ladspa").join(lib_name));
            if let Some(p) = lib_dir {
                if p.exists() {
                    return Some(p.to_string_lossy().to_string());
                }
            }
        }
    }

    // Common system paths
    for dir in &["/usr/lib/ladspa", "/usr/lib64/ladspa", "/usr/local/lib/ladspa"] {
        let path = format!("{dir}/{lib_name}");
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }

    None
}

fn denoise_config_path() -> std::path::PathBuf {
    let cache = std::env::var("XDG_CACHE_HOME")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.cache")
        });
    std::path::PathBuf::from(cache).join("cyberdeck/denoise.conf")
}

fn denoise_pid_path() -> std::path::PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(runtime).join("cyberdeck-denoise.pid")
}

fn write_denoise_config(lib_path: &str) -> Option<String> {
    let conf_path = denoise_config_path();
    let _ = std::fs::create_dir_all(conf_path.parent()?);
    let config = format!(
        r#"context.modules = [
{{  name = libpipewire-module-filter-chain
    args = {{
        node.description = "Noise Canceling Source"
        media.name       = "Noise Canceling Source"
        filter.graph = {{
            nodes = [
                {{
                    type  = ladspa
                    name  = rnnoise
                    plugin = "{lib_path}"
                    label = noise_suppressor_mono
                    control = {{
                        "VAD Threshold (%)" = 50.0
                        "VAD Grace Period (ms)" = 200
                        "Retroactive VAD Grace (ms)" = 0
                    }}
                }}
            ]
        }}
        capture.props = {{
            node.name   = capture.rnnoise_source
            node.passive = true
            audio.rate  = 48000
        }}
        playback.props = {{
            node.name   = rnnoise_source
            node.description = "Noise Canceling Source"
            media.class = Audio/Source
            audio.rate  = 48000
        }}
    }}
}}
]"#
    );
    std::fs::write(&conf_path, &config).ok()?;
    Some(conf_path.to_string_lossy().to_string())
}

fn toggle_denoise(currently_active: bool) {
    if currently_active {
        // Kill the denoise process
        if let Ok(pid_str) = std::fs::read_to_string(denoise_pid_path()) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                unsafe { libc::kill(pid, libc::SIGTERM); }
            }
        }
        let _ = std::fs::remove_file(denoise_pid_path());
    } else {
        let lib_path = match find_rnnoise_library() {
            Some(p) => p,
            None => {
                log::warn!("rnnoise LADSPA plugin not found, denoise unavailable");
                return;
            }
        };
        let conf_path = match write_denoise_config(&lib_path) {
            Some(p) => p,
            None => return,
        };
        match Command::new("pipewire")
            .args(["-c", &conf_path])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                let _ = std::fs::write(denoise_pid_path(), child.id().to_string());
                log::info!("denoise started (pid {})", child.id());
            }
            Err(e) => log::error!("failed to start denoise: {e}"),
        }
    }
}

pub fn cli_toggle_denoise() {
    // Check if denoise is active by looking for the PID file
    let active = denoise_pid_path().exists()
        && std::fs::read_to_string(denoise_pid_path())
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .map(|pid| unsafe { libc::kill(pid, 0) } == 0)
            .unwrap_or(false);
    toggle_denoise(active);
    if active {
        eprintln!("denoise disabled");
    } else {
        eprintln!("denoise enabled");
    }
}

// --- Deep module ---

#[derive(PartialEq)]
enum AudioTab {
    Output,
    Input,
}

pub struct AudioDeep {
    tab: AudioTab,
    cursor: usize,
}

impl AudioDeep {
    pub fn new() -> Self {
        Self {
            tab: AudioTab::Output,
            cursor: 0,
        }
    }

    fn devices<'a>(&self, data: &'a Value) -> Option<&'a Vec<Value>> {
        let key = match self.tab {
            AudioTab::Output => "outputs",
            AudioTab::Input => "inputs",
        };
        data.get(key).and_then(|v| v.as_array())
    }

    fn device_count(&self, data: &Value) -> usize {
        self.devices(data).map(|a| a.len()).unwrap_or(0)
    }

    fn selected_id(&self, data: &Value) -> Option<u32> {
        self.devices(data)
            .and_then(|devs| devs.get(self.cursor))
            .and_then(|d| d.get("id"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
    }
}

impl InteractiveModule for AudioDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<RenderedWidget> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let devices = match self.devices(data) {
            Some(d) if !d.is_empty() => d,
            _ => {
                let label = match self.tab {
                    AudioTab::Output => "no outputs",
                    AudioTab::Input => "no inputs",
                };
                return vec![RenderedWidget::new(label.into()).with_fg(idle_fg)];
            }
        };

        let mut widgets = Vec::new();

        // Denoise indicator on input tab
        if self.tab == AudioTab::Input {
            let denoise = data.get("denoise").and_then(|v| v.as_bool()).unwrap_or(false);
            if denoise {
                widgets.push(RenderedWidget::new("◆ denoise".into()).with_fg(active_fg));
            }
        }
        for (i, dev) in devices.iter().enumerate() {
            let name = dev.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let vol = dev.get("volume").and_then(|v| v.as_i64()).unwrap_or(0);
            let muted = dev.get("muted").and_then(|v| v.as_bool()).unwrap_or(false);
            let is_default = dev.get("default").and_then(|v| v.as_bool()).unwrap_or(false);

            let dev_fg = if i == self.cursor {
                fg
            } else if is_default {
                active_fg
            } else {
                idle_fg
            };

            let prefix = if is_default { "●" } else { "○" };
            let vol_str = if muted {
                "muted".to_string()
            } else {
                format!("{vol}%")
            };
            widgets.push(
                RenderedWidget::new(format!("{prefix} {name} {vol_str}")).with_fg(dev_fg),
            );
        }

        widgets
    }

    fn breadcrumb(&self) -> Vec<String> {
        match self.tab {
            AudioTab::Output => vec!["Output".into()],
            AudioTab::Input => vec!["Input".into()],
        }
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        let tab_label = match self.tab {
            AudioTab::Output => "input",
            AudioTab::Input => "output",
        };
        let mut hints = vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "default".into(), icon: None },
            KeyHintDef { key: "↑↓".into(), action: String::new(), label: "vol".into(), icon: None },
            KeyHintDef { key: "m".into(), action: String::new(), label: "mute".into(), icon: None },
        ];
        if self.tab == AudioTab::Input && find_rnnoise_library().is_some() {
            hints.push(KeyHintDef { key: "n".into(), action: String::new(), label: "denoise".into(), icon: None });
        }
        hints.push(KeyHintDef { key: "Tab".into(), action: String::new(), label: tab_label.into(), icon: None });
        hints.push(KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None });
        hints
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> bool {
        let count = self.device_count(data);

        match event.keysym {
            Keysym::Left => {
                if count > 0 {
                    self.cursor = self.cursor.checked_sub(1).unwrap_or(count - 1);
                }
                true
            }
            Keysym::Right => {
                if count > 0 {
                    self.cursor = (self.cursor + 1) % count;
                }
                true
            }
            Keysym::Up => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-volume {id} 5%+"));
                }
                true
            }
            Keysym::Down => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-volume {id} 5%-"));
                }
                true
            }
            Keysym::Return => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-default {id}"));
                }
                true
            }
            Keysym::Tab => {
                self.tab = match self.tab {
                    AudioTab::Output => AudioTab::Input,
                    AudioTab::Input => AudioTab::Output,
                };
                self.cursor = 0;
                true
            }
            _ if event.utf8.as_deref() == Some("m") => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-mute {id} toggle"));
                }
                true
            }
            _ if event.utf8.as_deref() == Some("n") && self.tab == AudioTab::Input => {
                let active = data.get("denoise").and_then(|v| v.as_bool()).unwrap_or(false);
                toggle_denoise(active);
                true
            }
            _ => false,
        }
    }

    fn reset(&mut self) {
        self.tab = AudioTab::Output;
        self.cursor = 0;
    }
}
