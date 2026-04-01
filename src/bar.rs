use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard;
use smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::delegate_compositor;
use smithay_client_toolkit::delegate_keyboard;
use smithay_client_toolkit::delegate_pointer;
use smithay_client_toolkit::delegate_layer;
use smithay_client_toolkit::delegate_output;
use smithay_client_toolkit::delegate_registry;
use smithay_client_toolkit::delegate_seat;
use smithay_client_toolkit::delegate_shm;
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::registry_handlers;
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::seat::keyboard::{
    KeyboardHandler, KeyEvent, Keysym, Modifiers,
};
use smithay_client_toolkit::seat::pointer::{
    PointerHandler, PointerEvent, PointerEventKind,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};

use tiny_skia::Pixmap;


use crate::color::Rgba;
use crate::config::{Config, ModuleDef, Position};
use crate::layout::{BarContent, Frame, Metrics, lay};
use crate::mods::InteractiveModule;
use crate::render::Renderer;
use crate::icons::IconSet;
use crate::source::{ModuleState, SourceManager};
use crate::template::TemplateEngine;

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) selected: Rgba,
    pub(crate) active: Rgba,
    pub(crate) idle: Rgba,
}

pub use crate::nav::{DisplayMode, NavState};

pub struct BarInstance {
    pub layer_surface: LayerSurface,
    pub pool: SlotPool,
    pub icons: IconSet,
    pub width: u32,
    pub scale: i32,
    pub configured: bool,
    pub output_name: Option<String>,
    pub output: WlOutput,
    pub frame: Option<Frame>,
}

impl BarInstance {
    fn new(
        output: &WlOutput,
        output_name: Option<String>,
        compositor: &CompositorState,
        layer_shell: &LayerShell,
        shm: &Shm,
        config: &Config,
        renderer: &Renderer,
        qh: &QueueHandle<BarApp>,
        icon_map: &HashMap<String, String>,
    ) -> Self {
        let bar_h = renderer.bar_height(&config.settings);
        let surface = compositor.create_surface(qh);
        let layer_surface = layer_shell.create_layer_surface(
            qh, surface, Layer::Top, Some("cyberdeck"), Some(output),
        );

        let anchor = match config.settings.position {
            Position::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            Position::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        };
        layer_surface.set_anchor(anchor);
        let m = config.settings.margin() as i32;
        layer_surface.set_margin(m, m, m, m);

        layer_surface.set_size(0, bar_h);
        let exclusive = bar_h as i32;
        layer_surface.set_exclusive_zone(exclusive);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer_surface.commit();

        let pool = SlotPool::new(256 * bar_h as usize * 4, shm)
            .expect("failed to create slot pool");

        let initial_scale = 2i32;
        crate::appicon::set_target_height((renderer.cell_h * initial_scale as f32).ceil() as u32);
        let icons = IconSet::load(
            config.settings.icons_dir.as_deref(),
            &config.settings.icon_weight,
            renderer.cell_h * initial_scale as f32,
            icon_map,
        );
        Self {
            layer_surface,
            pool,
            icons,
            width: 0,
            scale: initial_scale,
            configured: false,
            output_name,
            output: output.clone(),
            frame: None,
        }
    }
}

pub struct BarApp {
    pub registry: RegistryState,
    pub seat: SeatState,
    pub output: OutputState,
    pub compositor: CompositorState,
    pub shm: Shm,
    pub layer_shell: LayerShell,

    pub config: Config,
    pub renderer: Renderer,
    pub template_engine: TemplateEngine,
    pub source_mgr: SourceManager,

    pub states: Rc<std::cell::RefCell<HashMap<String, ModuleState>>>,
    pub dirty: Rc<Cell<bool>>,

    pub qh: QueueHandle<BarApp>,
    pub bars: HashMap<u32, BarInstance>,
    pub loop_handle: LoopHandle<'static, BarApp>,
    pub keyboard: Option<WlKeyboard>,
    pub pointer: Option<WlPointer>,
    pub nav: NavState,
    pub root_scroll: usize,
    pub modifiers: Modifiers,
    pub toasts: Vec<Toast>,
    pub badge_overrides: HashMap<String, RegistrationToken>,
    pub spotlight_toast_id: Option<u64>,
    pub interactive: HashMap<String, Box<dyn InteractiveModule>>,
    pub nav_changed: Instant,
    pub icon_map: HashMap<String, String>,
    prev_focused_ws: Option<i64>,
    prev_focused_win: Option<(i64, i64)>,
    nav_toast_id: Option<u64>,
    pub location_changed: Instant,
}

pub struct Toast {
    pub toast_id: u64,
    pub text: String,
    pub icon: Option<String>,
    pub icon_pixmap: Option<std::sync::Arc<tiny_skia::Pixmap>>,
    pub elems: Vec<crate::layout::Elem>,
    pub token: RegistrationToken,
    pub created: Instant,
    pub lifetime: Duration,
    /// When set, toast timer is paused with this much time remaining.
    pub paused_remaining: Option<Duration>,
}

const MAX_VISIBLE_TOASTS: usize = 3;
static NEXT_TOAST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl BarApp {
    pub fn new(
        config: Config,
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        loop_handle: &LoopHandle<'static, Self>,
    ) -> Self {
        let compositor = CompositorState::bind(globals, qh).expect("wl_compositor not available");
        let shm = Shm::bind(globals, qh).expect("wl_shm not available");
        let layer_shell = LayerShell::bind(globals, qh).expect("layer_shell not available");
        let seat = SeatState::new(globals, qh);
        let output = OutputState::new(globals, qh);
        let registry = RegistryState::new(globals);

        let renderer = Renderer::new(&config.settings.font, config.settings.font_size, &config.settings);
        let icon_map = crate::icons::discover(config.settings.icons_dir.as_deref());
        let template_engine = TemplateEngine::new(&config.bar, &icon_map);

        let states: Rc<std::cell::RefCell<HashMap<String, ModuleState>>> =
            Rc::new(std::cell::RefCell::new(HashMap::new()));
        let dirty = Rc::new(Cell::new(true));

        let mut source_mgr = SourceManager::new();
        source_mgr.register_modules(&config.bar, loop_handle, dirty.clone(), states.clone());

        // Spawn notification daemon and register calloop channel
        let notif_channel = crate::notifications::spawn_daemon();
        let notif_dirty = dirty.clone();
        loop_handle.insert_source(notif_channel, move |event, _, app| {
            use smithay_client_toolkit::reexports::calloop::channel::Event;
            if let Event::Msg(evt) = event {
                match evt {
                    crate::notifications::NotifyEvent::New(n) => {
                        let muted = crate::notifications::STORE.lock().unwrap().is_muted(&n.app_name);
                        if !muted {
                            let timeout_secs = if n.timeout_ms <= 0 { 5 } else {
                                (n.timeout_ms as u64 + 999) / 1000
                            };
                            let text = if n.body.is_empty() {
                                n.summary.clone()
                            } else {
                                format!("{} — {}", n.summary, n.body)
                            };
                            app.add_toast_with_pixmap(&text, Some(n.app_name.clone()), n.icon_pixmap.clone(), timeout_secs);
                        }
                    }
                    crate::notifications::NotifyEvent::Close(_id) => {
                        // Notification dismissed via D-Bus; store already updated
                    }
                }
                notif_dirty.set(true);
            }
        }).expect("failed to register notification channel");

        // Initialize deep modules before config is moved
        let mut interactive: HashMap<String, Box<dyn InteractiveModule>> = HashMap::new();
        let icon_resolver = |name: &str| template_engine.render_icon(name);
        for (id, module) in &config.bar.modules {
            if let Some(mt) = &module.module_type {
                if let Some(deep) = crate::mods::create_interactive(mt, module, &icon_resolver) {
                    interactive.insert(id.clone(), deep);
                }
            }
        }

        Self {
            registry,
            seat,
            output,
            compositor,
            shm,
            layer_shell,
            config,
            renderer,
            template_engine,
            source_mgr,
            states,
            dirty,
            qh: qh.clone(),
            bars: HashMap::new(),
            loop_handle: loop_handle.clone(),
            keyboard: None,
            pointer: None,
            nav: NavState::new(),
            root_scroll: 0,
            modifiers: Modifiers {
                ctrl: false,
                alt: false,
                shift: false,
                caps_lock: false,
                logo: false,
                num_lock: false,
            },
            toasts: Vec::new(),
            badge_overrides: HashMap::new(),
            spotlight_toast_id: None,
            interactive,
            nav_changed: Instant::now(),
            icon_map,
            prev_focused_ws: None,
            prev_focused_win: None,
            nav_toast_id: None,
            location_changed: Instant::now(),
        }
    }

    pub fn current_module(&self) -> Option<&ModuleDef> {
        if self.nav.stack.is_empty() {
            return None;
        }
        self.config.bar.modules.get(&self.nav.stack[0])
    }

    pub fn set_nav(&mut self, nav: NavState) {
        let needs_kb = !(nav.stack.is_empty() && matches!(nav.mode, DisplayMode::Visual));
        log::info!("nav -> stack={:?} mode={:?} kb={}", nav.stack, nav.mode, needs_kb);
        if let Some(mod_id) = nav.stack.first() {
            if let Some(deep) = self.interactive.get_mut(mod_id) {
                let data = self.states.borrow()
                    .get(mod_id)
                    .map(|s| s.data.clone())
                    .unwrap_or(serde_json::Value::Null);
                deep.activate(&data);
            }
        }
        self.nav = nav;
        self.nav_changed = Instant::now();
        for bar in self.bars.values() {
            let interactivity = if needs_kb {
                KeyboardInteractivity::Exclusive
            } else {
                KeyboardInteractivity::None
            };
            bar.layer_surface.set_keyboard_interactivity(interactivity);
            bar.layer_surface.wl_surface().commit();
        }
        self.dirty.set(true);
    }

    pub fn set_layout(&mut self, layout: crate::config::Layout) {
        self.config.settings.layout = layout;
        self.apply_style();
    }

    fn apply_style(&mut self) {
        self.renderer = Renderer::new(
            &self.config.settings.font,
            self.config.settings.font_size,
            &self.config.settings,
        );
        let bar_h = self.renderer.bar_height(&self.config.settings);
        let exclusive = bar_h as i32;
        let m = self.config.settings.margin() as i32;
        for bar in self.bars.values() {
            bar.layer_surface.set_size(0, bar_h);
            bar.layer_surface.set_exclusive_zone(exclusive);
            bar.layer_surface.set_margin(m, m, m, m);
            bar.layer_surface.wl_surface().commit();
        }
        self.dirty.set(true);
        log::info!("style: layout={:?}", self.config.settings.layout);
    }

    pub fn maybe_redraw(&mut self) {
        let animating = self.has_active_animations();
        if animating {
            self.dirty.set(true);
        }
        if !self.dirty.get() {
            return;
        }
        self.check_ws_changes();
        self.process_hooks();
        let qh = self.qh.clone();
        let nav_age = self.nav_changed.elapsed();
        let location_age = self.location_changed.elapsed();
        let ids: Vec<u32> = self.bars.keys().copied().collect();
        let mut drew = false;
        for id in ids {
            if let Some(bar) = self.bars.get_mut(&id) {
                if bar.configured && bar.width > 0 {
                    Self::draw_bar(
                        bar, &self.config, &mut self.renderer,
                        &self.template_engine, &self.states, &qh,
                        &mut self.nav, &mut self.root_scroll,
                        &self.toasts, &self.badge_overrides,
                        &self.interactive,
                        nav_age, location_age,
                    );
                    drew = true;
                }
            }
        }
        if drew {
            self.dirty.set(false);
        }
    }

    fn has_active_animations(&self) -> bool {
        let fade_dur = Duration::from_millis(300);

        // Nav transition
        if self.nav_changed.elapsed() < fade_dur {
            return true;
        }

        // Location indicator dim (bright for 2s, then fade over 1s)
        if self.location_changed.elapsed() < Duration::from_secs(3) {
            return true;
        }

        // Toast fade-in (skip paused toasts)
        for t in &self.toasts {
            if t.paused_remaining.is_some() { continue; }
            if t.created.elapsed() < fade_dur {
                return true;
            }
            // Toast fade-out (last 500ms of lifetime)
            let remaining = t.lifetime.saturating_sub(t.created.elapsed());
            if remaining < Duration::from_millis(500) {
                return true;
            }
        }

        false
    }

    fn process_hooks(&mut self) {
        let mut actions: Vec<(String, String, u64)> = Vec::new();
        let mut processed: Vec<String> = Vec::new();

        {
            let states = self.states.borrow();
            for (child_id, child) in &self.config.bar.modules {
                let Some(ms) = states.get(child_id) else { continue };
                if !ms.dirty { continue; }

                if ms.initialized {
                    self.template_engine.set_event_context(&ms.data, &ms.prev_data);
                    for (i, hook) in child.hooks.iter().enumerate() {
                        let is_true = self.template_engine.eval_hook_condition(child_id, i, &ms.data);
                        if is_true {
                            log::debug!("hook fired: {child_id} action={}", hook.action);
                            actions.push((child_id.clone(), hook.action.clone(), hook.timeout));
                        }
                    }
                } else {
                    log::debug!("hooks skipped (not initialized): {child_id}");
                }
                if child_id == "outputs" {
                    log::debug!("outputs dirty={} initialized={} data_vol={:?} prev_vol={:?}",
                        ms.dirty, ms.initialized,
                        ms.data.get("volume"),
                        ms.prev_data.get("volume"));
                }
                processed.push(child_id.clone());
            }
        }

        {
            let mut states = self.states.borrow_mut();
            for id in &processed {
                if let Some(ms) = states.get_mut(id) {
                    ms.prev_data = ms.data.clone();
                    ms.dirty = false;
                    ms.initialized = true;
                }
            }
        }

        for (mod_path, action, timeout) in actions {
            if let Some(badge_name) = action.strip_prefix("show-badge:") {
                let key = format!("{mod_path}.{badge_name}");
                self.set_badge_override(&key, timeout);
            } else {
                match action.as_str() {
                    "spotlight" => {
                        self.set_spotlight(&mod_path, timeout);
                    }
                    "toast" => {
                        let icon = self.config.bar.modules.get(&mod_path)
                            .and_then(|m| m.icon.clone());
                        let text = mod_path.clone();
                        self.set_toast(&text, icon, timeout);
                    }
                    cmd => {
                        Self::spawn_command(cmd);
                    }
                }
            }
        }
    }

    /// Detect workspace/window focus changes and update location_changed timestamp.
    fn check_ws_changes(&mut self) {
        let states = self.states.borrow();
        let Some(ws_state) = states.get("workspaces") else { return };
        if !ws_state.dirty { return; }
        let data = &ws_state.data;

        let workspaces = data.get("workspaces").and_then(|v| v.as_array());
        let windows = data.get("windows").and_then(|v| v.as_array());

        let cur_ws = workspaces.and_then(|wss| {
            wss.iter().find(|ws| ws.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                .and_then(|ws| ws.get("id").and_then(|v| v.as_i64()))
        });

        let cur_win = windows.and_then(|wins| {
            wins.iter().find(|w| w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                .map(|w| {
                    let col = w.get("col").and_then(|v| v.as_i64()).unwrap_or(0);
                    let row = w.get("row").and_then(|v| v.as_i64()).unwrap_or(0);
                    (col, row)
                })
        });

        let ws_changed = cur_ws.is_some() && self.prev_focused_ws.is_some()
            && cur_ws != self.prev_focused_ws;
        let win_changed = cur_win.is_some() && self.prev_focused_win.is_some()
            && cur_win != self.prev_focused_win;

        if ws_changed || win_changed {
            self.location_changed = Instant::now();
            log::info!("location changed: ws={ws_changed} win={win_changed}");
        }

        self.prev_focused_ws = cur_ws;
        self.prev_focused_win = cur_win;
    }

    fn set_nav_toast(&mut self, elems: Vec<crate::layout::Elem>) -> u64 {
        let tid = NEXT_TOAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nav_tid = tid;
        let token = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_secs(2)),
            move |_, _, app| {
                app.remove_toast(nav_tid);
                let was_nav = app.nav_toast_id == Some(nav_tid);
                let was_spotlight = app.spotlight_toast_id == Some(nav_tid);
                if was_nav { app.nav_toast_id = None; }
                if was_spotlight { app.spotlight_toast_id = None; }
                // Unpause if no other priority toast remains
                if (was_nav || was_spotlight)
                    && app.spotlight_toast_id.is_none()
                    && app.nav_toast_id.is_none()
                {
                    app.unpause_regular_toasts();
                }
                app.dirty.set(true);
                TimeoutAction::Drop
            },
        ).expect("failed to set nav toast timer");

        self.toasts.push(Toast {
            toast_id: tid,
            text: String::new(),
            icon: None,
            icon_pixmap: None,
            elems,
            token,
            created: Instant::now(),
            lifetime: Duration::from_secs(2),
            paused_remaining: None,
        });
        self.dirty.set(true);
        tid
    }

    pub fn set_toast(&mut self, text: &str, icon: Option<String>, timeout: u64) {
        self.add_toast_with_pixmap(text, icon, None, timeout);
    }

    fn add_toast_with_pixmap(
        &mut self,
        text: &str,
        icon: Option<String>,
        icon_pixmap: Option<std::sync::Arc<tiny_skia::Pixmap>>,
        timeout: u64,
    ) {
        // Cap visible toasts
        while self.toasts.len() >= MAX_VISIBLE_TOASTS {
            let old = self.toasts.remove(0);
            self.loop_handle.remove(old.token);
        }

        let tid = NEXT_TOAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let token = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_secs(timeout)),
            move |_, _, app| {
                app.remove_toast(tid);
                app.dirty.set(true);
                TimeoutAction::Drop
            },
        ).expect("failed to set toast timer");

        self.toasts.push(Toast {
            toast_id: tid,
            text: text.to_string(),
            icon,
            icon_pixmap,
            elems: Vec::new(),
            token,
            created: Instant::now(),
            lifetime: Duration::from_secs(timeout),
            paused_remaining: None,
        });

        // Pause immediately if a spotlight is active
        if self.spotlight_toast_id.is_some() {
            self.pause_regular_toasts();
        }
        self.dirty.set(true);
    }

    fn remove_toast(&mut self, tid: u64) {
        if let Some(pos) = self.toasts.iter().position(|t| t.toast_id == tid) {
            let old = self.toasts.remove(pos);
            // Token may already be removed if toast was paused; remove is a no-op then
            self.loop_handle.remove(old.token);
        }
    }

    /// Pause all regular toasts by removing their calloop timers
    /// and recording remaining lifetime. Skips priority toasts (spotlight, nav)
    /// and already-paused toasts. Idempotent.
    fn pause_regular_toasts(&mut self) {
        let spotlight_id = self.spotlight_toast_id;
        let nav_id = self.nav_toast_id;
        let mut to_pause: Vec<(u64, RegistrationToken, Duration)> = Vec::new();
        for t in &self.toasts {
            if Some(t.toast_id) == spotlight_id { continue; }
            if Some(t.toast_id) == nav_id { continue; }
            if t.paused_remaining.is_some() { continue; }
            let remaining = t.lifetime.saturating_sub(t.created.elapsed());
            if remaining.is_zero() { continue; }
            to_pause.push((t.toast_id, t.token, remaining));
        }
        for (tid, token, remaining) in to_pause {
            self.loop_handle.remove(token);
            if let Some(t) = self.toasts.iter_mut().find(|t| t.toast_id == tid) {
                t.paused_remaining = Some(remaining);
            }
        }
    }

    /// Resume paused toasts by re-registering calloop timers with their
    /// remaining lifetime.
    fn unpause_regular_toasts(&mut self) {
        let mut to_unpause: Vec<(u64, Duration)> = Vec::new();
        for t in &self.toasts {
            if let Some(remaining) = t.paused_remaining {
                to_unpause.push((t.toast_id, remaining));
            }
        }
        for (tid, remaining) in to_unpause {
            let token = self.loop_handle.insert_source(
                Timer::from_duration(remaining),
                move |_, _, app| {
                    app.remove_toast(tid);
                    if app.nav_toast_id == Some(tid) {
                        app.nav_toast_id = None;
                    }
                    app.dirty.set(true);
                    TimeoutAction::Drop
                },
            ).expect("failed to re-register toast timer");
            if let Some(t) = self.toasts.iter_mut().find(|t| t.toast_id == tid) {
                t.created = Instant::now();
                t.lifetime = remaining;
                t.token = token;
                t.paused_remaining = None;
            }
        }
    }

    fn set_spotlight(&mut self, mod_id: &str, timeout: u64) {
        let module = match self.config.bar.modules.get(mod_id) {
            Some(m) => m,
            None => return,
        };

        let data = self.states.borrow()
            .get(mod_id)
            .map(|s| s.data.clone())
            .unwrap_or(serde_json::Value::Null);

        // Render widget as toast elems
        let mut elems = Vec::new();
        if let Some(icon_name) = &module.icon {
            let icon_text = self.template_engine.render_icon(icon_name);
            elems.push(crate::layout::Elem::text(icon_text));
        }
        if let Some(widget_def) = &module.widget {
            elems.extend(self.template_engine.render_widget(mod_id, widget_def, &data, None));
        }

        if elems.is_empty() {
            return;
        }

        // Replace previous spotlight toast
        if let Some(tid) = self.spotlight_toast_id.take() {
            self.remove_toast(tid);
        }
        let tid = self.set_nav_toast(elems);
        self.spotlight_toast_id = Some(tid);
        self.pause_regular_toasts();
    }

    fn set_badge_override(&mut self, mod_path: &str, timeout: u64) {
        if let Some(old_token) = self.badge_overrides.remove(mod_path) {
            self.loop_handle.remove(old_token);
        }

        let path = mod_path.to_string();
        let token = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_secs(timeout)),
            move |_, _, app| {
                log::info!("badge override expired for {}", path);
                app.badge_overrides.remove(&path);
                app.dirty.set(true);
                TimeoutAction::Drop
            },
        ).expect("failed to set badge override timer");

        self.badge_overrides.insert(mod_path.to_string(), token);
        self.dirty.set(true);
    }

    fn draw_bar(
        bar: &mut BarInstance,
        config: &Config,
        renderer: &mut Renderer,
        template_engine: &TemplateEngine,
        states: &Rc<std::cell::RefCell<HashMap<String, ModuleState>>>,
        qh: &QueueHandle<BarApp>,
        nav: &mut NavState,
        root_scroll: &mut usize,
        toasts: &[Toast],
        badge_overrides: &HashMap<String, RegistrationToken>,
        interactive: &HashMap<String, Box<dyn InteractiveModule>>,
        nav_age: Duration,
        location_age: Duration,
    ) {
        if bar.width == 0 {
            return;
        }

        let track = config.settings.resolve_track();
        let pill = config.settings.resolve_pill();
        let pal = Palette {
            selected: Rgba::new(255, 255, 255, 204), // 80%
            active: Rgba::new(255, 255, 255, 140),   // 55%
            idle: Rgba::new(255, 255, 255, 89),      // 35%
        };

        let output_mul = config.settings.monitor_scale(bar.output_name.as_deref());
        let bar_w = bar.width as f32;
        let track_pad = track.padding * output_mul;
        let output_name = bar.output_name.as_deref();
        let gap = config.settings.gap * output_mul;

        let track_bg = track.color.with_opacity(track.opacity * config.settings.theme.opacity);
        let pill_bg = pill.color.with_opacity(pill.opacity * config.settings.theme.opacity);
        let surface_bg = if track.opacity > 0.0 { track_bg } else { Rgba::new(0, 0, 0, 0) };

        let pc = crate::view::PillCfg {
            padding: pill.padding * output_mul,
            radius: pill.radius * output_mul,
            max_chars: pill.max_chars,
        };

        let bar_h = ((renderer.cell_h + 2.0 * pill.padding + 2.0 * track.padding) * output_mul).ceil() as u32;
        let bar_content_w = bar_w - 2.0 * track_pad;
        let cell_w = renderer.cell_w * output_mul;
        let cell_h = renderer.cell_h * output_mul;
        let scale = bar.scale as f32 * output_mul;

        // Build content (views return raw spans, no pagination)
        let mut content = if nav.stack.is_empty() && matches!(nav.mode, DisplayMode::Visual) {
            crate::view::root_content(config, template_engine, states, pal, output_name, badge_overrides, toasts, location_age, gap, pill_bg, &pc)
        } else {
            match nav.mode {
                DisplayMode::Visual => {
                    let mod_id = nav.stack.first().map(|s| s.as_str());
                    crate::view::mod_content(
                        mod_id, config, template_engine, states, pal, output_name, interactive, gap, pill_bg, &pc,
                    ).unwrap_or_else(|| {
                        crate::view::text_content(nav, config, template_engine, states, pal, gap, pill_bg, &pc)
                    })
                }
                DisplayMode::Text => {
                    crate::view::text_content(nav, config, template_engine, states, pal, gap, pill_bg, &pc)
                }
            }
        };

        // Fixed side zone widths; center gets the rest
        let side_w = 300.0 * output_mul;
        content.left_w = Some(side_w);
        content.right_w = Some(side_w);
        let center_avail = bar_content_w - 2.0 * side_w - 2.0 * gap;

        // Accurate measurement for pagination and layout
        let metrics = Metrics::measure(&content, cell_w, cell_h, scale, output_mul, renderer, &bar.icons);
        let left_n = content.left.len();

        let selected = nav.selected;
        let scroll = if nav.stack.is_empty() && matches!(nav.mode, DisplayMode::Visual) {
            root_scroll
        } else {
            &mut nav.scroll
        };
        content.center = crate::view::paginate_spans(
            content.center, selected, scroll, center_avail,
            left_n, &metrics, template_engine, gap, pill_bg, pal.idle, &pc,
        );

        // Re-measure after pagination (center spans changed)
        let metrics = Metrics::measure(&content, cell_w, cell_h, scale, output_mul, renderer, &bar.icons);
        let mut frame = lay(&content, bar_w, bar_h as f32, track_pad, &metrics);

        // Nav transition: ease-in over 300ms
        let nav_fade = ease_out((nav_age.as_secs_f32() / 0.3).min(1.0));
        if nav_fade < 1.0 {
            for span in &mut frame.spans {
                span.opacity *= nav_fade;
            }
        }

        let scale = bar.scale as u32;
        let phys_w = bar.width * scale;
        let phys_h = bar_h * scale;
        let mut pixmap = Pixmap::new(phys_w.max(1), phys_h.max(1)).expect("failed to create pixmap");
        renderer.render_frame(&frame, &mut pixmap, &bar.icons, surface_bg, bar.scale, output_mul);

        bar.frame = Some(frame);

        let stride = phys_w as i32 * 4;
        let format = smithay_client_toolkit::reexports::client::protocol::wl_shm::Format::Argb8888;
        let (buffer, canvas) = bar.pool
            .create_buffer(phys_w as i32, phys_h as i32, stride, format)
            .expect("failed to create buffer");

        Renderer::copy_to_wl_buffer(&pixmap, canvas);

        bar.layer_surface.set_size(0, bar_h);
        let exclusive = bar_h as i32;
        bar.layer_surface.set_exclusive_zone(exclusive);
        bar.layer_surface.wl_surface().set_buffer_scale(bar.scale);
        bar.layer_surface.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        bar.layer_surface.wl_surface().damage_buffer(0, 0, phys_w as i32, phys_h as i32);
        bar.layer_surface.wl_surface().commit();

        bar.layer_surface.wl_surface().frame(qh, bar.layer_surface.wl_surface().clone());
    }

    pub(crate) fn bar_id_for_surface(&self, surface: &WlSurface) -> Option<u32> {
        self.bars.iter().find_map(|(&id, bar)| {
            if bar.layer_surface.wl_surface() == surface {
                Some(id)
            } else {
                None
            }
        })
    }

}

// SCTK delegate implementations

impl CompositorHandler for BarApp {
    fn scale_factor_changed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, surface: &WlSurface, factor: i32) {
        let id = self.bar_id_for_surface(surface);
        if let Some(bar) = id.and_then(|id| self.bars.get_mut(&id)) {
            let new_scale = factor.max(2);
            if new_scale != bar.scale {
                bar.scale = new_scale;
                bar.icons = IconSet::load(
                    self.config.settings.icons_dir.as_deref(),
                    &self.config.settings.icon_weight,
                    self.renderer.cell_h * new_scale as f32,
                    &self.icon_map,
                );
                log::info!("scale factor changed to {new_scale} (output reports {factor})");
            }
        }
        self.dirty.set(true);
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _transform: smithay_client_toolkit::reexports::client::protocol::wl_output::Transform,
    ) {}

    fn frame(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &WlSurface, _time: u32) {
    }

    fn surface_enter(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &WlSurface, _output: &WlOutput) {}

    fn surface_leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &WlSurface, _output: &WlOutput) {}
}

impl LayerShellHandler for BarApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        let id = self.bar_id_for_surface(layer.wl_surface());
        if let Some(id) = id {
            log::info!("layer surface closed: removing bar {id}");
            self.bars.remove(&id);
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let id = self.bar_id_for_surface(layer.wl_surface());
        if let Some(bar) = id.and_then(|id| self.bars.get_mut(&id)) {
            bar.width = configure.new_size.0;
            if bar.width == 0 {
                return;
            }
            bar.configured = true;
            self.dirty.set(true);
        }
    }
}

impl OutputHandler for BarApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }

    fn new_output(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, output: WlOutput) {
        let info = self.output.info(&output);
        let (id, name) = match info {
            Some(info) => (info.id, info.name.clone()),
            None => {
                log::warn!("new_output: no info available");
                return;
            }
        };

        log::info!("new output: id={id} name={name:?}");

        let bar = BarInstance::new(
            &output, name,
            &self.compositor, &self.layer_shell, &self.shm,
            &self.config, &self.renderer, qh, &self.icon_map,
        );
        self.bars.insert(id, bar);

        // Re-apply wallpaper so the new output isn't blank
        Self::spawn_command("cyberdeck wallpaper init");
    }

    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}

    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, output: WlOutput) {
        let id = self.bars.iter().find_map(|(&id, bar)| {
            if bar.output == output { Some(id) } else { None }
        });
        if let Some(id) = id {
            log::info!("output destroyed: removing bar {id}");
            self.bars.remove(&id);
        }
    }
}

impl SeatHandler for BarApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}

    fn new_capability(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, capability: Capability) {
        if matches!(capability, Capability::Keyboard) && self.keyboard.is_none() {
            match self.seat.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|_state, _kb, _event| {}),
            ) {
                Ok(kb) => {
                    log::info!("keyboard acquired");
                    self.keyboard = Some(kb);
                }
                Err(e) => log::error!("failed to get keyboard: {e:?}"),
            }
        }
        if matches!(capability, Capability::Pointer) && self.pointer.is_none() {
            match self.seat.get_pointer(qh, &seat) {
                Ok(ptr) => {
                    log::info!("pointer acquired");
                    self.pointer = Some(ptr);
                }
                Err(e) => log::error!("failed to get pointer: {e:?}"),
            }
        }
    }

    fn remove_capability(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat, capability: Capability) {
        if matches!(capability, Capability::Keyboard) {
            log::info!("keyboard removed");
            self.keyboard.take();
        }
        if matches!(capability, Capability::Pointer) {
            log::info!("pointer removed");
            self.pointer.take();
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}
}

impl KeyboardHandler for BarApp {
    fn enter(
        &mut self, _conn: &Connection, _qh: &QueueHandle<Self>,
        _kb: &WlKeyboard, _surface: &WlSurface, _serial: u32,
        _raw: &[u32], _keysyms: &[Keysym],
    ) {
        log::debug!("keyboard enter");
    }

    fn leave(
        &mut self, _conn: &Connection, _qh: &QueueHandle<Self>,
        _kb: &WlKeyboard, _surface: &WlSurface, _serial: u32,
    ) {
        log::debug!("keyboard leave");
    }

    fn press_key(
        &mut self, _conn: &Connection, _qh: &QueueHandle<Self>,
        _kb: &WlKeyboard, _serial: u32, event: KeyEvent,
    ) {
        log::debug!("key press: {:?}", event.keysym);
        self.handle_key(event);
    }

    fn release_key(
        &mut self, _conn: &Connection, _qh: &QueueHandle<Self>,
        _kb: &WlKeyboard, _serial: u32, _event: KeyEvent,
    ) {}

    fn update_modifiers(
        &mut self, _conn: &Connection, _qh: &QueueHandle<Self>,
        _kb: &WlKeyboard, _serial: u32, modifiers: Modifiers, _layout: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for BarApp {
    fn pointer_frame(
        &mut self, _conn: &Connection, _qh: &QueueHandle<Self>,
        _pointer: &WlPointer, events: &[PointerEvent],
    ) {
        for event in events {
            if let PointerEventKind::Press { button, .. } = event.kind {
                if button == 0x110 {
                    self.handle_click(&event.surface, event.position.0, event.position.1);
                }
            }
        }
    }
}

impl ShmHandler for BarApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for BarApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

delegate_registry!(BarApp);
delegate_compositor!(BarApp);
delegate_output!(BarApp);
delegate_seat!(BarApp);
delegate_shm!(BarApp);
delegate_keyboard!(BarApp);
delegate_pointer!(BarApp);
delegate_layer!(BarApp);

/// Smooth ease-out curve (decelerate)
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Compute toast opacity: fade in 300ms, fade out last 500ms
pub fn toast_opacity(toast: &Toast) -> f32 {
    let age = toast.created.elapsed().as_secs_f32();
    let lifetime = toast.lifetime.as_secs_f32();
    let remaining = (lifetime - age).max(0.0);

    let fade_in = ease_out((age / 0.3).min(1.0));
    let fade_out = if remaining < 0.5 {
        ease_out(remaining / 0.5)
    } else {
        1.0
    };

    fade_in * fade_out
}
