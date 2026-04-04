use std::process::Command;

use serde_json::{json, Value};

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;
use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};
use crate::pipewire;

pub fn subscribe(
    params: serde_json::Map<String, Value>,
    sender: Sender<(String, Value)>,
    id: String,
) {
    let mut last_state = String::new();
    loop {
        let cur_state = wpctl_state("@DEFAULT_AUDIO_SOURCE@");
        if cur_state != last_state {
            last_state = cur_state;
            pipewire::invalidate();
            let val = poll(&params);
            if sender.send((id.clone(), val)).is_err() {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn wpctl_state(target: &str) -> String {
    std::process::Command::new("wpctl")
        .args(["get-volume", target])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let state = pipewire::query();

    let default = state.inputs.iter().find(|d| d.is_default);
    let volume = default.map(|d| d.volume).unwrap_or(0);
    let muted = default.map(|d| d.muted).unwrap_or(false);
    let source_name = default.map(|d| d.name.as_str()).unwrap_or("unknown");

    let inputs: Vec<Value> = state.inputs.iter().map(|d| json!({
        "id": d.id,
        "name": d.name,
        "volume": d.volume,
        "muted": d.muted,
        "default": d.is_default,
    })).collect();

    json!({
        "volume": volume,
        "muted": muted,
        "source": source_name,
        "inputs": inputs,
        "denoise": state.denoise,
    })
}

// --- Denoise ---

fn find_rnnoise_library() -> Option<String> {
    let lib_name = "librnnoise_ladspa.so";

    if let Ok(ladspa_path) = std::env::var("LADSPA_PATH") {
        for dir in ladspa_path.split(':') {
            let path = format!("{dir}/{lib_name}");
            if std::path::Path::new(&path).exists() {
                return Some(path);
            }
        }
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for bin_dir in path_var.split(':') {
            let parent = std::path::Path::new(bin_dir).parent();
            for subdir in &["lib/ladspa", "lib"] {
                let candidate = parent.map(|p| p.join(subdir).join(lib_name));
                if let Some(p) = candidate {
                    if p.exists() {
                        return Some(p.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

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

pub fn denoise_pid_path() -> std::path::PathBuf {
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

pub fn toggle_denoise(currently_active: bool) {
    if currently_active {
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

// --- Deep module ---

pub struct InputsDeep {
    cursor: usize,
}

impl InputsDeep {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    fn devices<'a>(&self, data: &'a Value) -> Option<&'a Vec<Value>> {
        data.get("inputs").and_then(|v| v.as_array())
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

impl InteractiveModule for InputsDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let devices = match self.devices(data) {
            Some(d) if !d.is_empty() => d,
            _ => return vec![vec![Elem::text("no inputs").fg(idle_fg)]],
        };

        let mut items = Vec::new();

        let denoise = data.get("denoise").and_then(|v| v.as_bool()).unwrap_or(false);
        if denoise {
            items.push(vec![Elem::text("◆ denoise").fg(active_fg)]);
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
            let vol_str = if muted { "muted".to_string() } else { format!("{vol}%") };
            items.push(vec![
                Elem::text(format!("{prefix} {name} {vol_str}")).fg(dev_fg),
            ]);
        }

        items
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }



    fn key_hints(&self) -> Vec<KeyHintDef> {
        let mut hints = vec![
            KeyHintDef { key: "↑↓".into(), action: String::new(), label: "vol".into(), icon: None },
            KeyHintDef { key: "m".into(), action: String::new(), label: "mute".into(), icon: None },
        ];
        if find_rnnoise_library().is_some() {
            hints.push(KeyHintDef { key: "n".into(), action: String::new(), label: "denoise".into(), icon: None });
        }
        hints
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let count = self.device_count(data);

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
            Keysym::Up => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-volume {id} 5%+"));
                }
                KeyResult::Action
            }
            Keysym::Down => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-volume {id} 5%-"));
                }
                KeyResult::Action
            }
            Keysym::Return => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-default {id}"));
                }
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("m") => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-mute {id} toggle"));
                }
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("n") => {
                let active = data.get("denoise").and_then(|v| v.as_bool()).unwrap_or(false);
                toggle_denoise(active);
                pipewire::invalidate();
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn activate(&mut self, data: &serde_json::Value, _sub_path: &[String]) {
        if let Some(devs) = self.devices(data) {
            self.cursor = devs.iter().position(|d| {
                d.get("default").and_then(|v| v.as_bool()).unwrap_or(false)
            }).unwrap_or(0);
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }

    fn exec_action(&mut self, name: &str, _args: &[String], data: &serde_json::Value) -> Option<String> {
        match name {
            "denoise" => {
                let active = data.get("denoise").and_then(|v| v.as_bool()).unwrap_or(false);
                toggle_denoise(active);
                pipewire::invalidate();
                let state = if active { "disabled" } else { "enabled" };
                Some(format!("denoise {state}"))
            }
            _ => None,
        }
    }
}
