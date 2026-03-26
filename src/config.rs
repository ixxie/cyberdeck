use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::color::Rgba;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub settings: Settings,
    pub bar: BarDef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Settings {
    #[serde(default = "default_position")]
    pub position: Position,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_padding")]
    pub padding: f32,
    #[serde(default)]
    pub padding_horizontal: Option<f32>,
    #[serde(default)]
    pub padding_left: Option<f32>,
    #[serde(default)]
    pub padding_right: Option<f32>,
    #[serde(default)]
    pub background: Background,
    pub icons_dir: Option<String>,
    #[serde(default = "default_icon_weight")]
    pub icon_weight: String,
    #[serde(default)]
    pub output_scales: HashMap<String, f32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Top,
    Bottom,
}

#[derive(Debug, Deserialize)]
pub struct Background {
    #[serde(default = "default_bg_color")]
    pub color: Rgba,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BarDef {
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub modules: HashMap<String, ModuleDef>,
}

// --- Module definition (flat, no nesting) ---

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ModuleDef {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub source: Option<SourceDef>,
    #[serde(default)]
    pub badges: HashMap<String, BadgeDef>,
    pub widget: Option<WidgetDef>,
    pub label: Option<LabelDef>,
    #[serde(default)]
    pub hooks: Vec<HookDef>,
    #[serde(default)]
    pub key_hints: Vec<KeyHintDef>,
    #[serde(default, rename = "type")]
    pub module_type: Option<String>,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub commands: HashMap<String, String>,
}

impl ModuleDef {
    pub fn has_view(&self) -> bool {
        self.widget.is_some() || self.module_type.is_some() || !self.key_hints.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BadgeDef {
    pub template: String,
    pub condition: Option<String>,
    pub highlight: Option<String>,
}

// --- Hooks ---

#[derive(Debug, Deserialize, Serialize)]
pub struct HookDef {
    pub condition: String,
    pub action: String,
    #[serde(default = "default_hook_timeout")]
    pub timeout: u64,
}

// --- Key hints ---

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct KeyHintDef {
    pub key: String,
    pub action: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LabelDef {
    pub template: String,
}

// --- Source ---

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceDef {
    Poll {
        command: Vec<String>,
        #[serde(default = "default_interval")]
        interval: u64,
    },
    Subscribe {
        command: Vec<String>,
    },
    File {
        paths: Vec<String>,
        #[serde(default = "default_interval")]
        interval: u64,
    },
    Native {
        kind: String,
        #[serde(default = "default_interval")]
        interval: u64,
    },
}

// --- Widget ---

#[derive(Debug, Deserialize, Serialize)]
pub struct WidgetDef {
    pub template: String,
    pub condition: Option<String>,
}

// --- Runtime config (sparse JSON from distro/user) ---

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct RuntimeConfig {
    #[serde(default)]
    settings: RuntimeSettings,
    #[serde(default)]
    bar: RuntimeBarDef,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct RuntimeSettings {
    #[serde(default = "default_position")]
    position: Position,
    #[serde(default = "default_font")]
    font: String,
    #[serde(default = "default_font_size")]
    font_size: f32,
    #[serde(default = "default_padding")]
    padding: f32,
    #[serde(default)]
    padding_horizontal: Option<f32>,
    #[serde(default)]
    padding_left: Option<f32>,
    #[serde(default)]
    padding_right: Option<f32>,
    #[serde(default)]
    background: Background,
    icons_dir: Option<String>,
    #[serde(default = "default_icon_weight")]
    icon_weight: String,
    #[serde(default)]
    output_scales: HashMap<String, f32>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct RuntimeBarDef {
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    modules: HashMap<String, serde_json::Value>,
}

// --- Loading ---

impl Config {
    pub fn load(path: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let builtins = crate::modlib::builtin_modules();

        let path = match path {
            Some(p) => PathBuf::from(p),
            None => {
                let xdg = std::env::var("XDG_CONFIG_HOME")
                    .unwrap_or_else(|_| {
                        let home = std::env::var("HOME").unwrap_or_default();
                        format!("{home}/.config")
                    });
                PathBuf::from(xdg).join("cyberdeck/config.json")
            }
        };

        let data = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read config at {}: {e}", path.display()))?;

        // Detect old-style full config (modules have "name" field) vs new sparse config
        let raw: serde_json::Value = serde_json::from_str(&data)?;
        let is_legacy = raw.get("bar")
            .and_then(|b| b.get("modules"))
            .and_then(|m| m.as_object())
            .map(|mods| mods.values().any(|v| v.get("name").is_some()))
            .unwrap_or(false);

        let config = if is_legacy {
            // Old format: full module definitions in JSON, ignore builtins
            serde_json::from_value::<Config>(raw)?
        } else {
            // New format: sparse overrides merged with builtins
            let runtime: RuntimeConfig = serde_json::from_value(raw)?;
            let modules = Self::merge_modules(builtins, &runtime.bar.order, runtime.bar.modules);
            Config {
                settings: Settings {
                    position: runtime.settings.position,
                    font: runtime.settings.font,
                    font_size: runtime.settings.font_size,
                    padding: runtime.settings.padding,
                    padding_horizontal: runtime.settings.padding_horizontal,
                    padding_left: runtime.settings.padding_left,
                    padding_right: runtime.settings.padding_right,
                    background: runtime.settings.background,
                    icons_dir: runtime.settings.icons_dir,
                    icon_weight: runtime.settings.icon_weight,
                    output_scales: runtime.settings.output_scales,
                },
                bar: BarDef {
                    order: runtime.bar.order,
                    modules,
                },
            }
        };

        config.write_params();
        Ok(config)
    }

    fn merge_modules(
        builtins: HashMap<String, ModuleDef>,
        order: &[String],
        overrides: HashMap<String, serde_json::Value>,
    ) -> HashMap<String, ModuleDef> {
        let mut result = HashMap::new();

        for id in order {
            let base = builtins.get(id);
            let ov = overrides.get(id);

            let def = match (base, ov) {
                (Some(base_def), Some(override_val)) => {
                    // Merge: serialize base to Value, deep-merge override, deserialize back
                    let mut base_val = serde_json::to_value(base_def)
                        .expect("failed to serialize builtin module");
                    json_merge(&mut base_val, override_val.clone());
                    serde_json::from_value(base_val).unwrap_or_else(|e| {
                        log::error!("failed to merge module {id}: {e}");
                        // Fall back to just the builtin
                        builtins.get(id).map(|b| {
                            serde_json::from_value(serde_json::to_value(b).unwrap()).unwrap()
                        }).unwrap_or_else(|| serde_json::from_value(override_val.clone()).unwrap())
                    })
                }
                (Some(base_def), None) => {
                    // No override, reserialize the builtin
                    let val = serde_json::to_value(base_def)
                        .expect("failed to serialize builtin module");
                    serde_json::from_value(val).expect("failed to deserialize builtin module")
                }
                (None, Some(override_val)) => {
                    // User-defined module not in builtins
                    serde_json::from_value(override_val.clone()).unwrap_or_else(|e| {
                        log::error!("failed to parse user module {id}: {e}");
                        return ModuleDef {
                            name: id.clone(),
                            ..Default::default()
                        };
                    })
                }
                (None, None) => {
                    log::warn!("module {id} in order but not found in builtins or config");
                    continue;
                }
            };

            result.insert(id.clone(), def);
        }

        result
    }

    fn params_dir() -> PathBuf {
        let cache = std::env::var("XDG_CACHE_HOME")
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                format!("{home}/.cache")
            });
        PathBuf::from(cache).join("cyberdeck/params")
    }

    fn write_params(&self) {
        let base = Self::params_dir();
        for (id, module) in &self.bar.modules {
            if !module.params.is_empty() {
                Self::write_params_file(&base, id, &module.params);
            }
        }
    }

    fn write_params_file(base: &PathBuf, mod_path: &str, params: &HashMap<String, serde_json::Value>) {
        let dir = base.join(mod_path.replace('.', "/"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::error!("failed to create params dir {}: {e}", dir.display());
            return;
        }
        let file = dir.join("params.json");
        match serde_json::to_string_pretty(params) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&file, json) {
                    log::error!("failed to write params {}: {e}", file.display());
                }
            }
            Err(e) => log::error!("failed to serialize params for {mod_path}: {e}"),
        }
    }
}

impl Default for ModuleDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            icon: None,
            source: None,
            badges: HashMap::new(),
            widget: None,
            label: None,
            hooks: Vec::new(),
            key_hints: Vec::new(),
            module_type: None,
            params: HashMap::new(),
            commands: HashMap::new(),
        }
    }
}

/// Recursively merge `override_val` into `base`. Objects are deep-merged;
/// all other types are replaced.
fn json_merge(base: &mut serde_json::Value, override_val: serde_json::Value) {
    match (base, override_val) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(ov_map)) => {
            for (key, val) in ov_map {
                let entry = base_map.entry(key).or_insert(serde_json::Value::Null);
                json_merge(entry, val);
            }
        }
        (base, ov) => {
            *base = ov;
        }
    }
}

// --- Defaults ---

fn default_position() -> Position { Position::Top }
fn default_font() -> String { "monospace".into() }
fn default_font_size() -> f32 { 14.0 }
fn default_bg_color() -> Rgba { Rgba::new(0x22, 0x22, 0x22, 255) }
fn default_opacity() -> f32 { 0.8 }
fn default_padding() -> f32 { 6.0 }
fn default_interval() -> u64 { 5 }
fn default_icon_weight() -> String { "light".into() }
fn default_hook_timeout() -> u64 { 5 }

impl Default for Background {
    fn default() -> Self {
        Self {
            color: default_bg_color(),
            opacity: default_opacity(),
        }
    }
}

impl Default for Position {
    fn default() -> Self {
        default_position()
    }
}
