use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

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
use crate::layout::HitArea;
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
    pub hit_areas: Vec<HitArea>,
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
    ) -> Self {
        let bar_h = renderer.bar_height();
        let surface = compositor.create_surface(qh);
        let layer_surface = layer_shell.create_layer_surface(
            qh, surface, Layer::Top, Some("cyberdeck"), Some(output),
        );

        let anchor = match config.settings.position {
            Position::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            Position::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        };
        layer_surface.set_anchor(anchor);

        layer_surface.set_size(0, bar_h);
        layer_surface.set_exclusive_zone(bar_h as i32);
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
            hit_areas: Vec::new(),
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
    pub modifiers: Modifiers,
    pub toasts: Vec<Toast>,
    pub badge_overrides: HashMap<String, RegistrationToken>,
    pub spotlight_token: Option<RegistrationToken>,
    pub interactive: HashMap<String, Box<dyn InteractiveModule>>,
}

pub struct Toast {
    pub toast_id: u64,
    pub text: String,
    pub icon: Option<String>,
    pub icon_pixmap: Option<std::sync::Arc<tiny_skia::Pixmap>>,
    pub token: RegistrationToken,
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

        let pad_left = config.settings.padding_left.or(config.settings.padding_horizontal);
        let pad_right = config.settings.padding_right.or(config.settings.padding_horizontal);
        let renderer = Renderer::new(&config.settings.font, config.settings.font_size, config.settings.padding, pad_left, pad_right);
        let template_engine = TemplateEngine::new(&config.bar);

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
            spotlight_token: None,
            interactive,
        }
    }

    pub fn current_module(&self) -> Option<&ModuleDef> {
        if self.nav.stack.is_empty() {
            return None;
        }
        self.config.bar.modules.get(&self.nav.stack[0])
    }

    pub fn set_nav(&mut self, nav: NavState) {
        // Cancel any active spotlight timer
        if let Some(token) = self.spotlight_token.take() {
            self.loop_handle.remove(token);
        }

        let needs_kb = !(nav.stack.is_empty() && matches!(nav.mode, DisplayMode::Visual));
        log::info!("nav -> stack={:?} mode={:?} kb={}", nav.stack, nav.mode, needs_kb);
        self.nav = nav;
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

    pub fn maybe_redraw(&mut self) {
        if !self.dirty.get() {
            return;
        }
        self.process_hooks();
        let qh = self.qh.clone();
        let ids: Vec<u32> = self.bars.keys().copied().collect();
        let mut drew = false;
        for id in ids {
            if let Some(bar) = self.bars.get_mut(&id) {
                if bar.configured && bar.width > 0 {
                    Self::draw_bar(
                        bar, &self.config, &mut self.renderer,
                        &self.template_engine, &self.states, &qh,
                        &self.nav,
                        &self.toasts, &self.badge_overrides,
                        &self.interactive,
                    );
                    drew = true;
                }
            }
        }
        if drew {
            self.dirty.set(false);
        }
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
                            actions.push((child_id.clone(), hook.action.clone(), hook.timeout));
                        }
                    }
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
            token,
        });
        self.dirty.set(true);
    }

    fn remove_toast(&mut self, tid: u64) {
        if let Some(pos) = self.toasts.iter().position(|t| t.toast_id == tid) {
            let old = self.toasts.remove(pos);
            self.loop_handle.remove(old.token);
        }
    }

    fn set_spotlight(&mut self, mod_id: &str, timeout: u64) {
        let module = match self.config.bar.modules.get(mod_id) {
            Some(m) => m,
            None => return,
        };
        let mode = if module.has_view() {
            DisplayMode::Visual
        } else {
            return;
        };

        // Cancel existing spotlight timer if re-triggered
        if let Some(old) = self.spotlight_token.take() {
            self.loop_handle.remove(old);
        }

        self.set_nav(NavState::module(mod_id, mode));

        let token = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_secs(timeout)),
            move |_, _, app| {
                log::info!("spotlight expired");
                app.spotlight_token = None;
                app.set_nav(NavState::new());
                TimeoutAction::Drop
            },
        ).expect("failed to set spotlight timer");

        self.spotlight_token = Some(token);
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
        nav: &NavState,
        toasts: &[Toast],
        badge_overrides: &HashMap<String, RegistrationToken>,
        interactive: &HashMap<String, Box<dyn InteractiveModule>>,
    ) {
        if bar.width == 0 {
            return;
        }

        let bg = config.settings.background.color
            .with_opacity(config.settings.background.opacity);
        let pal = Palette {
            selected: Rgba::new(255, 255, 255, 230), // 90%
            active: Rgba::new(255, 255, 255, 166),   // 65%
            idle: Rgba::new(255, 255, 255, 102),     // 40%
        };

        let output_mul = bar.output_name.as_ref()
            .and_then(|name| config.settings.output_scales.get(name))
            .copied()
            .unwrap_or(1.0);
        let bar_content_w = bar.width as f32 - (renderer.pad_left + renderer.pad_right) * output_mul;
        let gap_px = renderer.cell_w * output_mul * 1.5;

        let layout = if nav.stack.is_empty() && matches!(nav.mode, DisplayMode::Visual) {
            crate::view::layout_root_visual(bar, config, template_engine, states, renderer, pal, bg, bar_content_w, output_mul, gap_px, badge_overrides, toasts)
        } else {
            match nav.mode {
                DisplayMode::Visual => {
                    let mod_id = nav.stack.first().map(|s| s.as_str());
                    crate::view::layout_module_view(
                        mod_id,
                        config, template_engine, states, bar, renderer, pal, bg, output_mul, bar_content_w, gap_px, interactive,
                    ).unwrap_or_else(|| {
                        crate::view::layout_text(bar, config, template_engine, states, nav, renderer, pal, bg, output_mul, bar_content_w, gap_px)
                    })
                }
                DisplayMode::Text => {
                    crate::view::layout_text(bar, config, template_engine, states, nav, renderer, pal, bg, output_mul, bar_content_w, gap_px)
                }
            }
        };
        bar.hit_areas = layout.hit_areas.clone();

        let scale = bar.scale as u32;
        let bar_h = ((renderer.cell_h + 2.0 * renderer.padding) * output_mul).ceil() as u32;
        let phys_w = bar.width * scale;
        let phys_h = bar_h * scale;
        let mut pixmap = Pixmap::new(phys_w.max(1), phys_h.max(1)).expect("failed to create pixmap");
        renderer.render_layout(&layout, &mut pixmap, &bar.icons, bg, bar.scale, output_mul);

        let stride = phys_w as i32 * 4;
        let format = smithay_client_toolkit::reexports::client::protocol::wl_shm::Format::Argb8888;
        let (buffer, canvas) = bar.pool
            .create_buffer(phys_w as i32, phys_h as i32, stride, format)
            .expect("failed to create buffer");

        Renderer::copy_to_wl_buffer(&pixmap, canvas);

        bar.layer_surface.set_size(0, bar_h);
        bar.layer_surface.set_exclusive_zone(bar_h as i32);
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
            self.bars.remove(&id);
        }
        if self.bars.is_empty() {
            std::process::exit(0);
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
            &self.config, &self.renderer, qh,
        );
        self.bars.insert(id, bar);
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
