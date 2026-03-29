use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};

use serde::{Deserialize, Serialize};
use smithay_client_toolkit::reexports::calloop::generic::Generic;
use smithay_client_toolkit::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::{BarApp, DisplayMode, NavState};

#[derive(Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum IpcRequest {
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
    #[serde(rename = "type")]
    Type { text: String },
    #[serde(rename = "key")]
    Key { key: String },
    #[serde(rename = "style")]
    SetStyle { style: String },
    #[serde(rename = "action")]
    Action { module: String, action: String, #[serde(default)] args: Vec<String> },
    #[serde(rename = "event")]
    Event { module: String, toast: String },
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
        IpcRequest::Action { module, action, args } => {
            let Some(m) = app.config.bar.modules.get(&module) else {
                return IpcResponse::err(&format!("unknown module: {module}"));
            };
            let icon = m.icon.clone();
            let name = m.name.clone();

            // Try deep module exec_action first
            if let Some(deep) = app.interactive.get_mut(&module) {
                let data = app.states.borrow()
                    .get(&module)
                    .map(|s| s.data.clone())
                    .unwrap_or(serde_json::Value::Null);
                if let Some(toast) = deep.exec_action(&action, &args, &data) {
                    app.source_mgr.nudge(&module);
                    app.set_toast(&toast, icon, 3);
                    app.dirty.set(true);
                    return state_response(app);
                }
            }

            // Fall back to action's run field
            if let Some(act) = m.action_by_name(&action) {
                if act.run != "native" {
                    BarApp::spawn_command(&act.run);
                    app.source_mgr.nudge(&module);
                    app.set_toast(&name, icon, 3);
                    app.dirty.set(true);
                    return state_response(app);
                }
            }

            IpcResponse::err(&format!("unknown action '{action}' in module '{module}'"))
        }
        IpcRequest::Event { module, toast } => {
            app.source_mgr.nudge(&module);
            // Only show toast if module has no spotlight hook
            let has_spotlight = app.config.bar.modules.get(&module)
                .map(|m| m.hooks.iter().any(|h| h.action == "spotlight"))
                .unwrap_or(false);
            if !has_spotlight {
                let icon = app.config.bar.modules.get(&module)
                    .and_then(|m| m.icon.clone());
                app.set_toast(&toast, icon, 3);
            }
            app.dirty.set(true);
            state_response(app)
        }
        IpcRequest::SetStyle { style } => {
            match style.as_str() {
                "classic" => app.set_layout(crate::config::Layout::Classic),
                "floating" => app.set_layout(crate::config::Layout::Floating),
                "pills" => app.set_layout(crate::config::Layout::Pills),
                "transparent" => app.set_layout(crate::config::Layout::Transparent),
                _ => return IpcResponse::err(&format!("unknown style: {style}. use: classic, floating, pills, transparent")),
            };
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

/// Fire-and-forget notification to the bar. Silently drops if bar not running.
pub fn notify_bar(module: &str, toast: &str) {
    if toast.is_empty() { return; }
    let path = sock_path();
    let Ok(mut stream) = UnixStream::connect(&path) else { return };
    let req = IpcRequest::Event {
        module: module.to_string(),
        toast: toast.to_string(),
    };
    let mut json = serde_json::to_string(&req).unwrap_or_default();
    json.push('\n');
    let _ = stream.write_all(json.as_bytes());
}

pub fn send_request(req: &IpcRequest) {
    let path = sock_path();
    let mut stream = UnixStream::connect(&path).unwrap_or_else(|e| {
        eprintln!("failed to connect to cyberdeck: {e}");
        std::process::exit(1);
    });

    let mut json = serde_json::to_string(req).unwrap();
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
    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        std::process::exit(1);
    }
}
