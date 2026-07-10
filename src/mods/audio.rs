use std::collections::BTreeMap;
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

// ---- Polling / data ----

pub fn subscribe(
    params: serde_json::Map<String, Value>,
    sender: Sender<(String, Value)>,
    id: String,
) {
    let mut last = String::new();
    loop {
        // Full status covers device add/remove, default changes and volumes —
        // polling only the defaults missed devices appearing or vanishing.
        let snap = wpctl_status();
        if snap != last {
            last = snap;
            pipewire::invalidate();
            let val = poll(&params);
            if sender.send((id.clone(), val)).is_err() {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn wpctl_status() -> String {
    Command::new("wpctl")
        .arg("status")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn profile_index(dev: &pipewire::BtDevice, preferred: &str, prefix: &str) -> Option<u32> {
    dev.profiles
        .iter()
        .find(|p| p.name == preferred)
        .or_else(|| dev.profiles.iter().find(|p| p.name.starts_with(prefix)))
        .map(|p| p.index)
}

fn bt_json(dev: &pipewire::BtDevice) -> Value {
    json!({
        "id": dev.device_id,
        "profile": dev.profile,
        "a2dp": profile_index(dev, "a2dp-sink", "a2dp-sink"),
        "hfp": profile_index(dev, "headset-head-unit", "headset"),
    })
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let pw = pipewire::query();

    // Group sinks + sources by description so a headset (one physical device,
    // both directions) renders as a single device with two role-keyed slots.
    let mut by_name: BTreeMap<String, (Option<&pipewire::PwDevice>, Option<&pipewire::PwDevice>)> =
        BTreeMap::new();
    for d in &pw.outputs {
        by_name.entry(d.name.clone()).or_default().0 = Some(d);
    }
    for d in &pw.inputs {
        by_name.entry(d.name.clone()).or_default().1 = Some(d);
    }

    let mut devices: Vec<Value> = by_name
        .into_iter()
        .map(|(name, (out, inp))| {
            let bt = out
                .and_then(|d| d.device_id)
                .or_else(|| inp.and_then(|d| d.device_id))
                .and_then(|id| pw.bt.iter().find(|b| b.device_id == id));
            json!({
                "name": name,
                "out": out.map(|d| json!({
                    "id": d.id,
                    "default": d.is_default,
                    "volume": d.volume,
                    "muted": d.muted,
                    "error": d.state == "error",
                })),
                "in": inp.map(|d| json!({
                    "id": d.id,
                    "default": d.is_default,
                    "volume": d.volume,
                    "muted": d.muted,
                    "error": d.state == "error",
                })),
                "bt": bt.map(bt_json),
            })
        })
        .collect();

    // Bluetooth devices whose active profile exposes no nodes at all (profile
    // "off") would otherwise vanish from the UI while still being connected.
    for b in &pw.bt {
        let covered = pw.outputs.iter().chain(&pw.inputs)
            .any(|d| d.device_id == Some(b.device_id));
        if !covered {
            devices.push(json!({
                "name": b.name,
                "out": Value::Null,
                "in": Value::Null,
                "bt": bt_json(b),
            }));
        }
    }

    let sink = pw.outputs.iter().find(|d| d.is_default);
    let source = pw.inputs.iter().find(|d| d.is_default);

    json!({
        "volume": sink.map(|d| d.volume).unwrap_or(0),
        "muted": sink.map(|d| d.muted).unwrap_or(false),
        "sink": sink.map(|d| d.name.as_str()).unwrap_or("unknown"),
        "source": source.map(|d| d.name.as_str()).unwrap_or("unknown"),
        "source_volume": source.map(|d| d.volume).unwrap_or(0),
        "source_muted": source.map(|d| d.muted).unwrap_or(false),
        "devices": devices,
        "denoise": pw.denoise,
    })
}

// ---- Denoise ----

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
    let cache = std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.cache")
    });
    std::path::PathBuf::from(cache).join("cyberdeck/denoise.conf")
}

pub fn denoise_pid_path() -> std::path::PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
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
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
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

// ---- Deep view ----

// A bluetooth device stays listed in both tabs even when its active profile
// has no node in that direction (a2dp has no mic; "off" has nothing) — hiding
// it made "connected buds, silent default mic" undiagnosable from the UI.
fn devices_for<'a>(data: &'a Value, dir_key: &str) -> Vec<&'a Value> {
    data.get("devices")
        .and_then(|v| v.as_array())
        .map(|all| {
            all.iter()
                .filter(|d| {
                    let has_dir = d.get(dir_key).map(|v| !v.is_null()).unwrap_or(false);
                    let is_bt = d.get("bt").map(|v| !v.is_null()).unwrap_or(false);
                    has_dir || is_bt
                })
                .collect()
        })
        .unwrap_or_default()
}

fn profile_tag(profile: &str) -> &'static str {
    if profile.starts_with("a2dp") {
        "a2dp"
    } else if profile.starts_with("headset") || profile.starts_with("hsp") || profile.starts_with("hfp") {
        "hfp"
    } else {
        "off"
    }
}

#[derive(Copy, Clone, PartialEq)]
enum Tab {
    Speakers = 0,
    Microphones = 1,
}

impl Tab {
    fn dir_key(self) -> &'static str {
        match self {
            Tab::Speakers => "out",
            Tab::Microphones => "in",
        }
    }
    fn wpctl_default(self) -> &'static str {
        match self {
            Tab::Speakers => "@DEFAULT_AUDIO_SINK@",
            Tab::Microphones => "@DEFAULT_AUDIO_SOURCE@",
        }
    }
    fn empty_label(self) -> &'static str {
        match self {
            Tab::Speakers => "no speakers",
            Tab::Microphones => "no microphones",
        }
    }
    fn default_action_label(self) -> &'static str {
        match self {
            Tab::Speakers => "default speaker",
            Tab::Microphones => "default mic",
        }
    }
}

pub struct AudioDeep {
    cursors: [usize; 2],
    tab: Tab,
    icon_speaker_high: String,
    icon_speaker_low: String,
    icon_speaker_slash: String,
    icon_microphone: String,
    icon_microphone_slash: String,
}

impl AudioDeep {
    pub fn new(icon_resolver: &dyn Fn(&str) -> String) -> Self {
        Self {
            cursors: [0, 0],
            tab: Tab::Speakers,
            icon_speaker_high: icon_resolver("speaker-high"),
            icon_speaker_low: icon_resolver("speaker-low"),
            icon_speaker_slash: icon_resolver("speaker-slash"),
            icon_microphone: icon_resolver("microphone"),
            icon_microphone_slash: icon_resolver("microphone-slash"),
        }
    }

    fn cursor_idx(&self) -> usize {
        self.cursors[self.tab as usize]
    }

    fn set_cursor(&mut self, v: usize) {
        self.cursors[self.tab as usize] = v;
    }

    fn tab_devices<'a>(&self, data: &'a Value) -> Vec<&'a Value> {
        devices_for(data, self.tab.dir_key())
    }

    fn selected_id(&self, data: &Value) -> Option<u32> {
        let devs = self.tab_devices(data);
        let dev = devs.get(self.cursor_idx())?;
        dev.get(self.tab.dir_key())?
            .get("id")?
            .as_u64()
            .map(|v| v as u32)
    }

    fn icon_for(&self, default: bool, muted: bool) -> &str {
        match self.tab {
            Tab::Speakers => {
                if muted {
                    &self.icon_speaker_slash
                } else if default {
                    &self.icon_speaker_high
                } else {
                    &self.icon_speaker_low
                }
            }
            Tab::Microphones => {
                if muted {
                    &self.icon_microphone_slash
                } else {
                    &self.icon_microphone
                }
            }
        }
    }
}

impl InteractiveModule for AudioDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let active_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.72) as u8);
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);

        let key = self.tab.dir_key();
        let devices = self.tab_devices(data);

        if devices.is_empty() {
            return vec![vec![Elem::text(self.tab.empty_label()).fg(idle_fg)]];
        }

        let cursor = self.cursor_idx();
        devices
            .iter()
            .enumerate()
            .map(|(i, dev)| {
                let name = dev.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let dir = dev.get(key).unwrap_or(&Value::Null);
                let bt_profile = dev.pointer("/bt/profile").and_then(|v| v.as_str());

                if dir.is_null() {
                    // Bluetooth device whose profile has no node this direction
                    let tag = bt_profile.map(profile_tag).unwrap_or("off");
                    let missing = match self.tab {
                        Tab::Speakers => "no output",
                        Tab::Microphones => "no mic",
                    };
                    let row_fg = if i == cursor { fg } else { idle_fg };
                    return vec![
                        Elem::text(format!("{} {name} {missing} [{tag}]", self.icon_for(false, true)))
                            .fg(row_fg),
                    ];
                }

                let muted = dir.get("muted").and_then(|v| v.as_bool()).unwrap_or(false);
                let vol = dir.get("volume").and_then(|v| v.as_i64()).unwrap_or(0);
                let is_default = dir.get("default").and_then(|v| v.as_bool()).unwrap_or(false);

                let icon = self.icon_for(is_default, muted);
                let errored = dir.get("error").and_then(|v| v.as_bool()).unwrap_or(false);
                let vol_str = if errored {
                    "error".to_string()
                } else if muted {
                    "muted".to_string()
                } else {
                    format!("{vol}%")
                };
                let tag = bt_profile.map(|p| format!(" [{}]", profile_tag(p))).unwrap_or_default();

                let row_fg = if i == cursor {
                    fg
                } else if is_default {
                    active_fg
                } else {
                    idle_fg
                };

                vec![Elem::text(format!("{icon} {name} {vol_str}{tag}")).fg(row_fg)]
            })
            .collect()
    }

    fn cursor(&self) -> Option<usize> {
        Some(self.cursor_idx())
    }

    fn tabs(&self) -> Vec<String> {
        vec!["speakers".into(), "microphones".into()]
    }

    fn active_tab(&self) -> usize {
        self.tab as usize
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        let mut hints = vec![
            KeyHintDef { key: "⇥".into(), action: String::new(), label: "tab".into(), icon: None },
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: self.tab.default_action_label().into(), icon: None },
            KeyHintDef { key: "↑↓".into(), action: String::new(), label: "vol".into(), icon: None },
            KeyHintDef { key: "m".into(), action: String::new(), label: "mute".into(), icon: None },
            KeyHintDef { key: "p".into(), action: String::new(), label: "bt profile".into(), icon: None },
        ];
        if self.tab == Tab::Microphones && find_rnnoise_library().is_some() {
            hints.push(KeyHintDef {
                key: "n".into(),
                action: String::new(),
                label: "denoise".into(),
                icon: None,
            });
        }
        hints
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let count = self.tab_devices(data).len();

        match event.keysym {
            Keysym::Tab => {
                self.tab = if self.tab == Tab::Speakers { Tab::Microphones } else { Tab::Speakers };
                KeyResult::Handled
            }
            Keysym::ISO_Left_Tab => {
                self.tab = if self.tab == Tab::Microphones { Tab::Speakers } else { Tab::Microphones };
                KeyResult::Handled
            }
            Keysym::Left => {
                if count > 0 {
                    let cur = self.cursor_idx();
                    self.set_cursor(cur.checked_sub(1).unwrap_or(count - 1));
                }
                KeyResult::Handled
            }
            Keysym::Right => {
                if count > 0 {
                    let cur = self.cursor_idx();
                    self.set_cursor((cur + 1) % count);
                }
                KeyResult::Handled
            }
            Keysym::Up => {
                BarApp::spawn_command(&format!("wpctl set-volume {} 5%+", self.tab.wpctl_default()));
                KeyResult::Action
            }
            Keysym::Down => {
                BarApp::spawn_command(&format!("wpctl set-volume {} 5%-", self.tab.wpctl_default()));
                KeyResult::Action
            }
            Keysym::Return => {
                if let Some(id) = self.selected_id(data) {
                    BarApp::spawn_command(&format!("wpctl set-default {id}"));
                }
                KeyResult::Action
            }
            _ if event.utf8.as_deref() == Some("m") => {
                BarApp::spawn_command(&format!("wpctl set-mute {} toggle", self.tab.wpctl_default()));
                KeyResult::Action
            }
            // Per-tab profile switch: mic tab moves the device to headset
            // (bidirectional), speakers tab back to a2dp (hi-fi).
            _ if event.utf8.as_deref() == Some("p") => {
                let devs = self.tab_devices(data);
                let bt = devs.get(self.cursor_idx()).and_then(|d| d.get("bt")).filter(|v| !v.is_null());
                if let Some(bt) = bt {
                    let target_key = match self.tab {
                        Tab::Speakers => "a2dp",
                        Tab::Microphones => "hfp",
                    };
                    let id = bt.get("id").and_then(|v| v.as_u64());
                    let target = bt.get(target_key).and_then(|v| v.as_u64());
                    if let (Some(id), Some(target)) = (id, target) {
                        BarApp::spawn_command(&format!("wpctl set-profile {id} {target}"));
                    }
                }
                KeyResult::Action
            }
            _ if self.tab == Tab::Microphones && event.utf8.as_deref() == Some("n") => {
                let active = data.get("denoise").and_then(|v| v.as_bool()).unwrap_or(false);
                toggle_denoise(active);
                pipewire::invalidate();
                KeyResult::Action
            }
            _ => KeyResult::Ignored,
        }
    }

    fn activate(&mut self, data: &Value, _sub_path: &[String]) {
        for (idx, key) in [(0usize, "out"), (1, "in")] {
            let filtered = devices_for(data, key);
            let pos = filtered
                .iter()
                .position(|d| {
                    d.get(key)
                        .and_then(|v| v.get("default"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                })
                .unwrap_or(0);
            self.cursors[idx] = pos;
        }
    }

    fn reset(&mut self) {
        self.cursors = [0, 0];
        self.tab = Tab::Speakers;
    }

    fn exec_action(&mut self, name: &str, _args: &[String], data: &Value) -> Option<String> {
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
