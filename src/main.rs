mod bar;
mod color;
mod nav;
mod config;
mod layout;
mod icons;
mod ipc;
mod mods;
mod render;
mod source;
mod template;
mod view;

use std::sync::atomic::{AtomicBool, Ordering};

use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{globals::registry_queue_init, Connection};

static RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn handle_signal(_sig: libc::c_int) {
    RUNNING.store(false, Ordering::Relaxed);
}

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().skip(1).collect();

    // Client mode: known IPC command as first arg
    if matches!(
        args.first().map(|s| s.as_str()),
        Some("launcher" | "dismiss" | "push" | "pop" | "navigate" | "state" | "run" | "type" | "key")
    ) {
        ipc::send_command(&args[0], &args[1..]);
        return;
    }

    // Mod command mode: cyberdeck <mod> <command> [args...]
    if args.len() >= 2 && !args[0].starts_with('-') {
        let config = config::Config::load(None).unwrap_or_else(|e| {
            eprintln!("error loading config: {e}");
            std::process::exit(1);
        });

        // Native mod commands
        if args[0] == "wallpaper" {
            let params: serde_json::Map<String, serde_json::Value> = config.bar.modules
                .get("wallpaper")
                .map(|m| m.params.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            match args[1].as_str() {
                "shuffle" => {
                    let group = args.get(2).map(|s| s.as_str());
                    mods::wallpaper::shuffle(&params, group);
                }
                "init" => mods::wallpaper::init(&params),
                _ => eprintln!("unknown wallpaper command: {}", args[1]),
            }
            return;
        }

        // Shell-based mod commands (fallback for non-native mods)
        if let Some(cmd) = config.find_command(&args[0], &args[1]) {
            let full_cmd = if args.len() > 2 {
                format!("{} {}", cmd, args[2..].join(" "))
            } else {
                cmd.to_string()
            };
            let status = std::process::Command::new("sh")
                .args(["-c", &full_cmd])
                .status()
                .unwrap_or_else(|e| {
                    eprintln!("failed to run '{}': {e}", full_cmd);
                    std::process::exit(1);
                });
            std::process::exit(status.code().unwrap_or(1));
        }
    }

    // Daemon mode
    unsafe {
        libc::signal(libc::SIGINT, handle_signal as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle_signal as libc::sighandler_t);
    }

    let config_path = if args.first().map(|s| s.as_str()) == Some("--config") {
        args.get(1).map(|s| s.as_str())
    } else {
        None
    };

    let config = config::Config::load(config_path)
        .unwrap_or_else(|e| {
            eprintln!("error loading config: {e}");
            std::process::exit(1);
        });

    let conn = Connection::connect_to_env().expect("failed to connect to Wayland");
    let (globals, event_queue) = registry_queue_init::<bar::BarApp>(&conn)
        .expect("failed to init registry");
    let qh = event_queue.handle();

    let mut event_loop: EventLoop<bar::BarApp> =
        EventLoop::try_new().expect("failed to create event loop");
    let loop_handle = event_loop.handle();

    WaylandSource::new(conn, event_queue)
        .insert(loop_handle.clone())
        .expect("failed to insert Wayland source");

    ipc::register_listener(&loop_handle);

    let mut app = bar::BarApp::new(config, &globals, &qh, &loop_handle);

    while RUNNING.load(Ordering::Relaxed) {
        if let Err(e) = event_loop.dispatch(std::time::Duration::from_millis(16), &mut app) {
            log::error!("event loop dispatch error: {e}");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app.maybe_redraw();
    }

    log::info!("shutting down");
    let _ = std::fs::remove_file(ipc::sock_path());
}
