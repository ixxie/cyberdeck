mod actions;
mod appicon;
mod bar;
mod cli;
mod color;
mod nav;
mod config;
mod layout;
mod icons;
mod ipc;
mod modlib;
mod mods;
mod notifications;
mod pipewire;
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

    let cli = cli::parse();

    if let Some(cmd) = cli.cmd {
        cli::run_cmd(cmd);
        return;
    }

    // Daemon mode
    unsafe {
        libc::signal(libc::SIGINT, handle_signal as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle_signal as libc::sighandler_t);
    }

    let config = config::Config::load(cli.config.as_deref())
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
