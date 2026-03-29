mod actions;
mod bluetooth;
mod keyboard;
pub mod inputs;
pub mod media;
pub mod outputs;
mod brightness;
mod launcher;
mod network;
mod notifications;
mod session;
mod recording;
mod storage;
mod system;
mod calendar;
pub mod wallpaper;
mod wallpaper_deep;
mod weather;
mod window;
mod workspaces;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::channel::{Sender, channel};
use smithay_client_toolkit::reexports::calloop::{LoopHandle, RegistrationToken};

use smithay_client_toolkit::seat::keyboard::KeyEvent;

use crate::color::Rgba;
use crate::config::KeyHintDef;
use crate::layout::Elem;
use crate::source::SharedState;

type PollFn = fn(&serde_json::Map<String, serde_json::Value>) -> serde_json::Value;

struct SourceSpec {
    poll_fn: PollFn,
    interval: u64,
    id: String,
    params: serde_json::Map<String, serde_json::Value>,
    nudge: Arc<AtomicBool>,
}

pub fn register<D: 'static>(
    kind: &str,
    interval: u64,
    id: &str,
    handle: &LoopHandle<'static, D>,
    dirty_flag: Rc<Cell<bool>>,
    states: SharedState,
    params: &HashMap<String, serde_json::Value>,
) -> Option<(RegistrationToken, Arc<AtomicBool>)> {
    let poll_fn: PollFn = match kind {
        "calendar" => calendar::poll,
        "brightness" => brightness::poll,
        "system" => system::poll,
        "storage" => storage::poll,
        "launcher" => launcher::poll,
        "session" => session::poll,
        "outputs" => outputs::poll,
        "inputs" => inputs::poll,
        "media" => media::poll,
        "network" => network::poll,
        "bluetooth" => bluetooth::poll,
        "weather" => weather::poll,
        "notifications" => notifications::poll,
        "window" => window::poll,
        "workspaces" => workspaces::poll,
        "wallpaper" => wallpaper::poll,
        "recording" => recording::poll,
        other => {
            log::error!("unknown native source kind: {other}");
            return None;
        }
    };

    let params_map: serde_json::Map<String, serde_json::Value> = params.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let (sender, receiver) = channel::<(String, serde_json::Value)>();

    // Register channel receiver on main thread
    let token = handle.insert_source(receiver, move |event, _, _| {
        if let smithay_client_toolkit::reexports::calloop::channel::Event::Msg((mod_id, val)) = event {
            let mut st = states.borrow_mut();
            if let Some(ms) = st.get_mut(&mod_id) {
                ms.data = val;
                ms.dirty = true;
            }
            dirty_flag.set(true);
        }
    }).expect("failed to register native source channel");

    // Spawn source thread
    let nudge = Arc::new(AtomicBool::new(false));
    let spec = SourceSpec {
        poll_fn,
        interval,
        id: id.to_string(),
        params: params_map,
        nudge: nudge.clone(),
    };
    spawn_poll_thread(spec, sender);

    Some((token, nudge))
}

// --- Deep Module trait ---

#[derive(Debug, Clone, PartialEq)]
pub enum KeyResult {
    Ignored,
    Handled,
    Action,
    Dismiss(String),
}

pub trait InteractiveModule {
    /// Each inner Vec<Elem> becomes a separate pill in the center zone.
    fn render_center(&self, fg: Rgba, data: &serde_json::Value) -> Vec<Vec<Elem>>;
    fn cursor(&self) -> Option<usize> { None }
    fn breadcrumb(&self) -> Vec<String>;
    fn key_hints(&self) -> Vec<KeyHintDef>;
    fn handle_key(&mut self, event: &KeyEvent, data: &serde_json::Value) -> KeyResult;
    fn reset(&mut self);

    /// Execute a named action (from CLI/IPC). Returns toast text if handled.
    fn exec_action(&mut self, _name: &str, _args: &[String], _data: &serde_json::Value) -> Option<String> {
        None
    }
}

pub fn create_interactive(
    module_type: &str,
    module: &crate::config::ModuleDef,
    icon_resolver: &dyn Fn(&str) -> String,
) -> Option<Box<dyn InteractiveModule>> {
    match module_type {
        "outputs" => Some(Box::new(outputs::OutputsDeep::new())),
        "inputs" => Some(Box::new(inputs::InputsDeep::new())),
        "media" => Some(Box::new(media::MediaDeep::new())),
        "notifications" => Some(Box::new(notifications::NotificationsDeep::new())),
        "calendar" => Some(Box::new(calendar::CalendarDeep::new())),
        "bluetooth" => Some(Box::new(bluetooth::BluetoothDeep::new())),
        "keyboard" => Some(Box::new(keyboard::KeyboardDeep::new())),
        "network" => Some(Box::new(network::NetworkDeep::new())),
        "wallpaper" => Some(Box::new(wallpaper_deep::WallpaperDeep::new(module))),
        "recording" => Some(Box::new(recording::RecordingDeep::new(icon_resolver))),
        "actions" => Some(Box::new(actions::ActionPalette::new(
            &module.name,
            module.key_hints.clone(),
            icon_resolver,
        ))),
        _ => None,
    }
}

fn spawn_poll_thread(spec: SourceSpec, sender: Sender<(String, serde_json::Value)>) {
    std::thread::Builder::new()
        .name(format!("mod-{}", spec.id))
        .spawn(move || {
            log::debug!("source thread started: {}", spec.id);

            // Immediate first poll
            let val = (spec.poll_fn)(&spec.params);
            if sender.send((spec.id.clone(), val)).is_err() {
                return;
            }

            let tick = Duration::from_millis(50);
            let total = Duration::from_secs(spec.interval);
            loop {
                // Sleep in short ticks, checking for nudge
                let mut elapsed = Duration::ZERO;
                while elapsed < total {
                    if spec.nudge.swap(false, Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(tick);
                    elapsed += tick;
                }

                let val = (spec.poll_fn)(&spec.params);
                if sender.send((spec.id.clone(), val)).is_err() {
                    break;
                }
            }

            log::debug!("source thread ended: {}", spec.id);
        })
        .expect("failed to spawn source thread");
}
