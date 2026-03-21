use std::collections::HashMap;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::generic::Generic;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{Interest, LoopHandle, Mode, PostAction, RegistrationToken};

use crate::config::{BarDef, SourceDef};

pub struct ModuleState {
    pub data: serde_json::Value,
    pub prev_data: serde_json::Value,
    pub dirty: bool,
    pub initialized: bool,
}

impl ModuleState {
    pub fn new() -> Self {
        Self {
            data: serde_json::Value::Object(serde_json::Map::new()),
            prev_data: serde_json::Value::Object(serde_json::Map::new()),
            dirty: false,
            initialized: false,
        }
    }
}

pub enum SourceHandle {
    Poll {
        _token: RegistrationToken,
    },
    Subscribe {
        child: Child,
        _token: RegistrationToken,
    },
    File {
        _token: RegistrationToken,
    },
    Native {
        _token: RegistrationToken,
    },
}

impl Drop for SourceManager {
    fn drop(&mut self) {
        for (id, handle) in self.sources.iter_mut() {
            if let SourceHandle::Subscribe { child, .. } = handle {
                log::info!("killing subscribe child for {id}");
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

pub struct SourceManager {
    pub sources: HashMap<String, SourceHandle>,
    pub nudges: HashMap<String, Arc<AtomicBool>>,
}

pub type SharedState = std::rc::Rc<std::cell::RefCell<HashMap<String, ModuleState>>>;

impl SourceManager {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            nudges: HashMap::new(),
        }
    }

    // Register all modules from the flat bar definition
    pub fn register_modules<D: 'static>(
        &mut self,
        bar: &BarDef,
        handle: &LoopHandle<'static, D>,
        dirty_flag: std::rc::Rc<std::cell::Cell<bool>>,
        states: SharedState,
    ) {
        for (id, module) in &bar.modules {
            if let Some(source) = &module.source {
                self.register_source(id, source, &module.params, handle, dirty_flag.clone(), states.clone());
            }
        }

        // Launcher is always registered (built-in)
        if !bar.modules.contains_key("launcher") {
            let empty = std::collections::HashMap::new();
            let source = SourceDef::Native { kind: "launcher".to_string(), interval: 60 };
            self.register_source("__launcher", &source, &empty, handle, dirty_flag, states);
        }
    }

    pub fn nudge(&self, mod_id: &str) {
        if let Some(flag) = self.nudges.get(mod_id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn register_source<D: 'static>(
        &mut self,
        path: &str,
        source: &SourceDef,
        params: &std::collections::HashMap<String, serde_json::Value>,
        handle: &LoopHandle<'static, D>,
        dirty_flag: std::rc::Rc<std::cell::Cell<bool>>,
        states: SharedState,
    ) {
        log::info!("registering source: {path} ({:?})", match source {
            SourceDef::Poll { interval, .. } => format!("poll interval={interval}s"),
            SourceDef::Subscribe { .. } => "subscribe".into(),
            SourceDef::File { paths, .. } => format!("file paths={}", paths.len()),
            SourceDef::Native { kind, interval } => format!("native kind={kind} interval={interval}s"),
        });
        states.borrow_mut().insert(path.to_string(), ModuleState::new());

        match source {
            SourceDef::Poll { command, interval } => {
                self.register_poll(path, command, *interval, handle, dirty_flag, states);
            }
            SourceDef::Subscribe { command } => {
                self.register_subscribe(path, command, handle, dirty_flag, states);
            }
            SourceDef::File { paths, interval } => {
                self.register_file(path, paths, *interval, handle, dirty_flag, states);
            }
            SourceDef::Native { kind, interval } => {
                if let Some((token, nudge)) = crate::mods::register(kind, *interval, path, handle, dirty_flag, states, params) {
                    self.sources.insert(path.to_string(), SourceHandle::Native { _token: token });
                    self.nudges.insert(path.to_string(), nudge);
                }
            }
        }
    }

    fn register_poll<D: 'static>(
        &mut self,
        id: &str,
        command: &[String],
        interval: u64,
        handle: &LoopHandle<'static, D>,
        dirty_flag: std::rc::Rc<std::cell::Cell<bool>>,
        states: SharedState,
    ) {
        let cmd: Vec<String> = command.to_vec();
        let mod_id = id.to_string();

        // Seed initial state synchronously
        if let Some(output) = run_command(&cmd) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&output) {
                let mut st = states.borrow_mut();
                if let Some(ms) = st.get_mut(&mod_id) {
                    ms.data = val;
                    ms.dirty = true;
                }
                dirty_flag.set(true);
            }
        }

        let poll_id = id.to_string();
        let timer = Timer::from_duration(Duration::from_secs(interval));
        let token = handle.insert_source(timer, move |_event, _metadata, _data| {
            log::debug!("poll timer fired for {poll_id}");
            match run_command(&cmd) {
                Some(output) => {
                    log::debug!("poll {poll_id} output: {}", output.trim());
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&output) {
                        let mut st = states.borrow_mut();
                        if let Some(ms) = st.get_mut(&mod_id) {
                            ms.data = val;
                            ms.dirty = true;
                        }
                        dirty_flag.set(true);
                    }
                }
                None => {
                    log::warn!("poll {poll_id} command failed: {:?}", cmd);
                }
            }
            TimeoutAction::ToDuration(Duration::from_secs(interval))
        }).expect("failed to register poll timer");

        self.sources.insert(id.to_string(), SourceHandle::Poll { _token: token });
    }

    fn register_subscribe<D: 'static>(
        &mut self,
        id: &str,
        command: &[String],
        handle: &LoopHandle<'static, D>,
        dirty_flag: std::rc::Rc<std::cell::Cell<bool>>,
        states: SharedState,
    ) {
        if command.is_empty() {
            log::error!("subscribe source for {id} has empty command");
            return;
        }

        let mut child = match Command::new(&command[0])
            .args(&command[1..])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                log::error!("failed to spawn subscribe source for {id}: {e}");
                return;
            }
        };

        let stdout = child.stdout.take().unwrap();
        let raw_fd = stdout.as_raw_fd();

        if let Err(e) = rustix::fs::fcntl_setfl(
            unsafe { std::os::fd::BorrowedFd::borrow_raw(raw_fd) },
            rustix::fs::OFlags::NONBLOCK,
        ) {
            log::error!("failed to set non-blocking for {id}: {e}");
            return;
        }

        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        std::mem::forget(stdout);

        let generic = Generic::new(fd, Interest::READ, Mode::Edge);
        let mod_id = id.to_string();
        let line_buf: std::rc::Rc<std::cell::RefCell<Vec<u8>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let lb = line_buf.clone();

        let token = handle.insert_source(generic, move |_event, fd, _data| {
            let mut tmp = [0u8; 4096];
            let mut buf = lb.borrow_mut();
            loop {
                match rustix::io::read(&mut *fd, &mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let line: Vec<u8> = buf.drain(..=pos).collect();
                            if let Ok(s) = std::str::from_utf8(&line) {
                                let s = s.trim();
                                if !s.is_empty() {
                                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
                                        let mut st = states.borrow_mut();
                                        if let Some(ms) = st.get_mut(&mod_id) {
                                            ms.data = val;
                                            ms.dirty = true;
                                        }
                                        dirty_flag.set(true);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) if e == rustix::io::Errno::AGAIN || e == rustix::io::Errno::WOULDBLOCK => break,
                    Err(e) => {
                        log::error!("read error for subscribe source: {e}");
                        break;
                    }
                }
            }
            Ok(PostAction::Continue)
        }).expect("failed to register subscribe source");

        self.sources.insert(id.to_string(), SourceHandle::Subscribe {
            child,
            _token: token,
        });
    }

    fn register_file<D: 'static>(
        &mut self,
        id: &str,
        paths: &[String],
        interval: u64,
        handle: &LoopHandle<'static, D>,
        dirty_flag: std::rc::Rc<std::cell::Cell<bool>>,
        states: SharedState,
    ) {
        let file_paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
        let mod_id = id.to_string();
        let file_id = id.to_string();

        // Seed initial state synchronously
        {
            let mut obj = serde_json::Map::new();
            for path in &file_paths {
                let key = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        obj.insert(key, val);
                    }
                }
            }
            if !obj.is_empty() {
                let mut st = states.borrow_mut();
                if let Some(ms) = st.get_mut(&mod_id) {
                    ms.data = serde_json::Value::Object(obj);
                    ms.dirty = true;
                }
                dirty_flag.set(true);
            }
        }

        let timer = Timer::from_duration(Duration::from_secs(interval));
        let token = handle.insert_source(timer, move |_event, _metadata, _data| {
            log::debug!("file timer fired for {file_id}");
            let mut obj = serde_json::Map::new();

            for path in &file_paths {
                let key = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        match serde_json::from_str::<serde_json::Value>(&content) {
                            Ok(val) => { obj.insert(key, val); }
                            Err(e) => {
                                log::warn!("file {}: invalid JSON: {e}", path.display());
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("file {}: read error: {e}", path.display());
                    }
                }
            }

            let val = serde_json::Value::Object(obj);
            let mut st = states.borrow_mut();
            if let Some(ms) = st.get_mut(&mod_id) {
                ms.data = val;
                ms.dirty = true;
            }
            dirty_flag.set(true);
            TimeoutAction::ToDuration(Duration::from_secs(interval))
        }).expect("failed to register file timer");

        self.sources.insert(id.to_string(), SourceHandle::File { _token: token });
    }
}

fn run_command(cmd: &[String]) -> Option<String> {
    if cmd.is_empty() {
        return None;
    }
    let output = Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}
