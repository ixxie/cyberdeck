//! Bluetooth audio watchdog.
//!
//! Recovers two concrete failure states observed in the wild, both of which
//! otherwise persist silently until the user notices their mic is dead:
//!
//! 1. A bluez audio node stuck in `error` state — the HFP/A2DP transport
//!    died but the device is still "connected", so PipeWire keeps routing
//!    to a node that produces silence.
//! 2. A connected device that advertises Handsfree/Headset service UUIDs
//!    but has no headset profile registered in PipeWire — profile
//!    registration failed at connect time, so the mic never appears and
//!    WirePlumber's autoswitch has nothing to switch to.
//!
//! The only remedy that works for both is a bluetooth reconnect cycle.
//! Deliberately restrained: fixed debounces, one action, a cooldown, and a
//! hard cap of two attempts per unhealthy episode. The attempt counter only
//! resets after the device is seen healthy, so the watchdog cannot loop on
//! its own reconnects. Past the cap it toasts once and goes quiet.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::ipc;
use crate::mods::bluetooth;
use crate::pipewire;

const POLL: Duration = Duration::from_secs(3);
const ERROR_DEBOUNCE: Duration = Duration::from_secs(5);
const PROFILE_GRACE: Duration = Duration::from_secs(20);
const COOLDOWN: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: u32 = 2;

// Handsfree (0x111e) and Headset (0x1108) service class UUIDs
const MIC_UUID_PREFIXES: [&str; 2] = ["0000111e", "00001108"];

#[derive(Default)]
struct Entry {
    unhealthy_since: Option<Instant>,
    last_attempt: Option<Instant>,
    attempts: u32,
    warned: bool,
    has_mic: Option<bool>,
}

pub fn spawn() {
    std::thread::Builder::new()
        .name("bt-audio-watchdog".into())
        .spawn(run)
        .expect("failed to spawn watchdog thread");
}

fn run() {
    let mut entries: HashMap<String, Entry> = HashMap::new();
    loop {
        std::thread::sleep(POLL);
        let pw = pipewire::query();

        for bt in &pw.bt {
            if bt.path.is_empty() || !bt.connected {
                continue;
            }
            let entry = entries.entry(bt.path.clone()).or_default();

            let errored = pw.outputs.iter().chain(&pw.inputs)
                .any(|n| n.device_id == Some(bt.device_id) && n.state == "error");

            let has_mic = *entry.has_mic.get_or_insert_with(|| {
                bluetooth::device_uuids(&bt.path).iter().any(|u| {
                    MIC_UUID_PREFIXES.iter().any(|p| u.starts_with(p))
                })
            });
            let mic_missing = has_mic
                && !bt.profiles.iter().any(|p| p.name.starts_with("headset"));

            if !errored && !mic_missing {
                // Keep last_attempt so the cooldown still rate-limits a
                // flapping transport across healthy/unhealthy episodes.
                *entry = Entry {
                    has_mic: entry.has_mic,
                    last_attempt: entry.last_attempt,
                    ..Default::default()
                };
                continue;
            }

            let since = *entry.unhealthy_since.get_or_insert_with(Instant::now);
            let debounce = if errored { ERROR_DEBOUNCE } else { PROFILE_GRACE };
            if since.elapsed() < debounce {
                continue;
            }
            if let Some(t) = entry.last_attempt {
                if t.elapsed() < COOLDOWN {
                    continue;
                }
            }
            if entry.attempts >= MAX_ATTEMPTS {
                if !entry.warned {
                    entry.warned = true;
                    let reason = if errored { "transport error" } else { "no mic profile" };
                    log::warn!("watchdog: giving up on {} ({reason}), reconnect manually", bt.name);
                    ipc::notify_bar("bluetooth", &format!("{} audio needs manual reconnect", bt.name));
                }
                continue;
            }

            let reason = if errored { "transport error" } else { "missing headset profile" };
            log::info!("watchdog: reconnecting {} ({reason}), attempt {}", bt.name, entry.attempts + 1);
            ipc::notify_bar("bluetooth", &format!("reconnecting {} audio", bt.name));

            entry.attempts += 1;
            entry.last_attempt = Some(Instant::now());
            entry.unhealthy_since = None;

            if let Err(e) = bluetooth::cycle_connection(&bt.path) {
                // Some earbuds sleep after a disconnect and refuse
                // host-initiated connects until physically woken — tell the
                // user, since the device is now fully offline.
                log::warn!("watchdog: reconnect of {} failed: {e}", bt.name);
                ipc::notify_bar("bluetooth", &format!("{} offline — wake it manually", bt.name));
            }
            pipewire::invalidate();
        }
    }
}
