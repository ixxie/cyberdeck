use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use smithay_client_toolkit::reexports::calloop::RegistrationToken;

use crate::bar::{NavState, Palette, Toast};
use crate::color::Rgba;
use crate::config::{Config, ModuleDef};
use crate::layout::{Elem, Metrics, Span, Zone};
use crate::mods::InteractiveModule;
use crate::source::ModuleState;
use crate::template::TemplateEngine;

#[derive(Debug, Clone)]
pub(crate) enum TextItem {
    Module { id: String },
    App { exec: String, desktop_id: Option<String> },
}

pub(crate) struct PillCfg {
    pub padding: f32,
    pub radius: f32,
}

fn pill(elems: Vec<Elem>, bg: Rgba, pc: &PillCfg) -> Span {
    Span::new(elems).bg(bg).radius(pc.radius).pad(pc.padding, pc.padding)
}

fn pill_bright(elems: Vec<Elem>, bg: Rgba, pc: &PillCfg) -> Span {
    let bright = Rgba::new(bg.r, bg.g, bg.b, (bg.a as f32 * 1.3).min(255.0) as u8);
    Span::new(elems).bg(bright).radius(pc.radius).pad(pc.padding, pc.padding)
}

/// Fixed pagination: fill a page from `scroll`, flip pages when cursor hits edge.
fn window_spans(
    spans: Vec<Span>,
    selected: usize,
    scroll: &mut usize,
    available_w: f32,
    m: &Metrics,
    template_engine: &TemplateEngine,
    gap: f32,
    bg: Rgba,
    caret_fg: Rgba,
    pc: &PillCfg,
) -> Vec<Span> {
    if spans.is_empty() { return spans; }

    let total_w: f32 = spans.iter().map(|s| m.span_w(s)).sum::<f32>()
        + (spans.len().saturating_sub(1)) as f32 * gap;
    if total_w <= available_w {
        *scroll = 0;
        return spans;
    }

    let left_caret = pill(vec![Elem::text(template_engine.render_icon("caret-left")).fg(caret_fg)], bg, pc)
        .path("__scroll_left");
    let right_caret = pill(vec![Elem::text(template_engine.render_icon("caret-right")).fg(caret_fg)], bg, pc)
        .path("__scroll_right");
    let caret_w = m.span_w(&left_caret) + gap;

    // Greedily compute page end from a given start
    let page_end = |start: usize| -> usize {
        let has_left = start > 0;
        let left_reserve = if has_left { caret_w } else { 0.0 };
        let mut used = left_reserve;
        let mut end = start;
        for (i, span) in spans[start..].iter().enumerate() {
            let sw = m.span_w(span) + if i > 0 { gap } else { 0.0 };
            let right_reserve = if start + i + 1 < spans.len() { caret_w } else { 0.0 };
            if used + sw + right_reserve > available_w && i > 0 {
                break;
            }
            used += sw;
            end = start + i + 1;
        }
        end
    };

    // Walk backward to find page start that ends at or before `before`
    let page_start_before = |before: usize| -> usize {
        let mut start = before.saturating_sub(1);
        loop {
            let end = page_end(start);
            if end >= before && start > 0 {
                // This page overshoots; try one earlier
                start -= 1;
            } else {
                // Found a page ending at or before `before`, or hit 0
                return start;
            }
        }
    };

    *scroll = (*scroll).min(spans.len().saturating_sub(1));
    let mut end = page_end(*scroll);

    // Page forward: selected past current page end
    if selected >= end {
        *scroll = end;
        end = page_end(*scroll);
    }

    // Page backward: selected before current page start
    if selected < *scroll {
        *scroll = page_start_before(*scroll);
        end = page_end(*scroll);
    }

    let has_left = *scroll > 0;
    let has_right = end < spans.len();

    let mut result = Vec::new();
    if has_left { result.push(left_caret); }
    result.extend(spans.into_iter().skip(*scroll).take(end - *scroll));
    if has_right { result.push(right_caret); }
    result
}

pub(crate) fn root_zones(
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    pal: Palette,
    output_name: Option<&str>,
    badge_overrides: &HashMap<String, RegistrationToken>,
    toasts: &[Toast],
    gap: f32,
    bg: Rgba,
    _icon_h: u32,
    _bar_w: f32,
    _m: &Metrics,
    pc: &PillCfg,
) -> Vec<Zone> {
    let states_ref = states.borrow();

    let render_badges = |id: &str| -> Vec<Elem> {
        let Some(child) = config.bar.modules.get(id) else { return Vec::new() };
        let data = states_ref.get(id)
            .map(|s| &s.data)
            .unwrap_or(&serde_json::Value::Null);
        let mut elems = Vec::new();
        for (badge_name, badge) in &child.badges {
            let key = format!("{id}.{badge_name}");
            let highlighted = badge_overrides.contains_key(&key);
            if let Some(elem) = template_engine.render_badge(id, badge_name, badge, data, output_name, highlighted) {
                elems.push(elem.fg(pal.active).path(id.to_string()));
            }
        }
        elems
    };

    // Left: launcher + window title
    let mut left_spans = Vec::new();
    let launcher_icon = template_engine.render_icon("terminal");
    let mut nav_elems = vec![Elem::text(launcher_icon).fg(pal.selected)];
    nav_elems.extend(render_badges("window"));
    left_spans.push(pill(nav_elems, bg, pc).path("launcher"));

    // Center: toasts
    let mut center_spans = Vec::new();
    let toast_fg = Rgba::new(pal.active.r, pal.active.g, pal.active.b,
        (pal.active.a as f32 * 0.85) as u8);
    for t in toasts {
        let opacity = crate::bar::toast_opacity(t);
        if !t.elems.is_empty() {
            // Structured toast (nav indicators): each elem is its own item in the pill
            let elems: Vec<Elem> = t.elems.iter().cloned().map(|e| {
                if e.fg == Rgba::default() { e.fg(toast_fg) } else { e }
            }).collect();
            center_spans.push(pill_bright(elems, bg, pc).opacity(opacity));
        } else {
            let text = if t.text.len() > 80 {
                let mut s = t.text.chars().take(77).collect::<String>();
                s.push_str("...");
                s
            } else {
                t.text.clone()
            };
            let mut elem = Elem::text(text).fg(toast_fg);
            if let Some(ref pm) = t.icon_pixmap {
                elem = elem.icon(pm.clone());
            }
            center_spans.push(pill_bright(vec![elem], bg, pc).opacity(opacity));
        }
    }

    // Right: alert badges + clock
    let mut right_spans = Vec::new();

    let mut mod_ids: Vec<&String> = config.bar.modules.keys().collect();
    mod_ids.sort();
    for id in mod_ids {
        match id.as_str() {
            "calendar" | "window" | "workspaces" => continue,
            _ => {
                let badge_elems = render_badges(id);
                for elem in badge_elems {
                    right_spans.push(pill(vec![elem], bg, pc));
                }
            }
        }
    }

    // Clock (calendar badges)
    let clock_elems = render_badges("calendar");
    if !clock_elems.is_empty() {
        right_spans.push(pill(clock_elems, bg, pc).path("calendar"));
    }

    drop(states_ref);

    vec![
        Zone::left(left_spans, gap),
        Zone::center(center_spans, gap),
        Zone::right(right_spans, gap),
    ]
}

pub(crate) fn mod_zones(
    mod_id: Option<&str>,
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    pal: Palette,
    output_name: Option<&str>,
    interactive: &HashMap<String, Box<dyn InteractiveModule>>,
    gap: f32,
    bg: Rgba,
    pc: &PillCfg,
) -> Option<Vec<Zone>> {
    let id = mod_id?;
    let module = config.bar.modules.get(id)?;

    let states_ref = states.borrow();

    // Deep module rendering
    if let Some(deep) = interactive.get(id) {
        let data = states_ref.get(id)
            .map(|s| &s.data)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        drop(states_ref);

        let items = deep.render_center(pal.selected, &data);
        let cursor = deep.cursor();
        let center_spans: Vec<Span> = items.into_iter().enumerate().map(|(i, elems)| {
            if cursor == Some(i) {
                pill_bright(elems, bg, pc)
            } else {
                pill(elems, bg, pc)
            }
        }).collect();

        let mut hints = Vec::new();
        for hint in deep.key_hints() {
            let display = if hint.label.is_empty() {
                format!("[{}] {}", hint.key, hint.action)
            } else {
                format!("[{}] {}", hint.key, hint.label)
            };
            hints.push(Elem::text(display).fg(pal.idle));
        }

        let breadcrumb = render_breadcrumb_elems(module, template_engine, pal.active);

        return Some(vec![
            Zone::left(vec![pill(breadcrumb, bg, pc).path("__back")], gap),
            Zone::center(center_spans, gap),
            Zone::right(vec![pill(hints, bg, pc)], gap),
        ]);
    }

    // Render the module's widget
    let data = states_ref.get(id)
        .map(|s| &s.data)
        .unwrap_or(&serde_json::Value::Null);

    let mut center_spans = Vec::new();
    if let Some(widget_def) = &module.widget {
        let elems = template_engine.render_widget(id, widget_def, data, output_name);
        for elem in elems {
            center_spans.push(pill(vec![elem.fg(pal.active)], bg, pc));
        }
    }

    // Show module view if has widget OR has key-hints
    if center_spans.is_empty() && module.key_hints.is_empty() {
        return None;
    }

    let breadcrumb = render_breadcrumb_elems(module, template_engine, pal.active);
    drop(states_ref);

    // Key hints
    let mut hints = Vec::new();
    for hint in &module.key_hints {
        let display = if hint.label.is_empty() {
            format!("[{}] {}", hint.key, hint.action)
        } else {
            format!("[{}] {}", hint.key, hint.label)
        };
        hints.push(Elem::text(display).fg(pal.idle));
    }
    if hints.is_empty() {
        hints.push(Elem::text("[Esc] back").fg(pal.idle));
    }

    Some(vec![
        Zone::left(vec![pill(breadcrumb, bg, pc).path("__back")], gap),
        Zone::center(center_spans, gap),
        Zone::right(vec![pill(hints, bg, pc)], gap),
    ])
}

fn render_breadcrumb_elems(
    module: &ModuleDef,
    template_engine: &TemplateEngine,
    fg: Rgba,
) -> Vec<Elem> {
    let mut elems = Vec::new();

    if let Some(icon_name) = &module.icon {
        let icon_text = template_engine.render_icon(icon_name);
        elems.push(Elem::text(icon_text).fg(fg));
    }
    elems.push(Elem::text(module.name.clone()).fg(fg));

    elems
}

pub(crate) fn text_zones(
    nav: &mut NavState,
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    pal: Palette,
    gap: f32,
    bg: Rgba,
    bar_w: f32,
    m: &Metrics,
    pc: &PillCfg,
) -> Vec<Zone> {
    let q = nav.query.to_lowercase();

    let mut items: Vec<(String, Vec<usize>, Option<String>)> = Vec::new();

    if nav.stack.is_empty() {
        let mut sorted: Vec<(&String, &ModuleDef)> = config.bar.modules.iter().collect();
        sorted.sort_by(|a, b| a.1.name.cmp(&b.1.name));

        for (_id, child) in &sorted {
            if child.widget.is_none() && child.module_type.is_none() { continue; }
            let display = child.name.to_lowercase();
            let icon_text = child.icon.as_ref().map(|name| template_engine.render_icon(name));
            if q.is_empty() {
                items.push((display, vec![], icon_text));
            } else if let Some(pos) = display.find(&q) {
                items.push((display, (pos..pos + q.len()).collect(), icon_text));
            }
        }

        let states_ref = states.borrow();
        if !q.is_empty() {
            if let Some(launcher_state) = states_ref.get("__launcher") {
                if let Some(entries) = launcher_state.data.get("entries").and_then(|v| v.as_array()) {
                    for entry in entries {
                        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if name.is_empty() { continue; }
                        let display = name.to_lowercase();
                        if let Some(pos) = display.find(&q) {
                            items.push((display, (pos..pos + q.len()).collect(), None));
                        }
                    }
                }
            }
        }
    } else {
        let states_ref = states.borrow();
        let mod_id = &nav.stack[0];
        if let Some(state) = states_ref.get(mod_id) {
            if let Some(entries) = state.data.get("entries").and_then(|v| v.as_array()) {
                for entry in entries {
                    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if name.is_empty() { continue; }
                    if q.is_empty() {
                        items.push((name.to_string(), vec![], None));
                    } else {
                        let lower = name.to_lowercase();
                        if let Some(pos) = lower.find(&q) {
                            items.push((name.to_string(), (pos..pos + q.len()).collect(), None));
                        }
                    }
                }
            }
        }
    }

    let total = items.len();
    let selected = nav.selected.min(items.len().saturating_sub(1));

    // Left: launcher icon + query
    let launcher_icon = template_engine.render_icon("terminal");
    let query_display = format!("{}_", nav.query);
    let left_elems = vec![
        Elem::text(launcher_icon).fg(pal.selected),
        Elem::text(query_display).fg(pal.selected),
    ];
    let left_spans = vec![pill(left_elems, bg, pc)];

    // Center: result items as individual pill spans
    let center_spans: Vec<Span> = items.iter().enumerate().map(|(i, (text, _, icon))| {
        let is_sel = i == selected;
        let item_fg = if is_sel { pal.selected } else { pal.idle };
        let mut elems = Vec::new();
        if let Some(icon_text) = icon {
            elems.push(Elem::text(icon_text.clone()).fg(item_fg));
        }
        elems.push(Elem::text(text.clone()).fg(item_fg));
        if is_sel {
            pill_bright(elems, bg, pc)
        } else {
            pill(elems, bg, pc)
        }
    }).collect();

    // Right: count
    let count_text = format!("{}/{}", items.len(), total);
    let right_elems = vec![Elem::text(count_text).fg(pal.idle)];
    let right_spans = vec![pill(right_elems, bg, pc)];

    // Apply windowing to center spans
    let left_w: f32 = left_spans.iter().map(|s| m.span_w(s)).sum::<f32>()
        + left_spans.len().saturating_sub(1) as f32 * gap;
    let right_w: f32 = right_spans.iter().map(|s| m.span_w(s)).sum::<f32>()
        + right_spans.len().saturating_sub(1) as f32 * gap;
    let center_avail = bar_w - left_w - right_w - 2.0 * gap;
    let center_spans = window_spans(
        center_spans, nav.selected, &mut nav.scroll,
        center_avail, m, template_engine, gap, bg, pal.idle, pc,
    );

    vec![
        Zone::left(left_spans, gap),
        Zone::center(center_spans, gap),
        Zone::right(right_spans, gap),
    ]
}

/// Workspace indicator: one Elem per workspace with hexagon icon
pub fn ws_indicator_elems(
    workspaces: &[serde_json::Value],
    template_engine: &TemplateEngine,
) -> Vec<Elem> {
    let icon = template_engine.render_icon("hexagon");
    let bright = Rgba::new(255, 255, 255, 200);
    let dim = Rgba::new(255, 255, 255, 60);

    workspaces.iter().map(|ws| {
        let focused = ws.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
        Elem::text(icon.clone()).fg(if focused { bright } else { dim })
    }).collect()
}

/// Window indicator: one Elem per window with app-window icon
pub fn win_indicator_elems(
    workspaces: &[serde_json::Value],
    windows: &[serde_json::Value],
    template_engine: &TemplateEngine,
) -> Vec<Elem> {
    let focused_ws = workspaces.iter().find(|ws| {
        ws.get("focused").and_then(|v| v.as_bool()).unwrap_or(false)
    });
    let Some(ws) = focused_ws else { return Vec::new() };
    let ws_id = ws.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

    let mut ws_wins: Vec<&serde_json::Value> = windows.iter()
        .filter(|w| w.get("workspace_id").and_then(|v| v.as_i64()).unwrap_or(-1) == ws_id)
        .collect();
    ws_wins.sort_by_key(|w| {
        let col = w.get("col").and_then(|v| v.as_i64()).unwrap_or(0);
        let row = w.get("row").and_then(|v| v.as_i64()).unwrap_or(0);
        (col, row)
    });

    let icon = template_engine.render_icon("app-window");
    let bright = Rgba::new(255, 255, 255, 200);
    let dim = Rgba::new(255, 255, 255, 60);

    ws_wins.iter().map(|w| {
        let focused = w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
        Elem::text(icon.clone()).fg(if focused { bright } else { dim })
    }).collect()
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
        let mut sorted: Vec<(&String, &ModuleDef)> = config.bar.modules.iter().collect();
        sorted.sort_by(|a, b| a.1.name.cmp(&b.1.name));

        for (id, m) in sorted {
            let display = m.name.to_lowercase();
            if q.is_empty() || display.contains(&q) {
                items.push((display, TextItem::Module { id: id.clone() }));
            }
        }

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
