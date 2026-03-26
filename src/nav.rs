use std::os::unix::process::CommandExt;

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::bar::BarApp;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    Visual,
    Text,
}

#[derive(Debug)]
pub struct NavState {
    pub stack: Vec<String>,
    pub mode: DisplayMode,
    pub query: String,
    pub selected: usize,
}

impl NavState {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            mode: DisplayMode::Visual,
            query: String::new(),
            selected: 0,
        }
    }

    pub fn text() -> Self {
        Self { mode: DisplayMode::Text, ..Self::new() }
    }

    pub fn module(id: &str, mode: DisplayMode) -> Self {
        Self { stack: vec![id.to_string()], mode, ..Self::new() }
    }
}

impl BarApp {
    pub(crate) fn handle_key(&mut self, event: KeyEvent) {
        match self.nav.mode {
            DisplayMode::Visual => self.handle_visual_key(event),
            DisplayMode::Text => self.handle_text_key(event),
        }
    }

    fn handle_visual_key(&mut self, event: KeyEvent) {
        if event.keysym == Keysym::Escape {
            self.set_nav(NavState::new());
            return;
        }

        if event.keysym == Keysym::BackSpace && !self.nav.stack.is_empty() {
            self.set_nav(NavState::text());
            return;
        }

        // / enters text mode at current level
        if event.utf8.as_deref() == Some("/") {
            let stack = self.nav.stack.clone();
            let mut nav = NavState::text();
            nav.stack = stack;
            self.set_nav(nav);
            return;
        }

        // Deep module keys
        if let Some(mod_id) = self.nav.stack.first().cloned() {
            if let Some(deep) = self.interactive.get_mut(&mod_id) {
                let data = self.states.borrow()
                    .get(&mod_id)
                    .map(|s| s.data.clone())
                    .unwrap_or(serde_json::Value::Null);
                if deep.handle_key(&event, &data) {
                    self.dirty.set(true);
                    return;
                }
            }
        }

        // Key hint actions
        if let Some(module) = self.current_module() {
            let key_name = Self::event_key_name(&event);
            if let Some(key_name) = key_name {
                let matched = module.key_hints.iter()
                    .find(|h| h.key == key_name)
                    .map(|h| h.action.clone());
                if let Some(action) = matched {
                    if action == "back" {
                        self.set_nav(NavState::new());
                    } else {
                        Self::spawn_command(&action);
                        if let Some(mod_id) = self.nav.stack.first().cloned() {
                            self.source_mgr.nudge(&mod_id);
                            let icon = self.config.bar.modules.get(&mod_id)
                                .and_then(|m| m.icon.clone());
                            let name = self.config.bar.modules.get(&mod_id)
                                .map(|m| m.name.clone())
                                .unwrap_or_default();
                            self.set_toast(&name, icon, 3);
                        }
                    }
                    return;
                }
            }
        }
    }

    fn handle_text_key(&mut self, event: KeyEvent) {
        let ctrl = self.modifiers.ctrl;

        // Esc: dismiss
        if event.keysym == Keysym::Escape {
            if self.nav.stack.is_empty() {
                self.set_nav(NavState::new());
            } else if ctrl {
                self.set_nav(NavState::text());
            } else {
                self.set_nav(NavState::new());
            }
            return;
        }

        // Enter: select
        if event.keysym == Keysym::Return {
            self.handle_text_select(ctrl);
            return;
        }

        // Left/Right: cycle selection
        if event.keysym == Keysym::Right {
            let count = crate::view::text_match_count(&self.nav, &self.config, &self.states);
            if count > 0 {
                self.nav.selected = (self.nav.selected + 1) % count;
                self.dirty.set(true);
            }
            return;
        }

        if event.keysym == Keysym::Left {
            let count = crate::view::text_match_count(&self.nav, &self.config, &self.states);
            if count > 0 {
                self.nav.selected = self.nav.selected.checked_sub(1).unwrap_or(count - 1);
                self.dirty.set(true);
            }
            return;
        }

        if event.keysym == Keysym::BackSpace {
            self.nav.query.pop();
            self.nav.selected = 0;
            self.dirty.set(true);
            self.check_auto_enter();
            return;
        }

        if let Some(s) = &event.utf8 {
            if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                self.nav.query.push_str(s);
                self.nav.selected = 0;
                self.dirty.set(true);
                self.check_auto_enter();
            }
        }
    }

    fn handle_text_select(&mut self, stay_text: bool) {
        let items = crate::view::text_matched_items(&self.nav, &self.config, &self.states);
        let selected = self.nav.selected.min(items.len().saturating_sub(1));

        if let Some((_, item)) = items.get(selected) {
            match item {
                crate::view::TextItem::Module { id } => {
                    let has_view = self.config.bar.modules.get(id)
                        .map(|m| m.has_view()).unwrap_or(false);
                    let mode = if stay_text || !has_view { DisplayMode::Text } else { DisplayMode::Visual };
                    self.set_nav(NavState::module(id, mode));
                }
                crate::view::TextItem::App { exec, desktop_id } => {
                    self.set_nav(NavState::new());
                    Self::launch_app(exec, desktop_id.as_deref());
                }
            }
        }
    }

    fn check_auto_enter(&mut self) {
        if self.nav.query.len() >= 2 {
            let items = crate::view::text_matched_items(&self.nav, &self.config, &self.states);
            if items.len() == 1 {
                if let Some((_, item)) = items.first() {
                    match item {
                        crate::view::TextItem::Module { id } => {
                            log::info!("auto-enter: {}", id);
                            let has_view = self.config.bar.modules.get(id)
                                .map(|m| m.has_view()).unwrap_or(false);
                            let mode = if has_view { DisplayMode::Visual } else { DisplayMode::Text };
                            self.set_nav(NavState::module(id, mode));
                        }
                        crate::view::TextItem::App { exec, desktop_id } => {
                            Self::launch_app(exec, desktop_id.as_deref());
                            self.set_nav(NavState::new());
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn handle_click(&mut self, surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface, x: f64, _y: f64) {
        let Some(bar_id) = self.bar_id_for_surface(surface) else { return };
        let Some(bar) = self.bars.get(&bar_id) else { return };

        let px = x as f32 - self.renderer.pad_left;

        for hit in &bar.hit_areas {
            if px >= hit.start_x && px < hit.end_x {
                if hit.is_breadcrumb {
                    log::info!("breadcrumb click -> root");
                    self.set_nav(NavState::new());
                    return;
                }

                log::info!("click hit: {} (x={:.0})", hit.path, px);

                if hit.path == "launcher" {
                    self.set_nav(NavState::text());
                    return;
                }

                if let Some(module) = self.config.bar.modules.get(&hit.path) {
                    let mode = if module.has_view() { DisplayMode::Visual } else { DisplayMode::Text };
                    self.set_nav(NavState::module(&hit.path, mode));
                }
                return;
            }
        }
    }

    fn event_key_name(event: &KeyEvent) -> Option<String> {
        match event.keysym {
            Keysym::Up => Some("Up".into()),
            Keysym::Down => Some("Down".into()),
            Keysym::Left => Some("Left".into()),
            Keysym::Right => Some("Right".into()),
            Keysym::Return => Some("Enter".into()),
            Keysym::Escape => Some("Esc".into()),
            Keysym::BackSpace => Some("BackSpace".into()),
            Keysym::Tab => Some("Tab".into()),
            _ => event.utf8.clone(),
        }
    }

    pub(crate) fn launch_app(exec: &str, _desktop_id: Option<&str>) {
        // Delegate to compositor via `niri msg action spawn-sh`.
        // The compositor has the full user session environment (PATH,
        // LD_LIBRARY_PATH, etc.) and handles cgroup isolation itself.
        let args: Vec<&str> = exec.split_whitespace().collect();
        match std::process::Command::new("niri")
            .args(["msg", "action", "spawn", "--"])
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => log::debug!("launched app via niri: {exec}"),
            Err(e) => {
                log::warn!("niri spawn failed: {e}, falling back to direct spawn");
                Self::spawn_command(exec);
            }
        }
    }

    pub(crate) fn spawn_command(cmd: &str) {
        match unsafe {
            std::process::Command::new("sh")
                .args(["-c", cmd])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .pre_exec(|| { libc::setsid(); Ok(()) })
                .spawn()
        } {
            Ok(_) => log::debug!("spawned: {cmd}"),
            Err(e) => log::error!("failed to spawn '{cmd}': {e}"),
        }
    }
}
