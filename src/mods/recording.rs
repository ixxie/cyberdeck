use serde_json::{json, Value};
use std::process::Command;
use std::sync::Mutex;

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

static REC_PID: Mutex<Option<u32>> = Mutex::new(None);

fn is_recording() -> bool {
    REC_PID.lock().unwrap().is_some()
}

fn stop_recording() -> bool {
    let mut pid = REC_PID.lock().unwrap();
    if let Some(p) = pid.take() {
        unsafe { libc::kill(p as i32, libc::SIGINT); }
        return true;
    }
    false
}

fn slurp_region() -> Option<String> {
    let r = Command::new("slurp").output().ok()?;
    if !r.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&r.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn start_recording(device_audio: bool, _mic_audio: bool) {
    if is_recording() { return; }

    std::thread::spawn(move || {
        let Some(region) = slurp_region() else { return };

        let home = std::env::var("HOME").unwrap_or_default();
        let dir = format!("{home}/Videos/snips");
        let _ = std::fs::create_dir_all(&dir);
        let ts = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
        let path = format!("{dir}/{ts}.mp4");

        let mut cmd = Command::new("wl-screenrec");
        cmd.args(["-f", &path, "-g", &region]);
        if device_audio {
            cmd.arg("--audio");
        }

        let Ok(mut child) = cmd.spawn() else {
            log::error!("recording: failed to start wl-screenrec");
            return;
        };

        let pid = child.id();
        *REC_PID.lock().unwrap() = Some(pid);
        log::info!("recording: started (pid={pid})");

        let _ = child.wait();

        let mut rec = REC_PID.lock().unwrap();
        if *rec == Some(pid) {
            *rec = None;
        }
        log::info!("recording: saved {path}");
    });
}

// Poll
pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    json!({ "recording": is_recording() })
}

// Deep view: toggles + record/stop
pub struct RecordingDeep {
    cursor: usize,
    device_audio: bool,
    mic_audio: bool,
    icon_record: String,
    icon_stop: String,
    icon_speaker: String,
    icon_mic: String,
}

impl RecordingDeep {
    pub fn new(icon_resolver: &dyn Fn(&str) -> String) -> Self {
        Self {
            cursor: 0,
            device_audio: false,
            mic_audio: false,
            icon_record: icon_resolver("record"),
            icon_stop: icon_resolver("stop"),
            icon_speaker: icon_resolver("speaker-high"),
            icon_mic: icon_resolver("microphone"),
        }
    }
}

impl InteractiveModule for RecordingDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);
        let recording = data.get("recording").and_then(|v| v.as_bool()).unwrap_or(false);

        if recording {
            let item_fg = if self.cursor == 0 { fg } else { idle_fg };
            vec![vec![Elem::text(format!("{} stop (x)", self.icon_stop)).fg(item_fg)]]
        } else {
            let items: Vec<(&str, String, &str)> = vec![
                (&self.icon_speaker, format!("audio: {}", if self.device_audio { "on" } else { "off" }), "a"),
                (&self.icon_mic, format!("mic: {}", if self.mic_audio { "on" } else { "off" }), "m"),
                (&self.icon_record, "record (r)".into(), "r"),
            ];
            items.iter().enumerate().map(|(i, (icon, label, _key))| {
                let item_fg = if i == self.cursor { fg } else { idle_fg };
                vec![Elem::text(format!("{icon} {label}")).fg(item_fg)]
            }).collect()
        }
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }

    fn breadcrumb(&self) -> Vec<String> {
        vec!["Recording".into()]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "toggle/run".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let recording = data.get("recording").and_then(|v| v.as_bool()).unwrap_or(false);
        let count = if recording { 1 } else { 3 };

        // Shortcut keys
        if let Some(ch) = event.utf8.as_deref() {
            match (ch, recording) {
                ("a", false) => {
                    self.device_audio = !self.device_audio;
                    return KeyResult::Handled;
                }
                ("m", false) => {
                    self.mic_audio = !self.mic_audio;
                    return KeyResult::Handled;
                }
                ("r", false) => {
                    start_recording(self.device_audio, self.mic_audio);
                    return KeyResult::Dismiss("recording".into());
                }
                ("x", true) => {
                    stop_recording();
                    return KeyResult::Dismiss("recording saved".into());
                }
                _ => {}
            }
        }

        match event.keysym {
            Keysym::Left => {
                self.cursor = self.cursor.checked_sub(1).unwrap_or(count - 1);
                KeyResult::Handled
            }
            Keysym::Right => {
                self.cursor = (self.cursor + 1) % count;
                KeyResult::Handled
            }
            Keysym::Return => {
                if recording {
                    stop_recording();
                    KeyResult::Dismiss("recording saved".into())
                } else {
                    match self.cursor {
                        0 => {
                            self.device_audio = !self.device_audio;
                            KeyResult::Handled
                        }
                        1 => {
                            self.mic_audio = !self.mic_audio;
                            KeyResult::Handled
                        }
                        2 => {
                            start_recording(self.device_audio, self.mic_audio);
                            KeyResult::Dismiss("recording".into())
                        }
                        _ => KeyResult::Ignored,
                    }
                }
            }
            _ => KeyResult::Ignored,
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }

    fn exec_action(&mut self, name: &str, _args: &[String], _data: &serde_json::Value) -> Option<String> {
        match name {
            "start" => {
                start_recording(self.device_audio, self.mic_audio);
                Some("recording".into())
            }
            "stop" => {
                stop_recording();
                Some("recording saved".into())
            }
            "toggle-audio" => {
                self.device_audio = !self.device_audio;
                let state = if self.device_audio { "on" } else { "off" };
                Some(format!("audio {state}"))
            }
            "toggle-mic" => {
                self.mic_audio = !self.mic_audio;
                let state = if self.mic_audio { "on" } else { "off" };
                Some(format!("mic {state}"))
            }
            _ => None,
        }
    }
}
