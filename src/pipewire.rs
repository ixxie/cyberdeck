//! Native PipeWire backend.
//!
//! A dedicated thread runs a PipeWire main loop with registry, node, device
//! and metadata listeners, mirroring the audio graph into a shared `PwState`.
//! `query()` is a cheap read of that mirror — always current, no subprocess,
//! no cache TTL. Mutations travel over a command channel into the loop.
//!
//! On top of the primitives sits a deterministic selection orchestrator
//! (`select_mic` / `select_speaker`). WirePlumber's on-demand headset
//! autoswitch has proven racy on flaky HFP links (RFCOMM resets mid-open,
//! transports acquired while dying), so profile choreography is owned here:
//! switch the profile explicitly, wait for the node to materialise, then move
//! the default — and report the exact failing stage instead of going silent.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::{Condvar, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use libspa::param::ParamType;
use libspa::pod::deserialize::PodDeserializer;
use libspa::pod::serialize::PodSerializer;
use libspa::pod::{Object, Pod, Property, Value, ValueArray};

use crate::ipc;
use crate::mods::bluetooth;

#[derive(Clone, Debug)]
pub struct PwDevice {
    pub id: u32,
    pub name: String,
    pub node_name: String,
    pub serial: String,
    pub volume: i64,
    pub muted: bool,
    pub is_default: bool,
    pub device_id: Option<u32>,
    /// Underscored MAC when this node belongs to a device façade.
    pub facade: Option<String>,
    pub state: String,
}

#[derive(Clone, Debug)]
pub struct BtProfile {
    pub index: u32,
    pub name: String,
    pub available: bool,
}

#[derive(Clone, Debug)]
pub struct BtDevice {
    pub device_id: u32,
    pub name: String,
    pub path: String,
    pub connected: bool,
    pub profile: String,
    pub profiles: Vec<BtProfile>,
}

/// A live capture stream — an app pulling audio. Passive streams (filter
/// chains like rnnoise) and monitor taps don't express mic demand.
#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub id: u32,
    pub node_name: String,
    pub target: Option<String>,
    pub passive: bool,
    pub monitor: bool,
    pub state: String,
}

#[derive(Clone, Debug, Default)]
pub struct PwState {
    pub outputs: Vec<PwDevice>,
    pub inputs: Vec<PwDevice>,
    pub bt: Vec<BtDevice>,
    pub capture: Vec<StreamInfo>,
    pub playback: Vec<StreamInfo>,
    /// (bluetooth device global id, underscored MAC) of the active façade.
    pub facade: Option<(u32, String)>,
    pub denoise: bool,
}

// ---- Public API ----

pub fn query() -> PwState {
    match BACKEND.get() {
        Some(b) => b.state.read().unwrap().clone(),
        None => PwState::default(),
    }
}

/// Kept for callers that used to bust the pw-dump cache; the mirror is
/// event-driven now, so there is nothing to invalidate.
pub fn invalidate() {}

/// Spawn the monitor thread. Idempotent; call once at startup.
pub fn start() {
    if BACKEND.get().is_some() {
        return;
    }
    pw::init();
    let (tx, rx) = pw::channel::channel::<Cmd>();
    let _ = BACKEND.set(Backend {
        state: RwLock::new(PwState::default()),
        gen: Mutex::new(0),
        cv: Condvar::new(),
        tx: Mutex::new(tx),
    });
    std::thread::Builder::new()
        .name("pw-monitor".into())
        .spawn(move || monitor_thread(rx))
        .expect("failed to spawn pipewire monitor");
}

/// Block until the mirror has seen the graph (or timeout). Useful for CLI
/// one-shots that query right after `start()`.
pub fn wait_ready(timeout: Duration) -> bool {
    wait_for(timeout, |s| {
        if s.outputs.is_empty() && s.inputs.is_empty() {
            None
        } else {
            Some(())
        }
    })
    .is_ok()
}

/// Current change generation of the mirror. Bumps on every graph event.
pub fn generation() -> u64 {
    match BACKEND.get() {
        Some(b) => *b.gen.lock().unwrap(),
        None => 0,
    }
}

/// Block until the mirror generation moves past `seen` (or timeout). Returns
/// the latest generation. The basis for event-driven UI refresh.
pub fn wait_generation(seen: u64, timeout: Duration) -> u64 {
    let Some(b) = BACKEND.get() else {
        std::thread::sleep(timeout);
        return seen;
    };
    let deadline = Instant::now() + timeout;
    let mut gen = b.gen.lock().unwrap();
    while *gen <= seen {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let (g, _) = b.cv.wait_timeout(gen, deadline - now).unwrap();
        gen = g;
    }
    *gen
}

/// Block until the mirror has been quiet for `quiet`, bounded by `max`.
/// Binding nodes/devices triggers a burst of info/param events right after
/// connect; one-shot readers should let it settle before trusting the state.
pub fn wait_settled(quiet: Duration, max: Duration) {
    let b = backend();
    let deadline = Instant::now() + max;
    let mut gen = b.gen.lock().unwrap();
    loop {
        let before = *gen;
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let wait = quiet.min(deadline - now);
        let (g, timeout) = b.cv.wait_timeout(gen, wait).unwrap();
        gen = g;
        if timeout.timed_out() && *gen == before {
            return;
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Target {
    Node(u32),
    BtDev(u32),
}

#[derive(Copy, Clone, Debug)]
pub enum VolTarget {
    Node(u32),
    DefaultSink,
    DefaultSource,
}

#[derive(Copy, Clone, PartialEq)]
pub enum ProfileKind {
    A2dp,
    Hfp,
}

pub fn volume_step(target: VolTarget, delta_pct: i64) {
    let state = query();
    let Some(node) = resolve_vol_target(&state, target) else {
        return;
    };
    let pct = (node.volume + delta_pct).clamp(0, 150);
    send(Cmd::Volume { node: node.id, pct });
}

pub fn toggle_mute(target: VolTarget) {
    let state = query();
    let Some(node) = resolve_vol_target(&state, target) else {
        return;
    };
    send(Cmd::Mute { node: node.id, mute: !node.muted });
}

/// Make `target` the default mic. For bluetooth devices this routes through
/// the device façade: the default becomes the persistent virtual mic, and the
/// demand policy engages the headset profile only while something actually
/// captures. No profile churn at selection time.
pub fn select_mic(target: Target) -> Result<String, String> {
    let _guard = switch_guard()?;
    let state = query();
    let prev_bt = default_bt_device(&state, false);

    let bt = match target {
        Target::Node(id) => {
            let n = find_node(&state, false, id).ok_or("mic is gone")?;
            match bt_for_node(&state, n) {
                Some(bt) => bt.clone(),
                None => {
                    let name = n.name.clone();
                    set_default_and_wait(false, n.id)?;
                    set_preferred(None);
                    // The old mic's headset profile is telephone-grade for
                    // playback; give the device its audio quality back.
                    if let Some(prev) = prev_bt {
                        restore_a2dp(prev.device_id);
                    }
                    return Ok(format!("mic → {name}"));
                }
            }
        }
        Target::BtDev(id) => state
            .bt
            .iter()
            .find(|b| b.device_id == id)
            .ok_or("device is gone")?
            .clone(),
    };

    let f = ensure_facade(&bt)?;
    let mic = wait_for(Duration::from_secs(5), |s| {
        s.inputs.iter().find(|n| n.node_name == f.mic_name()).map(|n| n.id)
    })
    .map_err(|_| format!("{}: façade mic did not appear", bt.name))?;
    set_default_and_wait(false, mic)?;
    set_preferred(Some(Preferred { device_id: bt.device_id, source_node: f.mic_name() }));

    if let Some(prev) = prev_bt {
        if prev.device_id != bt.device_id {
            restore_a2dp(prev.device_id);
        }
    }
    Ok(format!("mic → {}", bt.name))
}

/// Make `target` the default speaker. Bluetooth devices route through the
/// façade sink, which survives profile flips — no profile action here; the
/// demand policy keeps hi-fi when the mic is idle.
pub fn select_speaker(target: Target) -> Result<String, String> {
    let _guard = switch_guard()?;
    let state = query();

    let bt = match target {
        Target::Node(id) => {
            let n = find_node(&state, true, id).ok_or("speaker is gone")?;
            match bt_for_node(&state, n) {
                Some(bt) => bt.clone(),
                None => {
                    set_default_and_wait(true, n.id)?;
                    return Ok(format!("speaker → {}", n.name));
                }
            }
        }
        Target::BtDev(id) => state
            .bt
            .iter()
            .find(|b| b.device_id == id)
            .ok_or("device is gone")?
            .clone(),
    };

    let f = ensure_facade(&bt)?;
    let sink = wait_for(Duration::from_secs(5), |s| {
        s.outputs.iter().find(|n| n.node_name == f.sink_name()).map(|n| n.id)
    })
    .map_err(|_| format!("{}: façade sink did not appear", bt.name))?;
    set_default_and_wait(true, sink)?;
    Ok(format!("speaker → {}", bt.name))
}

/// Explicit profile flip for the UI's `p` key — same guarantees as the
/// selectors, including the BlueZ reconnect when HFP is missing.
pub fn switch_profile(device_id: u32, kind: ProfileKind) -> Result<String, String> {
    let _guard = switch_guard()?;
    let state = query();
    let bt = state
        .bt
        .iter()
        .find(|b| b.device_id == device_id)
        .ok_or("device is gone")?
        .clone();
    hold_policy_for(device_id);
    engage_profile(&bt, kind)?;
    kick_parked(device_id);
    let label = match kind {
        ProfileKind::A2dp => "a2dp",
        ProfileKind::Hfp => "headset",
    };
    Ok(format!("{} → {label}", bt.name))
}

// ---- Device façade ----
//
// Phone-style device abstraction (the Android/CoreAudio move): apps and
// defaults only ever see a persistent virtual sink/source pair, built from a
// pipewire loopback child process (same mechanism as the denoise chain).
// Profile flips replumb the hidden backend streams while the app-facing
// nodes never die — playback survives and the a2dp/hfp duality disappears
// from the UI. Backend sides carry node.dont-fallback so audio stalls
// briefly mid-flip instead of leaking to another device.

#[derive(Clone)]
struct Facade {
    device_id: u32,
    mac: String,
}

impl Facade {
    fn sink_name(&self) -> String {
        format!("deck_sink.{}", self.mac)
    }
    fn sink_out_name(&self) -> String {
        format!("deck_sink_out.{}", self.mac)
    }
    fn mic_name(&self) -> String {
        format!("deck_mic.{}", self.mac)
    }
    fn mic_in_name(&self) -> String {
        format!("deck_mic_in.{}", self.mac)
    }
}

static FACADE: Mutex<Option<Facade>> = Mutex::new(None);

fn facade() -> Option<Facade> {
    FACADE.lock().unwrap().clone()
}

/// Underscored MAC from a BlueZ object path (`.../dev_14_3F_A6_C5_09_88`).
fn mac_from_path(path: &str) -> Option<String> {
    path.rsplit_once("dev_").map(|(_, m)| m.to_string())
}

fn facade_pid_path() -> std::path::PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(runtime).join("cyberdeck-facade.pid")
}

fn write_facade_config(mac: &str, desc: &str) -> Option<String> {
    let cache = std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.cache")
    });
    let conf_path = std::path::PathBuf::from(cache).join(format!("cyberdeck/facade-{mac}.conf"));
    let _ = std::fs::create_dir_all(conf_path.parent()?);
    // module-loopback self-destructs at init when a configured target.object
    // does not exist yet. The sink backend can carry a static target (the
    // bluez output node exists in every profile, and ensure_facade only
    // spawns while the device is connected) plus dont-fallback so flip gaps
    // stall instead of leaking to another device. The mic backend gets no
    // target — the headset-profile source only exists on demand — and is
    // pinned at runtime by the policy instead; wireplumber's link-group
    // exclusion keeps it from ever looping back onto the façade itself.
    let config = format!(
        r#"context.modules = [
{{  name = libpipewire-module-rt
    flags = [ ifexists nofail ]
}}
{{  name = libpipewire-module-protocol-native }}
{{  name = libpipewire-module-client-node }}
{{  name = libpipewire-module-adapter }}
{{  name = libpipewire-module-loopback
    args = {{
        capture.props = {{
            node.name = "deck_sink.{mac}"
            node.description = "{desc}"
            media.class = Audio/Sink
            audio.rate = 48000
            audio.position = [ FL FR ]
            node.latency = "256/48000"
            deck.facade = "{mac}"
        }}
        playback.props = {{
            node.name = "deck_sink_out.{mac}"
            node.description = "{desc} link out"
            target.object = "bluez_output.{mac}.1"
            node.dont-fallback = true
            node.passive = true
            audio.rate = 48000
            node.latency = "256/48000"
            deck.facade = "{mac}"
        }}
    }}
}}
{{  name = libpipewire-module-loopback
    args = {{
        capture.props = {{
            node.name = "deck_mic_in.{mac}"
            node.description = "{desc} link in"
            node.passive = true
            audio.rate = 48000
            node.latency = "256/48000"
            deck.facade = "{mac}"
        }}
        playback.props = {{
            node.name = "deck_mic.{mac}"
            node.description = "{desc}"
            media.class = Audio/Source
            audio.rate = 48000
            node.latency = "256/48000"
            deck.facade = "{mac}"
        }}
    }}
}}
]"#
    );
    std::fs::write(&conf_path, &config).ok()?;
    Some(conf_path.to_string_lossy().to_string())
}

/// Bring up (or adopt) the façade for a bluetooth device. One façade lives at
/// a time; selecting a different device replaces it.
fn ensure_facade(bt: &BtDevice) -> Result<Facade, String> {
    let mac = mac_from_path(&bt.path).ok_or_else(|| format!("{}: no mac in device path", bt.name))?;

    if let Some(f) = facade() {
        if f.mac == mac {
            if f.device_id != bt.device_id {
                *FACADE.lock().unwrap() = Some(Facade { device_id: bt.device_id, mac: mac.clone() });
            }
            return Ok(facade().unwrap());
        }
        teardown_facade();
    }

    // A child from a previous daemon run may still serve this device.
    let nodes_present = query()
        .outputs
        .iter()
        .any(|n| n.facade.as_deref() == Some(mac.as_str()));
    if !nodes_present {
        let conf = write_facade_config(&mac, &bt.name).ok_or("failed to write façade config")?;
        let child = std::process::Command::new("pipewire")
            .args(["-c", &conf])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to start façade: {e}"))?;
        let _ = std::fs::write(facade_pid_path(), child.id().to_string());
        log::info!("audio: façade for {} started (pid {})", bt.name, child.id());
    }

    let f = Facade { device_id: bt.device_id, mac };
    *FACADE.lock().unwrap() = Some(f.clone());
    wait_for(Duration::from_secs(5), |s| {
        s.outputs.iter().find(|n| n.node_name == f.sink_name()).map(drop)
    })
    .map_err(|_| format!("{}: façade nodes did not appear", bt.name))?;
    Ok(f)
}

fn teardown_facade() {
    if let Ok(pid_str) = std::fs::read_to_string(facade_pid_path()) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
    }
    let _ = std::fs::remove_file(facade_pid_path());
    *FACADE.lock().unwrap() = None;
}

/// Keep the façade's hidden backend streams pointed at the device's real
/// nodes. Pins by node name via metadata; `pinned` dedupes so a stable graph
/// costs nothing. The sink side usually matches the config's static target;
/// the mic side is discovered here when the headset profile first engages.
fn pin_facade_backends(state: &PwState, f: &Facade, pinned: &mut HashMap<u32, String>) {
    let mut pin = |stream: Option<&StreamInfo>, real: Option<&PwDevice>| {
        let (Some(st), Some(real)) = (stream, real) else { return };
        if pinned.get(&st.id) == Some(&real.node_name) {
            return;
        }
        if st.target.as_deref() == Some(real.node_name.as_str()) && !pinned.contains_key(&st.id) {
            pinned.insert(st.id, real.node_name.clone());
            return;
        }
        log::info!("audio: pinning {} -> {}", st.node_name, real.node_name);
        send(Cmd::Retarget { stream: st.id, serial: Some(real.node_name.clone()) });
        pinned.insert(st.id, real.node_name.clone());
    };
    pin(
        state.playback.iter().find(|s| s.node_name == f.sink_out_name()),
        state.outputs.iter().find(|n| n.device_id == Some(f.device_id)),
    );
    pin(
        state.capture.iter().find(|s| s.node_name == f.mic_in_name()),
        state.inputs.iter().find(|n| n.device_id == Some(f.device_id)),
    );
}

// ---- Demand policy ----
//
// The profile follows mic *usage*: selecting a bluetooth mic records intent
// (preferred mic); the profile itself dances with demand. Real capture
// streams appear → headset profile engages; the last one has been gone for
// RESTORE_AFTER → a2dp comes back. Manual profile actions set a hold that a
// fresh demand edge clears, so the user always wins the argument they just
// started.

const RESTORE_AFTER: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct Preferred {
    device_id: u32,
    source_node: String,
}

static PREFERRED: Mutex<Option<Preferred>> = Mutex::new(None);
static POLICY_HOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn hold_policy_for(device_id: u32) {
    let pref = PREFERRED.lock().unwrap();
    if pref.as_ref().map(|p| p.device_id) == Some(device_id) {
        POLICY_HOLD.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn set_preferred(pref: Option<Preferred>) {
    *PREFERRED.lock().unwrap() = pref;
    POLICY_HOLD.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// Does any live stream want the mic? Streams following the default count
/// always; explicitly-targeted streams only when aimed at the preferred mic.
fn mic_demand(state: &PwState, pref: &Preferred) -> bool {
    state.capture.iter().any(|c| {
        if c.passive || c.monitor {
            return false;
        }
        match &c.target {
            None => true,
            Some(t) => t == &pref.source_node,
        }
    })
}

/// Spawn the demand-follow policy. Daemon only — CLI one-shots must not run
/// a second policy actor against the same graph.
pub fn start_policy() {
    std::thread::Builder::new()
        .name("pw-policy".into())
        .spawn(policy_thread)
        .expect("failed to spawn pipewire policy");
}

fn policy_thread() {
    use std::sync::atomic::Ordering;

    let mut seen = 0u64;
    let mut demand_was = false;
    let mut quiet_since: Option<Instant> = None;
    let mut facade_gone_since: Option<Instant> = None;
    let mut pinned: HashMap<u32, String> = HashMap::new();

    loop {
        // The timeout doubles as the debounce tick when the graph is quiet.
        seen = wait_generation(seen, Duration::from_secs(2));
        let state = query();

        // Adopt a façade left behind by a previous daemon run.
        if facade().is_none() {
            if let Some(mac) = state.outputs.iter().find_map(|n| n.facade.clone()) {
                if let Some(bt) = state
                    .bt
                    .iter()
                    .find(|b| mac_from_path(&b.path).as_deref() == Some(mac.as_str()))
                {
                    log::info!("audio: adopting façade for {}", bt.name);
                    *FACADE.lock().unwrap() = Some(Facade { device_id: bt.device_id, mac });
                }
            }
        }

        if let Some(f) = facade() {
            let device_present = state.bt.iter().any(|b| b.device_id == f.device_id && b.connected);
            if device_present {
                facade_gone_since = None;
                pin_facade_backends(&state, &f, &mut pinned);
                // Selecting the façade mic in a previous daemon run survives
                // as the default; rebuild the preferred-mic state from it.
                if PREFERRED.lock().unwrap().is_none() {
                    let mic_default = state
                        .inputs
                        .iter()
                        .any(|n| n.node_name == f.mic_name() && n.is_default);
                    if mic_default {
                        set_preferred(Some(Preferred {
                            device_id: f.device_id,
                            source_node: f.mic_name(),
                        }));
                    }
                }
            } else {
                let since = *facade_gone_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_secs(20) {
                    log::info!("audio: façade device gone, tearing down");
                    teardown_facade();
                    facade_gone_since = None;
                    pinned.clear();
                }
            }
        }

        let Some(pref) = PREFERRED.lock().unwrap().clone() else {
            demand_was = false;
            quiet_since = None;
            continue;
        };
        let Some(bt) = state.bt.iter().find(|b| b.device_id == pref.device_id) else {
            continue;
        };
        if !bt.connected {
            continue;
        }

        let demand = mic_demand(&state, &pref);
        if demand {
            quiet_since = None;
            if !demand_was {
                // Fresh demand outranks any manual hold.
                POLICY_HOLD.store(false, Ordering::SeqCst);
            }
            demand_was = true;
            if !POLICY_HOLD.load(Ordering::SeqCst) && !bt.profile.starts_with("headset") {
                let Ok(_guard) = SWITCHING.try_lock() else { continue };
                match engage_profile(bt, ProfileKind::Hfp) {
                    Ok(()) => {
                        kick_parked(bt.device_id);
                        ipc::notify_bar("audio", &format!("mic engaged: {}", bt.name));
                    }
                    Err(e) => ipc::notify_bar("audio", &e),
                }
            }
        } else {
            demand_was = false;
            if bt.profile.starts_with("headset") && !POLICY_HOLD.load(Ordering::SeqCst) {
                let since = *quiet_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= RESTORE_AFTER {
                    quiet_since = None;
                    let Ok(_guard) = SWITCHING.try_lock() else { continue };
                    restore_a2dp(bt.device_id);
                    kick_parked(bt.device_id);
                    ipc::notify_bar("audio", &format!("{}: back to hi-fi (mic idle)", bt.name));
                }
            } else {
                quiet_since = None;
            }
        }
    }
}

// ---- Orchestration internals ----

static SWITCHING: Mutex<()> = Mutex::new(());

fn switch_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    SWITCHING
        .try_lock()
        .map_err(|_| "another audio switch is in progress".to_string())
}

fn find_node(state: &PwState, sink: bool, id: u32) -> Option<&PwDevice> {
    let list = if sink { &state.outputs } else { &state.inputs };
    list.iter().find(|n| n.id == id)
}

fn bt_for_node<'a>(state: &'a PwState, node: &PwDevice) -> Option<&'a BtDevice> {
    let dev = node.device_id?;
    state.bt.iter().find(|b| b.device_id == dev)
}

fn default_bt_device(state: &PwState, sink: bool) -> Option<BtDevice> {
    let list = if sink { &state.outputs } else { &state.inputs };
    let node = list.iter().find(|n| n.is_default)?;
    bt_for_node(state, node).cloned()
}

fn pick_profile(profiles: &[BtProfile], kind: ProfileKind) -> Option<BtProfile> {
    let (exact, prefix) = match kind {
        ProfileKind::A2dp => ("a2dp-sink", "a2dp-sink"),
        ProfileKind::Hfp => ("headset-head-unit", "headset"),
    };
    let usable = |p: &&BtProfile| p.available;
    profiles
        .iter()
        .filter(usable)
        .find(|p| p.name == exact)
        .or_else(|| profiles.iter().filter(usable).find(|p| p.name.starts_with(prefix)))
        .cloned()
}

/// Ensure `kind` is the device's active profile, restoring the profile via a
/// BlueZ connect when it is missing entirely (the state a failed auto-connect
/// leaves behind: device up, Handsfree service never established).
fn engage_profile(bt: &BtDevice, kind: ProfileKind) -> Result<(), String> {
    let mut current = bt.clone();

    if pick_profile(&current.profiles, kind).is_none() {
        if kind == ProfileKind::A2dp {
            return Err(format!("{}: no a2dp profile offered", bt.name));
        }
        ipc::notify_bar("audio", &format!("{}: restoring mic profile…", bt.name));
        bluetooth::ensure_connected(&bt.path)
            .map_err(|e| format!("{}: bluetooth connect failed: {e}", bt.name))?;
        current = wait_for(Duration::from_secs(8), |s| {
            s.bt.iter()
                .find(|b| b.device_id == bt.device_id && pick_profile(&b.profiles, kind).is_some())
                .cloned()
        })
        .map_err(|_| {
            format!("{}: headset profile still missing after reconnect — power-cycle the earbuds", bt.name)
        })?;
    }

    let profile = pick_profile(&current.profiles, kind).unwrap();
    if current.profile == profile.name {
        return Ok(());
    }
    send(Cmd::Profile { device: bt.device_id, index: profile.index });
    wait_for(Duration::from_secs(5), |s| {
        s.bt.iter()
            .find(|b| b.device_id == bt.device_id && b.profile == profile.name)
            .map(drop)
    })
    .map_err(|_| format!("{}: profile switch to {} did not stick", bt.name, profile.name))
}

fn set_default_and_wait(sink: bool, node_id: u32) -> Result<(), String> {
    let state = query();
    let node = find_node(&state, sink, node_id).ok_or("node is gone")?;
    let name = node.node_name.clone();
    send(Cmd::Default { sink, node_name: name.clone() });
    wait_for(Duration::from_secs(3), |s| {
        let list = if sink { &s.outputs } else { &s.inputs };
        list.iter().find(|n| n.id == node_id && n.is_default).map(drop)
    })
    .map_err(|_| "default did not move — is WirePlumber running?".to_string())
}

/// A profile switch tears the device's sink node down and recreates it; app
/// streams that were playing get re-linked by WirePlumber but stay parked in
/// `suspended` and never resume on their own (and nothing else un-parks
/// them — macOS and Windows both punt here too). Once the new sink is up and
/// default, nudge every parked default-following playback stream with a
/// retarget-then-clear so it resumes, still following the default afterwards.
fn kick_parked(device_id: u32) {
    std::thread::spawn(move || {
        let Ok(sink) = wait_for(Duration::from_secs(6), |s| {
            s.outputs
                .iter()
                .find(|n| n.device_id == Some(device_id) && !n.serial.is_empty())
                .cloned()
        }) else {
            return;
        };
        let f = facade().filter(|f| f.device_id == device_id);
        if !sink.is_default && f.is_none() {
            return;
        }
        // Give WirePlumber a beat to finish its own re-linking pass.
        std::thread::sleep(Duration::from_millis(400));
        let state = query();
        // Default-following app streams, plus the façade's own backend
        // streams (their conf target survives the clear below).
        let is_facade_backend = |st: &StreamInfo| {
            f.as_ref()
                .map(|f| st.node_name == f.sink_out_name() || st.node_name == f.mic_in_name())
                .unwrap_or(false)
        };
        let parked: Vec<u32> = state
            .playback
            .iter()
            .chain(&state.capture)
            .filter(|st| {
                (st.target.is_none() || is_facade_backend(st))
                    && !st.monitor
                    && st.state != "running"
                    && (!st.passive || is_facade_backend(st))
            })
            .map(|st| st.id)
            .collect();
        if parked.is_empty() {
            return;
        }
        log::info!("audio: kicking {} parked stream(s) onto {}", parked.len(), sink.name);
        for id in &parked {
            send(Cmd::Retarget { stream: *id, serial: Some(sink.serial.clone()) });
        }
        std::thread::sleep(Duration::from_millis(500));
        for id in &parked {
            send(Cmd::Retarget { stream: *id, serial: None });
        }
    });
}

fn restore_a2dp(device_id: u32) {
    let state = query();
    let Some(bt) = state.bt.iter().find(|b| b.device_id == device_id) else {
        return;
    };
    if !bt.profile.starts_with("headset") {
        return;
    }
    if let Some(p) = pick_profile(&bt.profiles, ProfileKind::A2dp) {
        log::info!("audio: restoring {} to {}", bt.name, p.name);
        send(Cmd::Profile { device: device_id, index: p.index });
    }
}

fn resolve_vol_target(state: &PwState, target: VolTarget) -> Option<PwDevice> {
    let (node, is_sink) = match target {
        VolTarget::Node(id) => {
            if let Some(n) = state.outputs.iter().find(|n| n.id == id) {
                (n.clone(), true)
            } else {
                (state.inputs.iter().find(|n| n.id == id)?.clone(), false)
            }
        }
        VolTarget::DefaultSink => (state.outputs.iter().find(|n| n.is_default)?.clone(), true),
        VolTarget::DefaultSource => (state.inputs.iter().find(|n| n.is_default)?.clone(), false),
    };
    // Volume on a façade node writes through to the real device node so the
    // headset's hardware volume moves, not a software gain on the loopback.
    if let (Some(mac), Some((dev_id, fmac))) = (&node.facade, &state.facade) {
        if mac == fmac {
            let list = if is_sink { &state.outputs } else { &state.inputs };
            if let Some(real) = list.iter().find(|n| n.device_id == Some(*dev_id)) {
                return Some(real.clone());
            }
        }
    }
    Some(node)
}

// ---- Backend plumbing ----

enum Cmd {
    Profile { device: u32, index: u32 },
    Default { sink: bool, node_name: String },
    Volume { node: u32, pct: i64 },
    Mute { node: u32, mute: bool },
    Retarget { stream: u32, serial: Option<String> },
}

struct Backend {
    state: RwLock<PwState>,
    gen: Mutex<u64>,
    cv: Condvar,
    tx: Mutex<pw::channel::Sender<Cmd>>,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

fn backend() -> &'static Backend {
    BACKEND.get().expect("pipewire backend not started")
}

fn send(cmd: Cmd) {
    if backend().tx.lock().unwrap().send(cmd).is_err() {
        log::error!("pipewire: monitor loop gone, command dropped");
    }
}

fn publish(state: PwState) {
    let b = backend();
    *b.state.write().unwrap() = state;
    let mut gen = b.gen.lock().unwrap();
    *gen += 1;
    b.cv.notify_all();
}

fn wait_for<T>(timeout: Duration, f: impl Fn(&PwState) -> Option<T>) -> Result<T, ()> {
    let b = backend();
    let deadline = Instant::now() + timeout;
    let mut gen = b.gen.lock().unwrap();
    loop {
        if let Some(v) = f(&b.state.read().unwrap()) {
            return Ok(v);
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(());
        }
        let (g, _) = b.cv.wait_timeout(gen, deadline - now).unwrap();
        gen = g;
    }
}

// ---- Monitor thread ----

fn monitor_thread(mut rx: pw::channel::Receiver<Cmd>) {
    loop {
        match run_loop(rx) {
            Ok(returned) => rx = returned,
            Err(e) => {
                log::error!("pipewire: monitor loop failed fatally: {e}");
                return;
            }
        }
        publish(PwState::default());
        log::warn!("pipewire: connection lost, reconnecting in 2s");
        std::thread::sleep(Duration::from_secs(2));
    }
}

struct NodeEntry {
    name: String,
    node_name: String,
    serial: String,
    is_sink: bool,
    device_id: Option<u32>,
    facade: Option<String>,
    state: String,
    volume: i64,
    muted: bool,
    channels: usize,
    proxy: pw::node::Node,
    _listener: pw::node::NodeListener,
}

struct DeviceEntry {
    name: String,
    path: String,
    connected: bool,
    profile: String,
    profiles: Vec<BtProfile>,
    proxy: pw::device::Device,
    _listener: pw::device::DeviceListener,
}

struct StreamEntry {
    output: bool,
    node_name: String,
    target: Option<String>,
    passive: bool,
    monitor: bool,
    state: String,
    _proxy: pw::node::Node,
    _listener: pw::node::NodeListener,
}

#[derive(Default)]
struct Local {
    nodes: HashMap<u32, NodeEntry>,
    devices: HashMap<u32, DeviceEntry>,
    streams: HashMap<u32, StreamEntry>,
    default_sink: Option<String>,
    default_source: Option<String>,
}

impl Local {
    fn snapshot(&self) -> PwState {
        let mut outputs = Vec::new();
        let mut inputs = Vec::new();
        for (id, n) in &self.nodes {
            let is_default = if n.is_sink {
                self.default_sink.as_deref() == Some(n.node_name.as_str())
            } else {
                self.default_source.as_deref() == Some(n.node_name.as_str())
            };
            let dev = PwDevice {
                id: *id,
                name: n.name.clone(),
                node_name: n.node_name.clone(),
                serial: n.serial.clone(),
                volume: n.volume,
                muted: n.muted,
                is_default,
                device_id: n.device_id,
                facade: n.facade.clone(),
                state: n.state.clone(),
            };
            if n.is_sink {
                outputs.push(dev);
            } else {
                inputs.push(dev);
            }
        }
        outputs.sort_by_key(|d| d.id);
        inputs.sort_by_key(|d| d.id);
        let mut bt: Vec<BtDevice> = self
            .devices
            .iter()
            .map(|(id, d)| BtDevice {
                device_id: *id,
                name: d.name.clone(),
                path: d.path.clone(),
                connected: d.connected,
                profile: d.profile.clone(),
                profiles: d.profiles.clone(),
            })
            .collect();
        bt.sort_by_key(|d| d.device_id);
        let stream_info = |(id, s): (&u32, &StreamEntry)| StreamInfo {
            id: *id,
            node_name: s.node_name.clone(),
            target: s.target.clone(),
            passive: s.passive,
            monitor: s.monitor,
            state: s.state.clone(),
        };
        let capture = self.streams.iter().filter(|(_, s)| !s.output).map(stream_info).collect();
        let playback = self.streams.iter().filter(|(_, s)| s.output).map(stream_info).collect();
        let denoise = self.nodes.values().any(|n| n.node_name == "rnnoise_source");
        let facade = facade().map(|f| (f.device_id, f.mac));
        PwState { outputs, inputs, bt, capture, playback, facade, denoise }
    }
}

fn run_loop(rx: pw::channel::Receiver<Cmd>) -> Result<pw::channel::Receiver<Cmd>, String> {
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::Context::new(&mainloop).map_err(|e| e.to_string())?;
    let core = context.connect(None).map_err(|e| e.to_string())?;
    let registry = Rc::new(core.get_registry().map_err(|e| e.to_string())?);

    let local: Rc<RefCell<Local>> = Rc::default();
    let metadata: Rc<RefCell<Option<(u32, pw::metadata::Metadata, pw::metadata::MetadataListener)>>> =
        Rc::default();

    let _core_listener = {
        let mainloop = mainloop.clone();
        core.add_listener_local()
            .error(move |id, _seq, res, message| {
                if id == 0 {
                    log::warn!("pipewire: core error {res}: {message}");
                    mainloop.quit();
                }
            })
            .register()
    };

    let _registry_listener = {
        let registry_weak = Rc::downgrade(&registry);
        let local_add = local.clone();
        let metadata_add = metadata.clone();
        let local_rm = local.clone();
        let metadata_rm = metadata.clone();
        registry
            .add_listener_local()
            .global(move |global| {
                let local = &local_add;
                let metadata = &metadata_add;
                let Some(registry) = registry_weak.upgrade() else {
                    return;
                };
                let props = match global.props {
                    Some(p) => p,
                    None => return,
                };
                match global.type_ {
                    pw::types::ObjectType::Node => {
                        let class = props.get("media.class").unwrap_or("");
                        if class == "Stream/Input/Audio" || class == "Stream/Output/Audio" {
                            let output = class == "Stream/Output/Audio";
                            if let Err(e) = track_stream(&registry, global, output, local) {
                                log::warn!("pipewire: failed to track stream {}: {e}", global.id);
                            }
                            return;
                        }
                        if class != "Audio/Sink" && class != "Audio/Source" {
                            return;
                        }
                        if let Err(e) = track_node(&registry, global, class == "Audio/Sink", local) {
                            log::warn!("pipewire: failed to track node {}: {e}", global.id);
                        }
                    }
                    pw::types::ObjectType::Device => {
                        if props.get("device.api") != Some("bluez5") {
                            return;
                        }
                        if let Err(e) = track_device(&registry, global, local) {
                            log::warn!("pipewire: failed to track device {}: {e}", global.id);
                        }
                    }
                    pw::types::ObjectType::Metadata => {
                        if props.get("metadata.name") != Some("default") {
                            return;
                        }
                        match track_metadata(&registry, global, local) {
                            Ok(entry) => *metadata.borrow_mut() = Some(entry),
                            Err(e) => log::warn!("pipewire: failed to track metadata: {e}"),
                        }
                    }
                    _ => {}
                }
            })
            .global_remove(move |id| {
                let mut l = local_rm.borrow_mut();
                l.nodes.remove(&id);
                l.devices.remove(&id);
                l.streams.remove(&id);
                let snap = l.snapshot();
                drop(l);
                let is_meta = metadata_rm.borrow().as_ref().map(|(mid, ..)| *mid == id).unwrap_or(false);
                if is_meta {
                    *metadata_rm.borrow_mut() = None;
                }
                publish(snap);
            })
            .register()
    };

    let rx_guard = rx.attach(mainloop.loop_(), {
        let local = local.clone();
        let metadata = metadata.clone();
        move |cmd| handle_cmd(&local, &metadata, cmd)
    });

    mainloop.run();

    Ok(rx_guard.deattach())
}

fn track_node(
    registry: &pw::registry::Registry,
    global: &pw::registry::GlobalObject<&libspa::utils::dict::DictRef>,
    is_sink: bool,
    local: &Rc<RefCell<Local>>,
) -> Result<(), pw::Error> {
    let id = global.id;
    let node: pw::node::Node = registry.bind(global)?;

    let listener = node
        .add_listener_local()
        .info({
            let local = local.clone();
            move |info| {
                let mut l = local.borrow_mut();
                if let Some(entry) = l.nodes.get_mut(&id) {
                    entry.state = node_state_str(&info.state());
                    if let Some(props) = info.props() {
                        if let Some(name) = props.get("node.description").or_else(|| props.get("node.nick")) {
                            entry.name = name.to_string();
                        }
                        if let Some(name) = props.get("node.name") {
                            entry.node_name = name.to_string();
                        }
                        if let Some(serial) = props.get("object.serial") {
                            entry.serial = serial.to_string();
                        }
                        entry.device_id = props.get("device.id").and_then(|v| v.parse().ok());
                        entry.facade = props.get("deck.facade").map(|s| s.to_string());
                    }
                    // subscribe_params only pushes the initial values; changes
                    // are announced via PARAMS here and must be re-enumerated.
                    if info.change_mask().contains(pw::node::NodeChangeMask::PARAMS) {
                        entry.proxy.enum_params(0, Some(ParamType::Props), 0, u32::MAX);
                    }
                    let snap = l.snapshot();
                    drop(l);
                    publish(snap);
                }
            }
        })
        .param({
            let local = local.clone();
            move |_seq, param_id, _index, _next, param| {
                if param_id != ParamType::Props {
                    return;
                }
                let Some(param) = param else { return };
                let Some((volume, muted, channels)) = parse_props(param) else {
                    return;
                };
                let mut l = local.borrow_mut();
                if let Some(entry) = l.nodes.get_mut(&id) {
                    entry.volume = volume;
                    entry.muted = muted;
                    entry.channels = channels;
                    let snap = l.snapshot();
                    drop(l);
                    publish(snap);
                }
            }
        })
        .register();

    node.subscribe_params(&[ParamType::Props]);

    local.borrow_mut().nodes.insert(
        id,
        NodeEntry {
            name: String::new(),
            node_name: String::new(),
            serial: String::new(),
            is_sink,
            device_id: None,
            facade: None,
            state: String::new(),
            volume: 0,
            muted: false,
            channels: 2,
            proxy: node,
            _listener: listener,
        },
    );
    Ok(())
}

fn track_device(
    registry: &pw::registry::Registry,
    global: &pw::registry::GlobalObject<&libspa::utils::dict::DictRef>,
    local: &Rc<RefCell<Local>>,
) -> Result<(), pw::Error> {
    let id = global.id;
    let device: pw::device::Device = registry.bind(global)?;

    let listener = device
        .add_listener_local()
        .info({
            let local = local.clone();
            move |info| {
                let mut l = local.borrow_mut();
                if let Some(entry) = l.devices.get_mut(&id) {
                    if let Some(props) = info.props() {
                        if let Some(name) = props.get("device.description").or_else(|| props.get("device.alias")) {
                            entry.name = name.to_string();
                        }
                        if let Some(path) = props.get("api.bluez5.path") {
                            entry.path = path.to_string();
                        }
                        // Partial info updates omit the key; only a present
                        // value may change the flag.
                        if let Some(conn) = props.get("api.bluez5.connection") {
                            entry.connected = conn == "connected";
                        }
                    }
                    // subscribe_params only pushes the initial values; changes
                    // are announced via PARAMS here and must be re-enumerated.
                    if info.change_mask().contains(pw::device::DeviceChangeMask::PARAMS) {
                        entry.proxy.enum_params(0, Some(ParamType::Profile), 0, u32::MAX);
                        entry.proxy.enum_params(0, Some(ParamType::EnumProfile), 0, u32::MAX);
                    }
                    let snap = l.snapshot();
                    drop(l);
                    publish(snap);
                }
            }
        })
        .param({
            let local = local.clone();
            move |_seq, param_id, index, _next, param| {
                let mut l = local.borrow_mut();
                let Some(entry) = l.devices.get_mut(&id) else { return };
                match param_id {
                    ParamType::Profile => {
                        if let Some(p) = param.and_then(parse_profile) {
                            entry.profile = p.name;
                        }
                    }
                    ParamType::EnumProfile => {
                        if index == 0 {
                            entry.profiles.clear();
                        }
                        if let Some(p) = param.and_then(parse_profile) {
                            entry.profiles.push(p);
                        }
                    }
                    _ => return,
                }
                let snap = l.snapshot();
                drop(l);
                publish(snap);
            }
        })
        .register();

    device.subscribe_params(&[ParamType::Profile, ParamType::EnumProfile]);

    local.borrow_mut().devices.insert(
        id,
        DeviceEntry {
            name: String::new(),
            path: String::new(),
            connected: false,
            profile: "off".into(),
            profiles: Vec::new(),
            proxy: device,
            _listener: listener,
        },
    );
    Ok(())
}

fn track_stream(
    registry: &pw::registry::Registry,
    global: &pw::registry::GlobalObject<&libspa::utils::dict::DictRef>,
    output: bool,
    local: &Rc<RefCell<Local>>,
) -> Result<(), pw::Error> {
    let id = global.id;
    let node: pw::node::Node = registry.bind(global)?;

    let listener = node
        .add_listener_local()
        .info({
            let local = local.clone();
            move |info| {
                let mut l = local.borrow_mut();
                if let Some(entry) = l.streams.get_mut(&id) {
                    entry.state = node_state_str(&info.state());
                    if let Some(props) = info.props() {
                        if let Some(name) = props.get("node.name") {
                            entry.node_name = name.to_string();
                        }
                        entry.target = props
                            .get("target.object")
                            .or_else(|| props.get("node.target"))
                            .map(|s| s.to_string());
                        entry.monitor = props.get("stream.monitor") == Some("true");
                        entry.passive = matches!(props.get("node.passive"), Some("true" | "in" | "out"));
                    }
                    let snap = l.snapshot();
                    drop(l);
                    publish(snap);
                }
            }
        })
        .register();

    local.borrow_mut().streams.insert(
        id,
        StreamEntry {
            output,
            node_name: String::new(),
            target: None,
            passive: false,
            monitor: false,
            state: String::new(),
            _proxy: node,
            _listener: listener,
        },
    );
    Ok(())
}

type MetaEntry = (u32, pw::metadata::Metadata, pw::metadata::MetadataListener);

fn track_metadata(
    registry: &pw::registry::Registry,
    global: &pw::registry::GlobalObject<&libspa::utils::dict::DictRef>,
    local: &Rc<RefCell<Local>>,
) -> Result<MetaEntry, pw::Error> {
    let id = global.id;
    let meta: pw::metadata::Metadata = registry.bind(global)?;

    let listener = meta
        .add_listener_local()
        .property({
            let local = local.clone();
            move |_subject, key, _type, value| {
                let slot = match key {
                    Some("default.audio.sink") => 0,
                    Some("default.audio.source") => 1,
                    _ => return 0,
                };
                let name = value.and_then(|v| {
                    serde_json::from_str::<serde_json::Value>(v)
                        .ok()?
                        .get("name")?
                        .as_str()
                        .map(|s| s.to_string())
                });
                let mut l = local.borrow_mut();
                if slot == 0 {
                    l.default_sink = name;
                } else {
                    l.default_source = name;
                }
                let snap = l.snapshot();
                drop(l);
                publish(snap);
                0
            }
        })
        .register();

    Ok((id, meta, listener))
}

fn handle_cmd(local: &Rc<RefCell<Local>>, metadata: &Rc<RefCell<Option<MetaEntry>>>, cmd: Cmd) {
    match cmd {
        Cmd::Profile { device, index } => {
            let l = local.borrow();
            let Some(entry) = l.devices.get(&device) else { return };
            let pod_bytes = build_pod(Value::Object(Object {
                type_: libspa_sys::SPA_TYPE_OBJECT_ParamProfile,
                id: libspa_sys::SPA_PARAM_Profile,
                properties: vec![
                    prop(libspa_sys::SPA_PARAM_PROFILE_index, Value::Int(index as i32)),
                    prop(libspa_sys::SPA_PARAM_PROFILE_save, Value::Bool(true)),
                ],
            }));
            if let Some(pod) = pod_bytes.as_deref().and_then(Pod::from_bytes) {
                entry.proxy.set_param(ParamType::Profile, 0, pod);
            }
        }
        Cmd::Default { sink, node_name } => {
            let meta = metadata.borrow();
            let Some((_, meta, _)) = meta.as_ref() else {
                log::warn!("pipewire: no default metadata object, cannot set default");
                return;
            };
            let key = if sink { "default.configured.audio.sink" } else { "default.configured.audio.source" };
            let value = serde_json::json!({ "name": node_name }).to_string();
            meta.set_property(0, key, Some("Spa:String:JSON"), Some(&value));
        }
        Cmd::Volume { node, pct } => {
            let l = local.borrow();
            let Some(entry) = l.nodes.get(&node) else { return };
            let linear = ((pct.max(0) as f32) / 100.0).powi(3);
            let channels = entry.channels.max(1);
            let pod_bytes = build_pod(Value::Object(Object {
                type_: libspa_sys::SPA_TYPE_OBJECT_Props,
                id: libspa_sys::SPA_PARAM_Props,
                properties: vec![prop(
                    libspa_sys::SPA_PROP_channelVolumes,
                    Value::ValueArray(ValueArray::Float(vec![linear; channels])),
                )],
            }));
            if let Some(pod) = pod_bytes.as_deref().and_then(Pod::from_bytes) {
                entry.proxy.set_param(ParamType::Props, 0, pod);
            }
        }
        Cmd::Retarget { stream, serial } => {
            let meta = metadata.borrow();
            let Some((_, meta, _)) = meta.as_ref() else { return };
            match serial {
                Some(s) => {
                    // Serials are numeric; node names are plain strings.
                    let type_ = if s.chars().all(|c| c.is_ascii_digit()) { Some("Spa:Id") } else { None };
                    meta.set_property(stream, "target.object", type_, Some(&s));
                }
                None => {
                    meta.set_property(stream, "target.object", None, None);
                }
            };
        }
        Cmd::Mute { node, mute } => {
            let l = local.borrow();
            let Some(entry) = l.nodes.get(&node) else { return };
            let pod_bytes = build_pod(Value::Object(Object {
                type_: libspa_sys::SPA_TYPE_OBJECT_Props,
                id: libspa_sys::SPA_PARAM_Props,
                properties: vec![prop(libspa_sys::SPA_PROP_mute, Value::Bool(mute))],
            }));
            if let Some(pod) = pod_bytes.as_deref().and_then(Pod::from_bytes) {
                entry.proxy.set_param(ParamType::Props, 0, pod);
            }
        }
    }
}

// ---- Pod helpers ----

fn prop(key: u32, value: Value) -> Property {
    Property { key, flags: libspa::pod::PropertyFlags::empty(), value }
}

fn build_pod(value: Value) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    PodSerializer::serialize(Cursor::new(&mut bytes), &value).ok()?;
    Some(bytes)
}

fn parse_pod(pod: &Pod) -> Option<Value> {
    PodDeserializer::deserialize_any_from(pod.as_bytes())
        .map(|(_, v)| v)
        .ok()
}

fn parse_profile(pod: &Pod) -> Option<BtProfile> {
    let Value::Object(obj) = parse_pod(pod)? else {
        return None;
    };
    let mut index = None;
    let mut name = None;
    let mut available = true;
    for p in obj.properties {
        match p.key {
            k if k == libspa_sys::SPA_PARAM_PROFILE_index => {
                if let Value::Int(i) = p.value {
                    index = Some(i as u32);
                }
            }
            k if k == libspa_sys::SPA_PARAM_PROFILE_name => {
                if let Value::String(s) = p.value {
                    name = Some(s);
                }
            }
            k if k == libspa_sys::SPA_PARAM_PROFILE_available => {
                if let Value::Id(id) = p.value {
                    available = id.0 != libspa_sys::SPA_PARAM_AVAILABILITY_no;
                }
            }
            _ => {}
        }
    }
    Some(BtProfile { index: index?, name: name?, available })
}

fn parse_props(pod: &Pod) -> Option<(i64, bool, usize)> {
    let Value::Object(obj) = parse_pod(pod)? else {
        return None;
    };
    let mut volumes: Option<Vec<f32>> = None;
    let mut muted = false;
    for p in obj.properties {
        match p.key {
            k if k == libspa_sys::SPA_PROP_channelVolumes => {
                if let Value::ValueArray(ValueArray::Float(v)) = p.value {
                    volumes = Some(v);
                }
            }
            k if k == libspa_sys::SPA_PROP_mute => {
                if let Value::Bool(b) = p.value {
                    muted = b;
                }
            }
            _ => {}
        }
    }
    let volumes = volumes?;
    let channels = volumes.len().max(1);
    let avg = volumes.iter().sum::<f32>() / channels as f32;
    let pct = ((avg as f64).cbrt() * 100.0).round() as i64;
    Some((pct.clamp(0, 150), muted, channels))
}

fn node_state_str(state: &pw::node::NodeState) -> String {
    match state {
        pw::node::NodeState::Error(_) => "error",
        pw::node::NodeState::Creating => "creating",
        pw::node::NodeState::Suspended => "suspended",
        pw::node::NodeState::Idle => "idle",
        pw::node::NodeState::Running => "running",
    }
    .to_string()
}
