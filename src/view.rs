use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use smithay_client_toolkit::reexports::calloop::RegistrationToken;

use crate::bar::{BarInstance, NavState, Palette, Toast};
use crate::color::Rgba;
use crate::config::{BarDef, Config, ModuleDef};
use crate::layout::{Layout, RenderedWidget};
use crate::mods::InteractiveModule;
use crate::render::Renderer;
use crate::source::ModuleState;
use crate::template::TemplateEngine;

#[derive(Debug, Clone)]
pub(crate) enum TextItem {
    Module { id: String },
    App { exec: String, desktop_id: Option<String> },
}

pub(crate) fn layout_root_visual(
    bar: &BarInstance,
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    renderer: &Renderer,
    pal: Palette,
    bg: Rgba,
    bar_content_w: f32,
    output_mul: f32,
    gap_px: f32,
    badge_overrides: &HashMap<String, RegistrationToken>,
    toast: &Option<Toast>,
) -> Layout {
    let states_ref = states.borrow();
    let output_name = bar.output_name.as_deref();

    let render_badges = |id: &str| -> Vec<RenderedWidget> {
        let Some(child) = config.bar.modules.get(id) else { return Vec::new() };
        let data = states_ref.get(id)
            .map(|s| &s.data)
            .unwrap_or(&serde_json::Value::Null);
        let mut widgets = Vec::new();
        for (badge_name, badge) in &child.badges {
            let key = format!("{id}.{badge_name}");
            let highlighted = badge_overrides.contains_key(&key);
            if let Some(rw) = template_engine.render_badge(id, badge_name, badge, data, output_name, highlighted) {
                widgets.push(rw.with_path(id));
            }
        }
        widgets
    };

    // Fixed root layout: [nav] [clock] [window | workspaces]
    let mut nav = Vec::new();
    if let Some(t) = toast {
        let dim_fg = pal.active;
        if let Some(icon_name) = &t.icon {
            let icon_text = template_engine.render_icon(icon_name);
            nav.push(RenderedWidget::new(icon_text).with_fg(dim_fg));
        }
        nav.push(RenderedWidget::new(t.text.clone()).with_fg(dim_fg));
    } else {
        let launcher_icon = template_engine.render_icon("terminal");
        nav.push(RenderedWidget::new(launcher_icon).with_fg(pal.active).with_path("launcher"));
        for id in &config.bar.order {
            match id.as_str() {
                "calendar" | "window" | "workspaces" => continue,
                _ => nav.extend(render_badges(id)),
            }
        }
    }

    // Center: clock (calendar badges)
    let mut center = Vec::new();
    center.extend(render_badges("calendar"));

    // Right: window title + workspaces
    let mut right = Vec::new();
    right.extend(render_badges("window"));
    right.extend(render_badges("workspaces"));

    drop(states_ref);

    Layout::flex(
        &[nav, center, right], bar_content_w,
        &bar.icons, renderer.cell_w * output_mul, bar.scale as f32 * output_mul,
        gap_px, pal.active, bg,
    )
}

pub(crate) fn layout_module_view(
    mod_id: Option<&str>,
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    bar: &BarInstance,
    renderer: &Renderer,
    pal: Palette,
    bg: Rgba,
    output_mul: f32,
    bar_content_w: f32,
    gap_px: f32,
    interactive: &HashMap<String, Box<dyn InteractiveModule>>,
) -> Option<Layout> {
    let id = mod_id?;
    let module = config.bar.modules.get(id)?;

    let states_ref = states.borrow();
    let output_name = bar.output_name.as_deref();

    // Deep module rendering
    if let Some(deep) = interactive.get(id) {
        let data = states_ref.get(id)
            .map(|s| &s.data)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        drop(states_ref);
        let center = deep.render_center(pal.selected, &data);
        let mut hints = Vec::new();
        for hint in deep.key_hints() {
            let display = if hint.label.is_empty() {
                format!("[{}] {}", hint.key, hint.action)
            } else {
                format!("[{}] {}", hint.key, hint.label)
            };
            hints.push(RenderedWidget::new(display).with_fg(pal.idle));
        }
        let crumb = deep.breadcrumb();
        let mut breadcrumb = Vec::new();
        if let Some(icon_name) = &module.icon {
            let icon_text = template_engine.render_icon(icon_name);
            breadcrumb.push(RenderedWidget::new(icon_text).with_fg(pal.active));
        }
        breadcrumb.push(RenderedWidget::new(crumb.join(" > ")).with_fg(pal.active));
        return Some(Layout::flex(
            &[breadcrumb, center, hints],
            bar_content_w, &bar.icons, renderer.cell_w * output_mul, bar.scale as f32 * output_mul, gap_px, pal.active, bg,
        ));
    }

    // Render the module's widget
    let data = states_ref.get(id)
        .map(|s| &s.data)
        .unwrap_or(&serde_json::Value::Null);

    let mut content = Vec::new();
    if let Some(widget_def) = &module.widget {
        if let Some(rw) = template_engine.render_widget(id, widget_def, data, output_name) {
            content.push(rw);
        }
    }

    // Show module view if has widget OR has key-hints (so user can see available actions)
    if content.is_empty() && module.key_hints.is_empty() {
        return None;
    }

    // Breadcrumb
    let breadcrumb = render_breadcrumb(
        &config.bar, id,
        template_engine, pal.active,
    );
    drop(states_ref);

    // Key hints
    let dim_fg = pal.idle;
    let mut hints = Vec::new();
    for hint in &module.key_hints {
        let display = if hint.label.is_empty() {
            format!("[{}] {}", hint.key, hint.action)
        } else {
            format!("[{}] {}", hint.key, hint.label)
        };
        hints.push(RenderedWidget::new(display).with_fg(dim_fg));
    }
    if hints.is_empty() {
        hints.push(RenderedWidget::new("[Esc] back".into()).with_fg(dim_fg));
    }

    Some(Layout::flex(
        &[breadcrumb, content, hints],
        bar_content_w, &bar.icons, renderer.cell_w * output_mul, bar.scale as f32 * output_mul, gap_px, pal.active, bg,
    ))
}

pub(crate) fn render_breadcrumb(
    bar: &BarDef, mod_id: &str,
    template_engine: &TemplateEngine,
    fg: Rgba,
) -> Vec<RenderedWidget> {
    let Some(module) = bar.modules.get(mod_id) else {
        return Vec::new();
    };

    let mut widgets = Vec::new();

    // Breadcrumb: icon + name (indicator content belongs on root bar, not here)
    // Launcher shows only the icon (matching root bar appearance)
    if let Some(icon_name) = &module.icon {
        let icon_text = template_engine.render_icon(icon_name);
        widgets.push(RenderedWidget::new(icon_text).with_fg(fg));
    }
    if mod_id != "launcher" {
        widgets.push(RenderedWidget::new(module.name.clone()).with_fg(fg));
    }

    widgets
}

pub(crate) fn layout_text(
    bar: &mut BarInstance,
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    nav: &NavState,
    renderer: &Renderer,
    pal: Palette,
    bg: Rgba,
    output_mul: f32,
    bar_content_w: f32,
    gap_px: f32,
) -> Layout {
    let q = nav.query.to_lowercase();

    let mut items: Vec<(String, Vec<usize>)> = Vec::new();

    if nav.stack.is_empty() {
        // At root: show all modules (sorted by name)
        let mut sorted: Vec<(&String, &ModuleDef)> = config.bar.modules.iter().collect();
        sorted.sort_by(|a, b| a.1.name.cmp(&b.1.name));

        for (_id, child) in &sorted {
            if child.widget.is_none() && child.module_type.is_none() { continue; }
            let display = child.name.to_lowercase();
            if q.is_empty() {
                items.push((display, vec![]));
            } else {
                if let Some(pos) = display.find(&q) {
                    items.push((display, (pos..pos + q.len()).collect()));
                }
            }
        }

        // Merge launcher apps (only when searching)
        let states_ref = states.borrow();
        if !q.is_empty() {
            if let Some(launcher_state) = states_ref.get("__launcher") {
                if let Some(entries) = launcher_state.data.get("entries").and_then(|v| v.as_array()) {
                    for entry in entries {
                        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if name.is_empty() { continue; }
                        let display = name.to_lowercase();
                        if let Some(pos) = display.find(&q) {
                            items.push((display, (pos..pos + q.len()).collect()));
                        }
                    }
                }
            }
        }
    } else {
        // Inside a module: show entries from source data
        let states_ref = states.borrow();
        let mod_id = &nav.stack[0];
        if let Some(state) = states_ref.get(mod_id) {
            if let Some(entries) = state.data.get("entries").and_then(|v| v.as_array()) {
                for entry in entries {
                    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() { continue; }
                    if q.is_empty() {
                        items.push((name.to_string(), vec![]));
                    } else {
                        let lower = name.to_lowercase();
                        if let Some(pos) = lower.find(&q) {
                            items.push((name.to_string(), (pos..pos + q.len()).collect()));
                        }
                    }
                }
            }
        }
    }

    let total = items.len();

    let launcher_icon = template_engine.render_icon("terminal");
    let prefix = Some(RenderedWidget::new(launcher_icon).with_fg(pal.selected));

    Layout::flex_text_mode(
        prefix.as_ref(), &nav.query, &items, total, nav.selected,
        "", bar_content_w,
        &bar.icons, renderer.cell_w * output_mul, bar.scale as f32 * output_mul,
        gap_px, pal.selected, pal.idle, bg,
    )
}

pub(crate) fn text_matched_items(
    nav: &NavState,
    config: &Config,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
) -> Vec<(String, TextItem)> {
    let q = nav.query.to_lowercase();
    let at_root = nav.stack.is_empty();

    let mut items: Vec<(String, TextItem)> = Vec::new();

    if at_root {
        // At root: show all modules
        let mut sorted: Vec<(&String, &ModuleDef)> = config.bar.modules.iter().collect();
        sorted.sort_by(|a, b| a.1.name.cmp(&b.1.name));

        for (id, m) in sorted {
            let display = m.name.to_lowercase();
            if q.is_empty() || display.contains(&q) {
                items.push((display, TextItem::Module { id: id.clone() }));
            }
        }

        // Merge launcher apps (only when searching)
        if !q.is_empty() {
            let states_ref = states.borrow();
            if let Some(launcher_state) = states_ref.get("__launcher") {
                if let Some(entries) = launcher_state.data.get("entries").and_then(|v| v.as_array()) {
                    for entry in entries {
                        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let exec = entry.get("exec").and_then(|v| v.as_str()).unwrap_or("");
                        let desktop_id = entry.get("desktop_id").and_then(|v| v.as_str()).map(String::from);
                        if name.is_empty() || exec.is_empty() { continue; }
                        let display = name.to_lowercase();
                        if display.contains(&q) {
                            items.push((display, TextItem::App { exec: exec.to_string(), desktop_id }));
                        }
                    }
                }
            }
        }
    } else {
        // Inside a module: show entries from source data
        let states_ref = states.borrow();
        let mod_id = &nav.stack[0];
        if let Some(state) = states_ref.get(mod_id) {
            if let Some(entries) = state.data.get("entries").and_then(|v| v.as_array()) {
                for entry in entries {
                    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let exec = entry.get("exec").and_then(|v| v.as_str()).unwrap_or("");
                    let desktop_id = entry.get("desktop_id").and_then(|v| v.as_str()).map(String::from);
                    if name.is_empty() || exec.is_empty() { continue; }
                    let display = name.to_lowercase();
                    if q.is_empty() || display.contains(&q) {
                        items.push((display, TextItem::App { exec: exec.to_string(), desktop_id }));
                    }
                }
            }
        }
    }

    items
}

pub(crate) fn text_match_count(
    nav: &NavState,
    config: &Config,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
) -> usize {
    text_matched_items(nav, config, states).len()
}
