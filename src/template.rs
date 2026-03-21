use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tera::{Context, Tera, Value};

use crate::config::{BarDef, WidgetDef};
use crate::layout::RenderedWidget;

pub struct TemplateEngine {
    tera: Tera,
    event_ctx: Arc<Mutex<(serde_json::Value, serde_json::Value)>>,
    icon_map: HashMap<String, String>,
}

impl TemplateEngine {
    pub fn new(root: &BarDef) -> Self {
        let mut tera = Tera::default();

        // Register templates for all modules
        for (id, module) in &root.modules {
            // Widget
            if let Some(w) = &module.widget {
                let name = format!("{id}.widget");
                tera.add_raw_template(&name, &w.template)
                    .unwrap_or_else(|e| log::error!("bad template {name}: {e}"));

                if let Some(cond) = &w.condition {
                    let cond_name = format!("{id}.widget.__cond");
                    tera.add_raw_template(&cond_name, cond)
                        .unwrap_or_else(|e| log::error!("bad condition template {cond_name}: {e}"));
                }
            }

            for (badge_name, badge) in &module.badges {
                let name = format!("{id}.badge.{badge_name}");
                tera.add_raw_template(&name, &badge.template)
                    .unwrap_or_else(|e| log::error!("bad badge template {name}: {e}"));
                if let Some(cond) = &badge.condition {
                    let cond_name = format!("{id}.badge.{badge_name}.__cond");
                    tera.add_raw_template(&cond_name, cond)
                        .unwrap_or_else(|e| log::error!("bad badge condition {cond_name}: {e}"));
                }
                if let Some(hl) = &badge.highlight {
                    let hl_name = format!("{id}.badge.{badge_name}.__highlight");
                    tera.add_raw_template(&hl_name, hl)
                        .unwrap_or_else(|e| log::error!("bad badge highlight {hl_name}: {e}"));
                }
            }

            // Label
            if let Some(label) = &module.label {
                let name = format!("{id}.label");
                tera.add_raw_template(&name, &label.template)
                    .unwrap_or_else(|e| log::error!("bad label template {name}: {e}"));
            }

            // Hook conditions
            for (i, hook) in module.hooks.iter().enumerate() {
                let name = format!("{id}.__hook.{i}.__cond");
                tera.add_raw_template(&name, &hook.condition)
                    .unwrap_or_else(|e| log::error!("bad hook condition {name}: {e}"));
            }
        }

        // Parse icon map once, clone for closures
        let icon_map: HashMap<String, String> = {
            let json: HashMap<String, String> =
                serde_json::from_str(include_str!("../assets/icons.json")).unwrap_or_default();
            json.into_iter().filter_map(|(name, cp)| {
                u32::from_str_radix(
                    cp.trim_start_matches("0x").trim_start_matches("U+"), 16
                ).ok()
                .and_then(char::from_u32)
                .map(|ch| (name, ch.to_string()))
            }).collect()
        };

        let filter_map = icon_map.clone();
        tera.register_filter("icon", move |value: &Value, _args: &HashMap<String, Value>| {
            let name = value.as_str().unwrap_or("");
            Ok(Value::String(
                filter_map.get(name).cloned().unwrap_or_else(|| format!("[{name}]"))
            ))
        });

        let fn_map = icon_map.clone();
        tera.register_function("icon", move |args: &HashMap<String, Value>| {
            let name = args.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Ok(Value::String(
                fn_map.get(name).cloned().unwrap_or_else(|| format!("[{name}]"))
            ))
        });

        tera.register_filter("meter", |value: &Value, args: &HashMap<String, Value>| {
            let val = value.as_f64().unwrap_or(0.0);
            let max = args.get("max").and_then(|v| v.as_f64()).unwrap_or(100.0);
            let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            let ratio = if max > 0.0 { (val / max).clamp(0.0, 1.0) } else { 0.0 };
            let filled = (ratio * width as f64).round() as usize;
            let empty = width.saturating_sub(filled);
            let sep = "\u{2009}";
            let block = "\u{2588}";
            let mut result = String::new();
            for i in 0..filled {
                if i > 0 { result.push_str(sep); }
                result.push_str(block);
            }
            if empty > 0 {
                if filled > 0 { result.push_str(sep); }
                result.push('\x01');
                for i in 0..empty {
                    if i > 0 { result.push_str(sep); }
                    result.push_str(block);
                }
                result.push('\x02');
            }
            Ok(Value::String(result))
        });

        tera.register_filter("bar", |value: &Value, args: &HashMap<String, Value>| {
            let val = value.as_f64().unwrap_or(0.0);
            let max = args.get("max").and_then(|v| v.as_f64()).unwrap_or(100.0);
            let ratio = if max > 0.0 { (val / max).clamp(0.0, 1.0) } else { 0.0 };
            let blocks = [
                '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}',
                '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
            ];
            let idx = ((ratio * 7.0).round() as usize).min(7);
            Ok(Value::String(blocks[idx].to_string()))
        });

        tera.register_filter("human_bytes", |value: &Value, _args: &HashMap<String, Value>| {
            let bytes = value.as_f64().unwrap_or(0.0);
            let s = if bytes >= 1e12 {
                format!("{:.1}T", bytes / 1e12)
            } else if bytes >= 1e9 {
                format!("{:.1}G", bytes / 1e9)
            } else if bytes >= 1e6 {
                format!("{:.1}M", bytes / 1e6)
            } else if bytes >= 1e3 {
                format!("{:.1}K", bytes / 1e3)
            } else {
                format!("{}B", bytes as u64)
            };
            Ok(Value::String(s))
        });

        tera.register_filter("human_duration", |value: &Value, _args: &HashMap<String, Value>| {
            let secs = value.as_u64()
                .or_else(|| value.as_f64().map(|f| f as u64))
                .unwrap_or(0);
            let s = if secs >= 86400 {
                format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
            } else if secs >= 3600 {
                format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
            } else if secs >= 60 {
                format!("{}m {}s", secs / 60, secs % 60)
            } else {
                format!("{}s", secs)
            };
            Ok(Value::String(s))
        });

        tera.register_filter("pad_left", |value: &Value, args: &HashMap<String, Value>| {
            let s = match value {
                Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let chars = s.chars().count();
            if chars >= width {
                Ok(Value::String(s))
            } else {
                Ok(Value::String(format!("{}{}", " ".repeat(width - chars), s)))
            }
        });

        tera.register_filter("pad_right", |value: &Value, args: &HashMap<String, Value>| {
            let s = match value {
                Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            let width = args.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let chars = s.chars().count();
            if chars >= width {
                Ok(Value::String(s))
            } else {
                Ok(Value::String(format!("{}{}", s, " ".repeat(width - chars))))
            }
        });

        tera.register_filter("color", |value: &Value, _args: &HashMap<String, Value>| {
            Ok(value.clone())
        });

        tera.register_filter("dim", |value: &Value, _args: &HashMap<String, Value>| {
            let s = value.as_str().unwrap_or("");
            Ok(Value::String(format!("\x01{s}\x02")))
        });

        let event_ctx: Arc<Mutex<(serde_json::Value, serde_json::Value)>> =
            Arc::new(Mutex::new((serde_json::Value::Null, serde_json::Value::Null)));
        let ctx_ref = event_ctx.clone();
        tera.register_function("changed", move |args: &HashMap<String, Value>| {
            let key = args.get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| tera::Error::msg("changed() requires 'key' argument"))?;
            let guard = ctx_ref.lock().unwrap();
            let (ref current, ref prev) = *guard;
            Ok(Value::Bool(current.get(key) != prev.get(key)))
        });

        Self { tera, event_ctx, icon_map }
    }

    pub fn render_icon(&self, name: &str) -> String {
        self.icon_map.get(name).cloned().unwrap_or_else(|| format!("[{name}]"))
    }

    pub fn render_badge(
        &self,
        mod_id: &str,
        badge_name: &str,
        badge: &crate::config::BadgeDef,
        data: &serde_json::Value,
        output_name: Option<&str>,
        highlighted: bool,
    ) -> Option<RenderedWidget> {
        let tpl_path = format!("{mod_id}.badge.{badge_name}");

        // Skip condition check when highlighted (override active)
        if !highlighted {
            if badge.condition.is_some() {
                let cond_name = format!("{tpl_path}.__cond");
                let mut ctx = Context::from_value(data.clone())
                    .unwrap_or_default();
                ctx.insert("__output", &output_name.unwrap_or(""));
                let rendered = self.tera.render(&cond_name, &ctx).unwrap_or_default();
                let trimmed = rendered.trim();
                if trimmed.is_empty() || trimmed == "false" || trimmed == "0" {
                    return None;
                }
            }
        }

        let name = if highlighted && badge.highlight.is_some() {
            format!("{tpl_path}.__highlight")
        } else {
            tpl_path
        };
        let mut ctx = Context::from_value(data.clone())
            .unwrap_or_default();
        ctx.insert("__output", &output_name.unwrap_or(""));
        match self.tera.render(&name, &ctx) {
            Ok(text) if !text.trim().is_empty() => {
                let mut rw = RenderedWidget::new(text.trim().to_string());
                if let Some(s) = badge.icon_scale {
                    rw = rw.with_icon_scale(s);
                }
                Some(rw)
            }
            Ok(_) => None,
            Err(e) => {
                log::error!("badge render error {name}: {e}");
                None
            }
        }
    }

    pub fn eval_hook_condition(
        &self,
        path: &str,
        hook_idx: usize,
        data: &serde_json::Value,
    ) -> bool {
        let name = format!("{path}.__hook.{hook_idx}.__cond");
        let ctx = Context::from_value(data.clone()).unwrap_or_default();
        let rendered = self.tera.render(&name, &ctx).unwrap_or_default();
        let trimmed = rendered.trim();
        !trimmed.is_empty() && trimmed != "false" && trimmed != "0"
    }

    pub fn render_widget(
        &self,
        path: &str,
        widget: &WidgetDef,
        data: &serde_json::Value,
        output_name: Option<&str>,
    ) -> Option<RenderedWidget> {
        if widget.condition.is_some() {
            let cond_name = format!("{path}.widget.__cond");
            let mut ctx = Context::from_value(data.clone()).unwrap_or_default();
            ctx.insert("__output", &output_name.unwrap_or(""));
            let rendered = self.tera.render(&cond_name, &ctx).unwrap_or_default();
            let trimmed = rendered.trim();
            if trimmed.is_empty() || trimmed == "false" || trimmed == "0" {
                return None;
            }
        }

        let name = format!("{path}.widget");
        let mut ctx = Context::from_value(data.clone()).unwrap_or_default();
        ctx.insert("__output", &output_name.unwrap_or(""));
        match self.tera.render(&name, &ctx) {
            Ok(text) => Some(RenderedWidget::new(text)),
            Err(e) => {
                log::error!("template render error {name}: {e}");
                None
            }
        }
    }

    pub fn set_event_context(
        &self,
        current: &serde_json::Value,
        prev: &serde_json::Value,
    ) {
        *self.event_ctx.lock().unwrap() = (current.clone(), prev.clone());
    }

}
