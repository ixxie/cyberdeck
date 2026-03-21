use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};

use serde::{Deserialize, Serialize};
use smithay_client_toolkit::reexports::calloop::generic::Generic;
use smithay_client_toolkit::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::{BarApp, DisplayMode, NavState};

#[derive(Serialize, Deserialize)]
#[serde(tag = "cmd")]
enum IpcRequest {
    #[serde(rename = "launcher")]
    Launcher,
    #[serde(rename = "dismiss")]
    Dismiss,
    #[serde(rename = "push")]
    Push { child: String },
    #[serde(rename = "pop")]
    Pop,
    #[serde(rename = "navigate")]
    Navigate { path: Vec<String> },
    #[serde(rename = "state")]
    State,
    #[serde(rename = "run")]
    Run { module: String, hint_key: String },
    #[serde(rename = "type")]
    Type { text: String },
    #[serde(rename = "key")]
    Key { key: String },
}

#[derive(Serialize, Deserialize)]
struct IpcResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected: Option<usize>,
}

impl IpcResponse {
    fn err(msg: &str) -> Self {
        Self {
            ok: false,
            error: Some(msg.to_string()),
            path: None,
            mode: None,
            query: None,
            selected: None,
        }
    }
}

pub fn sock_path() -> std::path::PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(runtime_dir).join("cyberdeck.sock")
}

pub fn register_listener(handle: &LoopHandle<'static, BarApp>) {
    let path = sock_path();
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)
        .expect("failed to bind IPC socket");

    // Restrict socket to owner only
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

    listener.set_nonblocking(true)
        .expect("failed to set socket non-blocking");

    let generic = Generic::new(listener, Interest::READ, Mode::Edge);
    handle.insert_source(generic, |_event, listener, app| {
        loop {
            match listener.accept() {
                Ok((stream, _)) => handle_client(stream, app),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    log::error!("IPC accept error: {e}");
                    break;
                }
            }
        }
        Ok(PostAction::Continue)
    }).expect("failed to register IPC listener");
}

fn handle_client(stream: UnixStream, app: &mut BarApp) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(100)));

    let mut reader = BufReader::new(&stream);
    let mut line = String::new();

    if reader.read_line(&mut line).is_err() {
        let _ = write_response(&stream, IpcResponse::err("read error"));
        return;
    }

    let req: IpcRequest = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let _ = write_response(&stream, IpcResponse::err(&format!("invalid request: {e}")));
            return;
        }
    };

    let resp = dispatch(req, app);
    let _ = write_response(&stream, resp);
}

fn write_response(mut stream: &UnixStream, resp: IpcResponse) -> std::io::Result<()> {
    let mut json = serde_json::to_string(&resp)?;
    json.push('\n');
    stream.write_all(json.as_bytes())?;
    stream.flush()
}

fn dispatch(req: IpcRequest, app: &mut BarApp) -> IpcResponse {
    match req {
        IpcRequest::Launcher => {
            let new_mode = match app.nav.mode {
                DisplayMode::Visual => DisplayMode::Text,
                DisplayMode::Text => DisplayMode::Visual,
            };
            let mut new_nav = NavState::new();
            new_nav.stack = app.nav.stack.clone();
            new_nav.mode = new_mode;
            if new_mode == DisplayMode::Text {
                new_nav.query = String::new();
                new_nav.selected = 0;
            }
            app.set_nav(new_nav);
            state_response(app)
        }
        IpcRequest::Dismiss => {
            app.set_nav(NavState::new());
            state_response(app)
        }
        IpcRequest::Push { child } => {
            if app.config.bar.modules.contains_key(&child) {
                let module = &app.config.bar.modules[&child];
                let mode = if module.has_view() { DisplayMode::Visual } else { DisplayMode::Text };
                app.set_nav(NavState::module(&child, mode));
                state_response(app)
            } else {
                IpcResponse::err(&format!("unknown module: {child}"))
            }
        }
        IpcRequest::Pop => {
            app.set_nav(NavState::new());
            state_response(app)
        }
        IpcRequest::Navigate { path } => {
            if path.is_empty() {
                app.set_nav(NavState::new());
                return state_response(app);
            }
            // Only single-depth navigation now
            let id = &path[0];
            if app.config.bar.modules.contains_key(id) {
                app.set_nav(NavState::module(id, DisplayMode::Visual));
                state_response(app)
            } else {
                IpcResponse::err(&format!("unknown module: {id}"))
            }
        }
        IpcRequest::State => state_response(app),
        IpcRequest::Run { module, hint_key } => {
            if let Some(m) = app.config.bar.modules.get(&module) {
                if let Some(hint) = m.key_hints.iter().find(|h| h.key == hint_key) {
                    if hint.action != "back" {
                        BarApp::spawn_command(&hint.action);
                        app.source_mgr.nudge(&module);
                        let icon = m.icon.clone();
                        let name = m.name.clone();
                        app.set_toast(&name, icon, 3);
                    }
                    app.dirty.set(true);
                    state_response(app)
                } else {
                    IpcResponse::err(&format!("unknown key-hint '{hint_key}' in module '{module}'"))
                }
            } else {
                IpcResponse::err(&format!("unknown module: {module}"))
            }
        }
        IpcRequest::Type { text } => {
            if !matches!(app.nav.mode, DisplayMode::Text) {
                return IpcResponse::err("not in text mode");
            }
            app.nav.query.push_str(&text);
            app.nav.selected = 0;
            app.dirty.set(true);
            state_response(app)
        }
        IpcRequest::Key { key } => {
            let event = build_key_event(&key);
            app.handle_key(event);
            state_response(app)
        }
    }
}

fn state_response(app: &BarApp) -> IpcResponse {
    let mode_str = match app.nav.mode {
        DisplayMode::Visual => "visual",
        DisplayMode::Text => "text",
    };

    let (query, selected) = if matches!(app.nav.mode, DisplayMode::Text) {
        (Some(app.nav.query.clone()), Some(app.nav.selected))
    } else {
        (None, None)
    };

    IpcResponse {
        ok: true,
        error: None,
        path: Some(app.nav.stack.clone()),
        mode: Some(mode_str.to_string()),
        query,
        selected,
    }
}

fn build_key_event(key: &str) -> KeyEvent {
    let (keysym, utf8) = match key {
        "Return" | "Enter" => (Keysym::Return, None),
        "Escape" => (Keysym::Escape, None),
        "BackSpace" => (Keysym::BackSpace, None),
        "Tab" => (Keysym::Tab, None),
        "Up" => (Keysym::Up, None),
        "Down" => (Keysym::Down, None),
        "Left" => (Keysym::Left, None),
        "Right" => (Keysym::Right, None),
        "Page_Up" => (Keysym::Page_Up, None),
        "Page_Down" => (Keysym::Page_Down, None),
        s => {
            let raw = s.chars().next().map(|c| c as u32).unwrap_or(0);
            (Keysym::new(raw), Some(s.to_string()))
        }
    };

    KeyEvent {
        time: 0,
        raw_code: 0,
        keysym,
        utf8,
    }
}

// Client mode

pub fn send_command(cmd: &str, args: &[String]) {
    let req = match cmd {
        "launcher" => IpcRequest::Launcher,
        "dismiss" => IpcRequest::Dismiss,
        "push" => {
            let child = args.first().unwrap_or_else(|| {
                eprintln!("usage: cyberdeck push <module>");
                std::process::exit(1);
            });
            IpcRequest::Push { child: child.clone() }
        }
        "pop" => IpcRequest::Pop,
        "navigate" => {
            if args.is_empty() {
                eprintln!("usage: cyberdeck navigate <module>");
                std::process::exit(1);
            }
            IpcRequest::Navigate { path: args.to_vec() }
        }
        "state" => IpcRequest::State,
        "run" => {
            if args.len() < 2 {
                eprintln!("usage: cyberdeck run <module> <key>");
                std::process::exit(1);
            }
            IpcRequest::Run { module: args[0].clone(), hint_key: args[1].clone() }
        }
        "type" => {
            let text = args.first().unwrap_or_else(|| {
                eprintln!("usage: cyberdeck type <text>");
                std::process::exit(1);
            });
            IpcRequest::Type { text: text.clone() }
        }
        "key" => {
            let key = args.first().unwrap_or_else(|| {
                eprintln!("usage: cyberdeck key <keyname>");
                std::process::exit(1);
            });
            IpcRequest::Key { key: key.clone() }
        }
        _ => {
            eprintln!("unknown command: {cmd}");
            std::process::exit(1);
        }
    };

    let path = sock_path();
    let mut stream = UnixStream::connect(&path).unwrap_or_else(|e| {
        eprintln!("failed to connect to cyberdeck: {e}");
        std::process::exit(1);
    });

    let mut json = serde_json::to_string(&req).unwrap();
    json.push('\n');
    stream.write_all(json.as_bytes()).unwrap_or_else(|e| {
        eprintln!("failed to send command: {e}");
        std::process::exit(1);
    });
    let _ = stream.flush();

    let mut reader = BufReader::new(&stream);
    let mut response = String::new();
    reader.read_line(&mut response).unwrap_or_else(|e| {
        eprintln!("failed to read response: {e}");
        std::process::exit(1);
    });

    print!("{response}");

    let resp: serde_json::Value = serde_json::from_str(response.trim()).unwrap_or_default();
    if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
}
