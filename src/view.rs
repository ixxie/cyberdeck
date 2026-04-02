use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::RegistrationToken;

use crate::bar::{NavState, Palette, Toast};
use crate::color::Rgba;
use crate::config::{Config, ModuleDef};
use crate::layout::{BarContent, Elem, Metrics, Span};
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
    pub max_chars: usize,
}

fn truncate_elems(elems: &mut Vec<Elem>, max_chars: usize) {
    if max_chars == 0 { return; }
    let total: usize = elems.iter()
        .map(|e| e.text.chars().filter(|&c| c != '\x01' && c != '\x02').count())
        .sum();
    if total <= max_chars { return; }

    let mut budget = max_chars.saturating_sub(1); // reserve 1 for ellipsis
    let mut truncated_at = None;
    for (ei, elem) in elems.iter_mut().enumerate() {
        let visible: usize = elem.text.chars()
            .filter(|&c| c != '\x01' && c != '\x02')
            .count();
        if visible <= budget {
            budget -= visible;
            continue;
        }
        // Truncate this elem at `budget` visible chars
        let mut seen = 0;
        let mut byte_end = 0;
        for (i, ch) in elem.text.char_indices() {
            if ch == '\x01' || ch == '\x02' {
                byte_end = i + ch.len_utf8();
                continue;
            }
            if seen >= budget { break; }
            seen += 1;
            byte_end = i + ch.len_utf8();
        }
        elem.text.truncate(byte_end);
        elem.text.push('\u{2026}');
        truncated_at = Some(ei);
        break;
    }
    if let Some(idx) = truncated_at {
        elems.truncate(idx + 1);
    }
}

fn pill(mut elems: Vec<Elem>, bg: Rgba, pc: &PillCfg) -> Span {
    truncate_elems(&mut elems, pc.max_chars);
    Span::new(elems).bg(bg).radius(pc.radius).pad(pc.padding, pc.padding)
}

fn pill_bright(mut elems: Vec<Elem>, bg: Rgba, pc: &PillCfg) -> Span {
    truncate_elems(&mut elems, pc.max_chars);
    let bright = Rgba::new(bg.r, bg.g, bg.b, (bg.a as f32 * 1.3).min(255.0) as u8);
    Span::new(elems).bg(bright).radius(pc.radius).pad(pc.padding, pc.padding)
}

/// Fixed pagination: fill a page from `scroll`, flip pages when cursor hits edge.
/// Paginate spans using accurately measured widths from Metrics.
/// `span_offset` is the flat index of the first span in `m.span_w_at()`.
pub(crate) fn paginate_spans(
    spans: Vec<Span>,
    selected: usize,
    scroll: &mut usize,
    available_w: f32,
    span_offset: usize,
    m: &Metrics,
    template_engine: &TemplateEngine,
    gap: f32,
    bg: Rgba,
    caret_fg: Rgba,
    pc: &PillCfg,
) -> Vec<Span> {
    if spans.is_empty() { return spans; }

    let sw = |i: usize| m.span_w_at(span_offset + i);

    let total_w: f32 = (0..spans.len()).map(|i| sw(i)).sum::<f32>()
        + (spans.len().saturating_sub(1)) as f32 * gap;
    if total_w <= available_w {
        *scroll = 0;
        return spans;
    }

    let left_caret = pill(vec![Elem::text(template_engine.render_icon("caret-left")).fg(caret_fg)], bg, pc)
        .path("__scroll_left");
    let right_caret = pill(vec![Elem::text(template_engine.render_icon("caret-right")).fg(caret_fg)], bg, pc)
        .path("__scroll_right");
    // Estimate caret width as 1 icon + padding (close enough for reserving space)
    let caret_w = m.cell_h + 2.0 * pc.padding + gap;

    let page_end = |start: usize| -> usize {
        let has_left = start > 0;
        let left_reserve = if has_left { caret_w } else { 0.0 };
        let mut used = left_reserve;
        let mut end = start;
        for i in start..spans.len() {
            let s = sw(i) + if i > start { gap } else { 0.0 };
            let right_reserve = if i + 1 < spans.len() { caret_w } else { 0.0 };
            if used + s + right_reserve > available_w && i > start {
                break;
            }
            used += s;
            end = i + 1;
        }
        end
    };

    let page_start_before = |before: usize| -> usize {
        let mut start = before.saturating_sub(1);
        loop {
            let end = page_end(start);
            if end >= before && start > 0 {
                start -= 1;
            } else {
                return start;
            }
        }
    };

    *scroll = (*scroll).min(spans.len().saturating_sub(1));
    let mut end = page_end(*scroll);

    if selected >= end {
        *scroll = end;
        end = page_end(*scroll);
    }

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

/// Compute location indicator opacity: bright on change, dims after 2s over 1s
fn location_opacity(location_age: Duration) -> f32 {
    let secs = location_age.as_secs_f32();
    if secs < 2.0 {
        1.0
    } else if secs < 3.0 {
        1.0 - (secs - 2.0) // linear fade over 1s
    } else {
        0.0
    }
}

/// Build workspace pill for the left zone (always visible, clickable).
/// During rename, shows editable query with cursor.
fn workspace_pill(
    states: &HashMap<String, ModuleState>,
    pal: Palette,
    bg: Rgba,
    pc: &PillCfg,
    rename_query: Option<&str>,
) -> Span {
    if let Some(query) = rename_query {
        let text = format!("{query}|");
        return pill(vec![Elem::text(text).fg(pal.selected)], bg, pc).path("workspace");
    }

    let ws_data = states.get("workspaces").map(|s| &s.data);
    let workspaces = ws_data
        .and_then(|d| d.get("workspaces"))
        .and_then(|v| v.as_array());

    let focused_ws = workspaces.and_then(|wss| {
        wss.iter().find(|ws| {
            ws.get("focused").and_then(|v| v.as_bool()).unwrap_or(false)
        })
    });

    let ws_idx = focused_ws
        .and_then(|ws| ws.get("idx").and_then(|v| v.as_i64()))
        .unwrap_or(1);
    let ws_name = focused_ws
        .and_then(|ws| ws.get("name").and_then(|v| v.as_str()))
        .unwrap_or("");

    let label = if !ws_name.is_empty() && ws_name != ws_idx.to_string() {
        ws_name.to_string()
    } else {
        format!("workspace {ws_idx}")
    };

    pill(vec![Elem::text(label).fg(pal.active)], bg, pc).path("workspace")
}

/// Build window title pill for the center zone (fades with location age).
fn title_pill(
    states: &HashMap<String, ModuleState>,
    pal: Palette,
    location_age: Duration,
    bg: Rgba,
    pc: &PillCfg,
) -> Option<Span> {
    let win_data = states.get("window").map(|s| &s.data);
    let title = win_data
        .and_then(|d| d.get("title").and_then(|v| v.as_str()))
        .unwrap_or("");

    if title.is_empty() {
        return None;
    }

    let t = location_opacity(location_age);
    let bright = pal.active;
    let dim = pal.idle;
    let fg = Rgba::new(
        bright.r,
        bright.g,
        bright.b,
        (dim.a as f32 + (bright.a as f32 - dim.a as f32) * t) as u8,
    );

    Some(pill(vec![Elem::text(title.to_string()).fg(fg)], bg, pc))
}

pub(crate) fn root_content(
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    pal: Palette,
    output_name: Option<&str>,
    badge_overrides: &HashMap<String, RegistrationToken>,
    toasts: &[Toast],
    location_age: Duration,
    gap: f32,
    bg: Rgba,
    pc: &PillCfg,
    ws_rename: Option<&str>,
) -> BarContent {
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

    // Left: launcher icon + workspace pill
    let launcher_icon = template_engine.render_icon("terminal");
    let left = vec![
        pill(vec![Elem::text(launcher_icon).fg(pal.selected)], bg, pc).path("launcher"),
        workspace_pill(&states_ref, pal, bg, pc, ws_rename),
    ];

    // Center: toasts override location indicator
    let mut center = Vec::new();
    let has_toasts = toasts.iter().any(|t| t.paused_remaining.is_none());
    if has_toasts {
        let toast_fg = Rgba::new(pal.active.r, pal.active.g, pal.active.b,
            (pal.active.a as f32 * 0.85) as u8);
        for t in toasts {
            if t.paused_remaining.is_some() { continue; }
            let opacity = crate::bar::toast_opacity(t);
            if !t.elems.is_empty() {
                let elems: Vec<Elem> = t.elems.iter().cloned().map(|e| {
                    if e.fg == Rgba::default() { e.fg(toast_fg) } else { e }
                }).collect();
                center.push(pill_bright(elems, bg, pc).opacity(opacity));
            } else {
                let mut elem = Elem::text(t.text.clone()).fg(toast_fg);
                if let Some(ref pm) = t.icon_pixmap {
                    elem = elem.icon(pm.clone());
                }
                center.push(pill_bright(vec![elem], bg, pc).opacity(opacity));
            }
        }
    } else {
        // Window title as default center
        if let Some(tp) = title_pill(&states_ref, pal, location_age, bg, pc) {
            center.push(tp);
        }
    }

    // Right: alert badges + per-app notification icons + clock
    let mut right = Vec::new();
    let mut mod_ids: Vec<&String> = config.bar.modules.keys().collect();
    mod_ids.sort();
    for id in mod_ids {
        match id.as_str() {
            "calendar" | "window" | "workspaces" | "notifications" => continue,
            _ => {
                for elem in render_badges(id) {
                    right.push(pill(vec![elem], bg, pc));
                }
            }
        }
    }

    // Per-app notification icons (replace bell badge)
    let store = crate::notifications::STORE.lock().unwrap();
    let groups = store.by_app();
    drop(store);
    for group in &groups {
        if group.unread == 0 { continue; }
        let fg = if group.muted { pal.idle } else { pal.active };
        let mut elems = Vec::new();
        if let Some(ref pm) = group.icon_pixmap {
            elems.push(Elem::text(String::new()).icon(pm.clone()).fg(fg));
        } else {
            let icon = template_engine.render_icon("bell");
            elems.push(Elem::text(icon).fg(fg));
        }
        if group.unread > 1 {
            if let Some(last) = elems.last_mut() {
                last.text = group.unread.to_string();
                last.font_scale = 0.7;
                last.y_offset = -4.0;
            }
        }
        let path = format!("__notif_app:{}", group.app_name);
        right.push(pill(elems, bg, pc).path(&path));
    }

    let clock_elems = render_badges("calendar");
    if !clock_elems.is_empty() {
        right.push(pill(clock_elems, bg, pc).path("calendar"));
    }

    drop(states_ref);
    BarContent { left, center, right, gap, left_w: None, right_w: None }
}

pub(crate) fn mod_content(
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
) -> Option<BarContent> {
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
        let center: Vec<Span> = items.into_iter().enumerate().map(|(i, elems)| {
            if cursor == Some(i) {
                pill_bright(elems, bg, pc)
            } else {
                pill(elems, bg, pc)
            }
        }).collect();

        let hint_strs: Vec<String> = deep.key_hints().iter().map(|hint| {
            if hint.label.is_empty() {
                format!("[{}] {}", hint.key, hint.action)
            } else {
                format!("[{}] {}", hint.key, hint.label)
            }
        }).collect();
        let hints = vec![Elem::text(hint_strs.join("  ")).fg(pal.idle)];

        let breadcrumb = render_breadcrumb_elems(module, template_engine, pal.active);
        let left = vec![pill(breadcrumb, bg, pc).path("__back")];
        let right = vec![pill(hints, bg, pc)];

        return Some(BarContent { left, center, right, gap, left_w: None, right_w: None });
    }

    // Render the module's widget
    let data = states_ref.get(id)
        .map(|s| &s.data)
        .unwrap_or(&serde_json::Value::Null);

    let mut center = Vec::new();
    if let Some(widget_def) = &module.widget {
        let elems = template_engine.render_widget(id, widget_def, data, output_name);
        for elem in elems {
            center.push(pill(vec![elem.fg(pal.active)], bg, pc));
        }
    }

    if center.is_empty() && module.key_hints.is_empty() {
        return None;
    }

    let breadcrumb = render_breadcrumb_elems(module, template_engine, pal.active);
    drop(states_ref);

    let hint_strs: Vec<String> = module.key_hints.iter().map(|hint| {
        if hint.label.is_empty() {
            format!("[{}] {}", hint.key, hint.action)
        } else {
            format!("[{}] {}", hint.key, hint.label)
        }
    }).collect();
    let hints_text = if hint_strs.is_empty() {
        "[Esc] back".to_string()
    } else {
        hint_strs.join("  ")
    };
    let hints = vec![Elem::text(hints_text).fg(pal.idle)];

    let left = vec![pill(breadcrumb, bg, pc).path("__back")];
    let right = vec![pill(hints, bg, pc)];

    Some(BarContent { left, center, right, gap, left_w: None, right_w: None })
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

pub(crate) fn text_content(
    nav: &mut NavState,
    config: &Config,
    template_engine: &TemplateEngine,
    states: &Rc<RefCell<HashMap<String, ModuleState>>>,
    pal: Palette,
    gap: f32,
    bg: Rgba,
    pc: &PillCfg,
) -> BarContent {
    let q = nav.query.to_lowercase();

    // (display, highlights, icon_elem, description)
    let mut items: Vec<(String, Vec<usize>, Option<Elem>, String)> = Vec::new();

    if nav.stack.is_empty() {
        let mut sorted: Vec<(&String, &ModuleDef)> = config.bar.modules.iter().collect();
        sorted.sort_by(|a, b| a.1.name.cmp(&b.1.name));

        for (_id, child) in &sorted {
            if child.widget.is_none() && child.module_type.is_none() { continue; }
            let display = child.name.to_lowercase();
            let icon_elem = child.icon.as_ref().map(|name| Elem::text(template_engine.render_icon(name)));
            let desc = child.description.clone().unwrap_or_default();
            if q.is_empty() {
                items.push((display, vec![], icon_elem, desc));
            } else if let Some(pos) = display.find(&q) {
                items.push((display, (pos..pos + q.len()).collect(), icon_elem, desc));
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
                            let icon_elem = entry.get("icon")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .and_then(|s| crate::appicon::lookup(s))
                                .map(|pm| Elem::text(String::new()).icon(pm));
                            let comment = entry.get("comment")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            items.push((display, (pos..pos + q.len()).collect(), icon_elem, comment));
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
                        items.push((name.to_string(), vec![], None, String::new()));
                    } else {
                        let lower = name.to_lowercase();
                        if let Some(pos) = lower.find(&q) {
                            items.push((name.to_string(), (pos..pos + q.len()).collect(), None, String::new()));
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
    let center_spans: Vec<Span> = items.iter().enumerate().map(|(i, (text, _, icon, _))| {
        let is_sel = i == selected;
        let item_fg = if is_sel { pal.selected } else { pal.idle };
        let mut elems = Vec::new();
        if let Some(icon_elem) = icon {
            elems.push(icon_elem.clone().fg(item_fg));
        }
        elems.push(Elem::text(text.clone()).fg(item_fg));
        if is_sel {
            pill_bright(elems, bg, pc)
        } else {
            pill(elems, bg, pc)
        }
    }).collect();

    // Right: description of selected item, or count
    let right_text = items.get(selected)
        .map(|(_, _, _, desc)| desc.as_str())
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .unwrap_or_else(|| format!("{}/{}", items.len(), total));
    let right = vec![pill(vec![Elem::text(right_text).fg(pal.idle)], bg, pc)];

    BarContent { left: left_spans, center: center_spans, right, gap, left_w: None, right_w: None }
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