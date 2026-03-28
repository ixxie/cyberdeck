use serde_json::{json, Value};
use std::process::Command;
use std::sync::Mutex;

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::mods::{InteractiveModule, KeyResult};

static REC_PID: Mutex<Option<u32>> = Mutex::new(None);

fn snip_dir(sub: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/{sub}/snips")
}

fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string()
}

fn ensure_dir(path: &str) {
    let _ = std::fs::create_dir_all(path);
}

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

fn snip_region() {
    std::thread::spawn(|| {
        let region = Command::new("slurp").output();
        let Ok(region) = region else { return };
        if !region.status.success() { return; }
        let region = String::from_utf8_lossy(&region.stdout).trim().to_string();
        if region.is_empty() { return; }

        let dir = snip_dir("Pictures");
        ensure_dir(&dir);
        let path = format!("{dir}/{}.png", timestamp());

        let ok = Command::new("grim")
            .args(["-g", &region, &path])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if ok {
            // Copy to clipboard
            let cat = Command::new("cat").arg(&path).output();
            if let Ok(cat) = cat {
                let _ = Command::new("wl-copy")
                    .args(["-t", "image/png"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .and_then(|mut child| {
                        use std::io::Write;
                        if let Some(ref mut stdin) = child.stdin {
                            stdin.write_all(&cat.stdout)?;
                        }
                        child.wait()
                    });
            }
            log::info!("snip: saved & copied {path}");
        }
    });
}

fn snip_screen() {
    std::thread::spawn(|| {
        let dir = snip_dir("Pictures");
        ensure_dir(&dir);
        let path = format!("{dir}/{}.png", timestamp());

        let ok = Command::new("grim")
            .arg(&path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if ok {
            log::info!("snip: saved {path}");
        }
    });
}

fn start_recording(with_region: bool, with_audio: bool) {
    if is_recording() { return; }

    std::thread::spawn(move || {
        let region = if with_region {
            let r = Command::new("slurp").output();
            match r {
                Ok(r) if r.status.success() => {
                    let s = String::from_utf8_lossy(&r.stdout).trim().to_string();
                    if s.is_empty() { return; }
                    Some(s)
                }
                _ => return,
            }
        } else {
            None
        };

        let dir = snip_dir("Videos");
        ensure_dir(&dir);
        let path = format!("{dir}/{}.mp4", timestamp());

        let mut args = vec!["wl-screenrec", "-f", &path];
        let region_str;
        if let Some(ref r) = region {
            region_str = r.clone();
            args.push("-g");
            args.push(&region_str);
        }
        if with_audio {
            args.push("--audio");
        }

        let child = Command::new(args[0])
            .args(&args[1..])
            .spawn();

        let Ok(child) = child else {
            log::error!("snip: failed to start wl-screenrec");
            return;
        };

        let pid = child.id();
        *REC_PID.lock().unwrap() = Some(pid);
        log::info!("snip: recording started (pid={pid})");

        // Wait for process to exit
        let _ = Command::new("tail")
            .args(["--pid", &pid.to_string(), "-f", "/dev/null"])
            .status();

        let mut rec = REC_PID.lock().unwrap();
        if *rec == Some(pid) {
            *rec = None;
        }
        log::info!("snip: recording saved {path}");
    });
}

// Poll: check if recording is active
pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    json!({ "recording": is_recording() })
}

// Interactive module
pub struct SnipDeep {
    cursor: usize,
}

impl SnipDeep {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    fn actions(&self, recording: bool) -> Vec<(&'static str, &'static str, &'static str)> {
        if recording {
            vec![("x", "stop recording", "stop")]
        } else {
            vec![
                ("r", "region screenshot", "selection"),
                ("s", "screen screenshot", "monitor"),
                ("v", "record screen", "record"),
                ("a", "record + audio", "microphone"),
            ]
        }
    }
}

impl InteractiveModule for SnipDeep {
    fn render_center(&self, fg: Rgba, data: &Value) -> Vec<Vec<Elem>> {
        let idle_fg = Rgba::new(fg.r, fg.g, fg.b, (fg.a as f32 * 0.44) as u8);
        let recording = data.get("recording").and_then(|v| v.as_bool()).unwrap_or(false);
        let actions = self.actions(recording);

        actions.iter().enumerate().map(|(i, (key, label, _icon))| {
            let item_fg = if i == self.cursor { fg } else { idle_fg };
            vec![Elem::text(format!("{label} ({key})")).fg(item_fg)]
        }).collect()
    }

    fn cursor(&self) -> Option<usize> { Some(self.cursor) }

    fn breadcrumb(&self) -> Vec<String> {
        vec!["Snip".into()]
    }

    fn key_hints(&self) -> Vec<KeyHintDef> {
        vec![
            KeyHintDef { key: "←→".into(), action: String::new(), label: "select".into(), icon: None },
            KeyHintDef { key: "⏎".into(), action: String::new(), label: "run".into(), icon: None },
            KeyHintDef { key: "Esc".into(), action: "back".into(), label: "back".into(), icon: None },
        ]
    }

    fn handle_key(&mut self, event: &KeyEvent, data: &Value) -> KeyResult {
        let recording = data.get("recording").and_then(|v| v.as_bool()).unwrap_or(false);
        let actions = self.actions(recording);
        let count = actions.len();
        if count == 0 { return KeyResult::Ignored; }

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
                if let Some((key, _, _)) = actions.get(self.cursor) {
                    self.exec_action(key);
                }
                KeyResult::Action
            }
            _ => {
                if let Some(ch) = event.utf8.as_deref() {
                    if actions.iter().any(|(k, _, _)| *k == ch) {
                        self.exec_action(ch);
                        return KeyResult::Action;
                    }
                }
                KeyResult::Ignored
            }
        }
    }

    fn reset(&mut self) {
        self.cursor = 0;
    }
}

impl SnipDeep {
    fn exec_action(&self, key: &str) {
        match key {
            "r" => snip_region(),
            "s" => snip_screen(),
            "v" => start_recording(false, false),
            "a" => start_recording(false, true),
            "x" => { stop_recording(); }
            _ => {}
        }
    }
}
