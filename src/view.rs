use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use smithay_client_toolkit::reexports::calloop::RegistrationToken;
use tiny_skia::Pixmap;

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

const PILL_R: f32 = 6.0;
const PILL_PAD: f32 = 6.0;

fn pill(elems: Vec<Elem>, bg: Rgba) -> Span {
    Span::new(elems).bg(bg).radius(PILL_R).pad(PILL_PAD)
}

fn pill_bright(elems: Vec<Elem>, bg: Rgba) -> Span {
    let bright = Rgba::new(bg.r, bg.g, bg.b, (bg.a as f32 * 1.3).min(255.0) as u8);
    Span::new(elems).bg(bright).radius(PILL_R).pad(PILL_PAD)
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
) -> Vec<Span> {
    if spans.is_empty() { return spans; }

    let total_w: f32 = spans.iter().map(|s| m.span_w(s)).sum::<f32>()
        + (spans.len().saturating_sub(1)) as f32 * gap;
    if total_w <= available_w {
        *scroll = 0;
        return spans;
    }

    let left_caret = pill(vec![Elem::text(template_engine.render_icon("caret-left")).fg(caret_fg)], bg)
        .path("__scroll_left");
    let right_caret = pill(vec![Elem::text(template_engine.render_icon("caret-right")).fg(caret_fg)], bg)
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
    icon_h: u32,
    _bar_w: f32,
    _m: &Metrics,
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

    // Left: launcher icon
    let mut left_spans = Vec::new();
    let launcher_icon = template_engine.render_icon("terminal");
    left_spans.push(pill(vec![Elem::text(launcher_icon).fg(pal.selected)], bg).path("launcher"));

    // Center: minimap + window title in one pill
    let mut center_elems = Vec::new();

    if let Some(ws_data) = states_ref.get("workspaces").map(|s| &s.data) {
        let workspaces = ws_data.get("workspaces").and_then(|v| v.as_array());
        let windows = ws_data.get("windows").and_then(|v| v.as_array());
        if let (Some(wss), Some(wins)) = (workspaces, windows) {
            let output = output_name.unwrap_or("");
            if let Some(pm) = render_minimap(wss, wins, output, icon_h) {
                center_elems.push(Elem::text("").fg(pal.selected).icon(Arc::new(pm)));
            }
        }
    }

    center_elems.extend(render_badges("window"));

    // Hide center when toasts are active to avoid overlap
    let toast_presence: f32 = toasts.iter()
        .map(|t| crate::bar::toast_opacity(t))
        .fold(0.0f32, f32::max);

    let mut center_spans = Vec::new();
    if !center_elems.is_empty() && toast_presence < 0.01 {
        center_spans.push(pill(center_elems, bg).path("overview"));
    }

    // Right: toasts + alert badges + clock
    let mut right_spans = Vec::new();

    let toast_fg = Rgba::new(pal.active.r, pal.active.g, pal.active.b,
        (pal.active.a as f32 * 0.85) as u8);
    for t in toasts {
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
        let opacity = crate::bar::toast_opacity(t);
        right_spans.push(pill_bright(vec![elem], bg).opacity(opacity));
    }

    for id in &config.bar.order {
        match id.as_str() {
            "calendar" | "window" | "workspaces" => continue,
            _ => {
                let badge_elems = render_badges(id);
                for elem in badge_elems {
                    right_spans.push(pill(vec![elem], bg));
                }
            }
        }
    }

    // Clock (calendar badges)
    let clock_elems = render_badges("calendar");
    if !clock_elems.is_empty() {
        right_spans.push(pill(clock_elems, bg).path("calendar"));
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

        let center_elems = deep.render_center(pal.selected, &data);
        let center_span = pill(center_elems, bg);

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
            Zone::left(vec![pill(breadcrumb, bg).path("__back")], gap),
            Zone::center(vec![center_span], gap),
            Zone::right(vec![pill(hints, bg)], gap),
        ]);
    }

    // Render the module's widget
    let data = states_ref.get(id)
        .map(|s| &s.data)
        .unwrap_or(&serde_json::Value::Null);

    let mut content = Vec::new();
    if let Some(widget_def) = &module.widget {
        if let Some(elem) = template_engine.render_widget(id, widget_def, data, output_name) {
            content.push(elem.fg(pal.active));
        }
    }

    // Show module view if has widget OR has key-hints
    if content.is_empty() && module.key_hints.is_empty() {
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
        Zone::left(vec![pill(breadcrumb, bg).path("__back")], gap),
        Zone::center(vec![pill(content, bg)], gap),
        Zone::right(vec![pill(hints, bg)], gap),
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
    let left_spans = vec![pill(left_elems, bg)];

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
            pill_bright(elems, bg)
        } else {
            pill(elems, bg)
        }
    }).collect();

    // Right: count
    let count_text = format!("{}/{}", items.len(), total);
    let right_elems = vec![Elem::text(count_text).fg(pal.idle)];
    let right_spans = vec![pill(right_elems, bg)];

    // Apply windowing to center spans
    let left_w: f32 = left_spans.iter().map(|s| m.span_w(s)).sum::<f32>()
        + left_spans.len().saturating_sub(1) as f32 * gap;
    let right_w: f32 = right_spans.iter().map(|s| m.span_w(s)).sum::<f32>()
        + right_spans.len().saturating_sub(1) as f32 * gap;
    let center_avail = bar_w - left_w - right_w - 2.0 * gap;
    let center_spans = window_spans(
        center_spans, nav.selected, &mut nav.scroll,
        center_avail, m, template_engine, gap, bg, pal.idle,
    );

    vec![
        Zone::left(left_spans, gap),
        Zone::center(center_spans, gap),
        Zone::right(right_spans, gap),
    ]
}

/// Render workspace minimap: stacked rows, each showing window tile layout
/// Focused workspace is brighter, focused window is highlighted
fn render_minimap(
    workspaces: &[serde_json::Value],
    windows: &[serde_json::Value],
    output: &str,
    icon_h: u32,
) -> Option<Pixmap> {
    if icon_h == 0 { return None; }

    // Filter workspaces for this output
    let wss: Vec<&serde_json::Value> = workspaces.iter()
        .filter(|ws| ws.get("output").and_then(|v| v.as_str()).unwrap_or("") == output)
        .collect();
    if wss.is_empty() { return None; }

    let n = wss.len();
    let ws_gap = (icon_h as f32 * 0.15).max(2.0).ceil() as u32;
    let total_gap = (n.saturating_sub(1) as u32) * ws_gap;
    let row_h = ((icon_h.saturating_sub(total_gap)) as f32 / n as f32).floor().max(2.0) as u32;
    let map_w = (icon_h as f32 * 1.6).ceil() as u32;
    let tile_gap = 2u32;

    let total_h = row_h * n as u32 + total_gap;
    let y_offset = (icon_h.saturating_sub(total_h)) / 2;

    let mut pm = Pixmap::new(map_w, icon_h)?;
    let mut y = y_offset;

    for ws in &wss {
        let ws_id = ws.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let ws_focused = ws.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
        let ws_active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);

        let ws_wins: Vec<&serde_json::Value> = windows.iter()
            .filter(|w| w.get("workspace_id").and_then(|v| v.as_i64()).unwrap_or(-1) == ws_id)
            .collect();

        let ws_alpha = if ws_focused { 200u8 } else if ws_active { 140u8 } else { 60u8 };

        if ws_wins.is_empty() {
            // Empty workspace: thin line spanning full width like populated ones
            let line_x = 0u32;
            let line_h = 1u32;
            let line_y = y + (row_h.saturating_sub(line_h)) / 2;
            fill_rounded_rect(pm.data_mut(), map_w, icon_h, line_x, line_y, map_w, line_h, 0.0, ws_alpha / 3);
        } else {
            let max_col = ws_wins.iter()
                .map(|w| w.get("col").and_then(|v| v.as_i64()).unwrap_or(1))
                .max()
                .unwrap_or(1);

            let mut col_widths: Vec<f64> = vec![0.0; max_col as usize];
            for w in &ws_wins {
                let col = (w.get("col").and_then(|v| v.as_i64()).unwrap_or(1) - 1) as usize;
                let tw = w.get("w").and_then(|v| v.as_f64()).unwrap_or(400.0);
                if col < col_widths.len() {
                    col_widths[col] = col_widths[col].max(tw);
                }
            }
            let total_w: f64 = col_widths.iter().sum();
            if total_w <= 0.0 { y += row_h + ws_gap; continue; }

            let usable_w = map_w.saturating_sub(tile_gap * (max_col as u32).saturating_sub(1));
            let scale = usable_w as f64 / total_w;

            let mut col_x = Vec::with_capacity(col_widths.len());
            let mut cx = 0u32;
            for (i, &cw) in col_widths.iter().enumerate() {
                col_x.push(cx);
                cx += (cw * scale).round() as u32;
                if i < col_widths.len() - 1 {
                    cx += tile_gap;
                }
            }

            for col_idx in 0..max_col as usize {
                let mut col_wins: Vec<&&serde_json::Value> = ws_wins.iter()
                    .filter(|w| (w.get("col").and_then(|v| v.as_i64()).unwrap_or(1) - 1) as usize == col_idx)
                    .collect();
                col_wins.sort_by_key(|w| w.get("row").and_then(|v| v.as_i64()).unwrap_or(1));

                let tile_w = (col_widths[col_idx] * scale).round() as u32;
                let tx = col_x.get(col_idx).copied().unwrap_or(0);

                let win_heights: Vec<f64> = col_wins.iter()
                    .map(|w| w.get("h").and_then(|v| v.as_f64()).unwrap_or(300.0))
                    .collect();
                let total_h: f64 = win_heights.iter().sum();
                let n_rows = col_wins.len();
                let usable_h = row_h.saturating_sub(tile_gap * n_rows.saturating_sub(1) as u32);
                let h_scale = if total_h > 0.0 { usable_h as f64 / total_h } else { 1.0 };

                let mut ty = y;
                for (ri, w) in col_wins.iter().enumerate() {
                    let focused = w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
                    let tile_h = (win_heights[ri] * h_scale).round().max(1.0) as u32;
                    if ri > 0 { ty += tile_gap; }
                    let alpha = if focused { 255u8 } else { ws_alpha };
                    let corner_r = (tile_h.min(tile_w) as f32 * 0.25).max(1.0);
                    fill_rounded_rect(pm.data_mut(), map_w, icon_h, tx, ty, tile_w, tile_h, corner_r, alpha);
                    ty += tile_h;
                }
            }
        }

        y += row_h + ws_gap;
    }

    Some(pm)
}

fn fill_rounded_rect(data: &mut [u8], buf_w: u32, buf_h: u32, x: u32, y: u32, w: u32, h: u32, r: f32, alpha: u8) {
    let r = r.min(w as f32 / 2.0).min(h as f32 / 2.0);
    for py in y..y + h {
        if py >= buf_h { break; }
        for px in x..x + w {
            if px >= buf_w { break; }
            let lx = px - x;
            let ly = py - y;

            // Check corners
            let in_corner = if lx < r as u32 && ly < r as u32 {
                // top-left
                let dx = r - lx as f32 - 0.5;
                let dy = r - ly as f32 - 0.5;
                dx * dx + dy * dy > r * r
            } else if lx >= w - r as u32 && ly < r as u32 {
                // top-right
                let dx = lx as f32 + 0.5 - (w as f32 - r);
                let dy = r - ly as f32 - 0.5;
                dx * dx + dy * dy > r * r
            } else if lx < r as u32 && ly >= h - r as u32 {
                // bottom-left
                let dx = r - lx as f32 - 0.5;
                let dy = ly as f32 + 0.5 - (h as f32 - r);
                dx * dx + dy * dy > r * r
            } else if lx >= w - r as u32 && ly >= h - r as u32 {
                // bottom-right
                let dx = lx as f32 + 0.5 - (w as f32 - r);
                let dy = ly as f32 + 0.5 - (h as f32 - r);
                dx * dx + dy * dy > r * r
            } else {
                false
            };

            if in_corner { continue; }

            let idx = (py * buf_w + px) as usize * 4;
            if idx + 3 < data.len() {
                data[idx] = 255;
                data[idx + 1] = 255;
                data[idx + 2] = 255;
                data[idx + 3] = alpha;
            }
        }
    }
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
