use serde_json::Value;

pub enum ActionResult {
    Ok { toast: String },
    BarOnly,
    Unknown,
}

/// Execute a native action standalone (no bar required).
/// Returns Ok with toast text, BarOnly if it needs the bar process, or Unknown.
pub fn exec_native(
    module: &str,
    action: &str,
    args: &[String],
    params: &serde_json::Map<String, Value>,
) -> ActionResult {
    match (module, action) {
        ("recording", "start") => {
            crate::mods::recording::start_recording(false, false);
            ActionResult::Ok { toast: "recording".into() }
        }
        ("recording", "stop") => {
            crate::mods::recording::stop_recording();
            ActionResult::Ok { toast: "recording saved".into() }
        }
        ("recording", "toggle-audio" | "toggle-mic") => ActionResult::BarOnly,

        ("inputs", "denoise") => {
            let pid_path = crate::mods::inputs::denoise_pid_path();
            let active = pid_path.exists()
                && std::fs::read_to_string(&pid_path)
                    .ok()
                    .and_then(|s| s.trim().parse::<i32>().ok())
                    .map(|pid| unsafe { libc::kill(pid, 0) } == 0)
                    .unwrap_or(false);
            crate::mods::inputs::toggle_denoise(active);
            crate::pipewire::invalidate();
            let state = if active { "disabled" } else { "enabled" };
            ActionResult::Ok { toast: format!("denoise {state}") }
        }

        ("wallpaper", "shuffle") => {
            let group = args.first().map(|s| s.as_str());
            crate::mods::wallpaper::shuffle(params, group);
            ActionResult::Ok { toast: "shuffled".into() }
        }
        ("wallpaper", "init") => {
            crate::mods::wallpaper::init(params);
            ActionResult::Ok { toast: String::new() }
        }

        ("notifications", "clear") => ActionResult::BarOnly,

        _ => ActionResult::Unknown,
    }
}
