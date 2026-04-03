use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::color::Rgba;

// --- Layout & Theme ---

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Classic,
    Floating,
    #[default]
    Pills,
    Transparent,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ThemeOverride {
    pub color: Option<Rgba>,
    pub opacity: Option<f32>,
    pub radius: Option<f32>,
    pub padding: Option<f32>,
    pub max_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ThemeCfg {
    #[serde(default = "default_color")]
    pub color: Rgba,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_radius")]
    pub radius: f32,
    #[serde(default = "default_padding")]
    pub padding: f32,
    #[serde(default)]
    pub track: Option<ThemeOverride>,
    #[serde(default)]
    pub pill: Option<ThemeOverride>,
}

impl Default for ThemeCfg {
    fn default() -> Self {
        Self {
            color: default_color(),
            opacity: default_opacity(),
            radius: default_radius(),
            padding: default_padding(),
            track: None,
            pill: None,
        }
    }
}

pub struct ResolvedTrack {
    pub color: Rgba,
    pub opacity: f32,
    pub radius: f32,
    pub padding: f32,
}

pub struct ResolvedPill {
    pub color: Rgba,
    pub opacity: f32,
    pub radius: f32,
    pub padding: f32,
    pub max_chars: usize,
}

#[derive(Debug, Deserialize, Default)]
pub struct MonitorCfg {
    pub scale: Option<f32>,
}

// --- Top-level config ---

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
    #[serde(default = "default_emoji_font")]
    pub emoji_font: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub theme: ThemeCfg,
    pub icons_dir: Option<String>,
    #[serde(default = "default_icon_weight")]
    pub icon_weight: String,
    #[serde(default)]
    pub monitors: HashMap<String, MonitorCfg>,
    #[serde(default = "default_true")]
    pub defocus_on_niri_events: bool,
    #[serde(default = "default_true")]
    pub wrap_nav: bool,
}

fn default_true() -> bool { true }

impl Settings {
    pub fn margin(&self) -> f32 {
        match self.layout {
            Layout::Classic => 0.0,
            _ => self.gap,
        }
    }

    pub fn monitor_scale(&self, name: Option<&str>) -> f32 {
        name.and_then(|n| self.monitors.get(n))
            .and_then(|m| m.scale)
            .unwrap_or(self.scale)
    }

    pub fn resolve_track(&self) -> ResolvedTrack {
        let ov = self.theme.track.as_ref();
        let (opacity, padding, radius) = match self.layout {
            Layout::Classic => (1.0, self.gap, 0.0),
            Layout::Floating => (1.0, self.gap, self.theme.radius),
            Layout::Pills | Layout::Transparent => (0.0, 0.0, 0.0),
        };
        ResolvedTrack {
            color: ov.and_then(|o| o.color).unwrap_or(self.theme.color),
            opacity: ov.and_then(|o| o.opacity).unwrap_or(opacity),
            radius: ov.and_then(|o| o.radius).unwrap_or(radius),
            padding: ov.and_then(|o| o.padding).unwrap_or(padding),
        }
    }

    pub fn resolve_pill(&self) -> ResolvedPill {
        let ov = self.theme.pill.as_ref();
        let opacity = match self.layout {
            Layout::Transparent => 0.5,
            _ => 1.0,
        };
        ResolvedPill {
            color: ov.and_then(|o| o.color).unwrap_or(self.theme.color),
            opacity: ov.and_then(|o| o.opacity).unwrap_or(opacity),
            radius: ov.and_then(|o| o.radius).unwrap_or(self.theme.radius),
            padding: ov.and_then(|o| o.padding).unwrap_or(self.theme.padding),
            max_chars: ov.and_then(|o| o.max_chars).unwrap_or(48),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Top,
    Bottom,
}

// --- Bar definition ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BarDef {
    #[serde(default)]
    pub modules: HashMap<String, ModuleDef>,
}

// --- Module definition ---

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ModuleDef {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
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
    #[serde(default, flatten)]
    pub params: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub actions: Vec<ActionDef>,
}

impl ModuleDef {
    pub fn has_view(&self) -> bool {
        self.widget.is_some() || self.module_type.is_some()
            || !self.key_hints.is_empty() || !self.actions.is_empty()
    }

    pub fn action_by_name(&self, name: &str) -> Option<&ActionDef> {
        self.actions.iter().find(|a| a.name == name)
    }

    pub fn action_by_key(&self, key: &str) -> Option<&ActionDef> {
        self.actions.iter().find(|a| a.key.as_deref() == Some(key))
    }
}

// --- Actions ---

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ActionDef {
    pub name: String,
    pub run: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
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
    #[serde(default = "default_emoji_font")]
    emoji_font: String,
    #[serde(default = "default_font_size")]
    font_size: f32,
    #[serde(default)]
    layout: Layout,
    #[serde(default = "default_gap")]
    gap: f32,
    #[serde(default = "default_scale")]
    scale: f32,
    #[serde(default)]
    theme: ThemeCfg,
    icons_dir: Option<String>,
    #[serde(default = "default_icon_weight")]
    icon_weight: String,
    #[serde(default)]
    monitors: HashMap<String, MonitorCfg>,
    #[serde(default = "default_true")]
    defocus_on_niri_events: bool,
    #[serde(default = "default_true")]
    wrap_nav: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
struct RuntimeBarDef {
    #[serde(default)]
    modules: HashMap<String, serde_json::Value>,
}

// --- Loading ---

impl Config {
    pub fn config_dir() -> PathBuf {
        let xdg = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                format!("{home}/.config")
            });
        PathBuf::from(xdg).join("cyberdeck")
    }

    /// Find config file: explicit path, or probe config.toml then config.json
    fn config_path(path: Option<&str>) -> PathBuf {
        if let Some(p) = path {
            return PathBuf::from(p);
        }
        let dir = Self::config_dir();
        let toml = dir.join("config.toml");
        if toml.exists() { return toml; }
        dir.join("config.json")
    }

    fn resolve_icons_dir(configured: Option<String>) -> Option<String> {
        if configured.is_some() { return configured; }
        if let Ok(env) = std::env::var("CYBERDECK_ICONS") {
            if PathBuf::from(&env).is_dir() { return Some(env); }
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            format!("{home}/.local/share/cyberdeck/icons"),
            "/usr/share/cyberdeck/icons".to_string(),
            "/usr/local/share/cyberdeck/icons".to_string(),
        ];
        for c in &candidates {
            if PathBuf::from(c).is_dir() { return Some(c.clone()); }
        }
        None
    }

    const DEFAULT_CONFIG: &str = r#"{
  "settings": {},
  "bar": {
    "modules": {
      "calendar": {},
      "workspaces": {},
      "window": {},
      "notifications": {}
    }
  }
}"#;

    pub fn load(path: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        let builtins = crate::modlib::builtin_modules();
        let path = Self::config_path(path);
        let is_toml = path.extension().map(|e| e == "toml").unwrap_or(false);

        let data = if path.exists() {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("failed to read config at {}: {e}", path.display()))?
        } else {
            log::info!("no config found at {}, using defaults", path.display());
            Self::DEFAULT_CONFIG.to_string()
        };

        let raw: serde_json::Value = if is_toml {
            let toml_val: toml::Value = toml::from_str(&data)
                .map_err(|e| format!("failed to parse TOML config: {e}"))?;
            toml_to_json(toml_val)
        } else {
            serde_json::from_str(&data)?
        };
        let is_legacy = raw.get("bar")
            .and_then(|b| b.get("modules"))
            .and_then(|m| m.as_object())
            .map(|mods| mods.values().any(|v| v.get("name").is_some()))
            .unwrap_or(false);

        let config = if is_legacy {
            serde_json::from_value::<Config>(raw)?
        } else {
            let runtime: RuntimeConfig = serde_json::from_value(raw)?;
            let modules = Self::merge_modules(builtins, runtime.bar.modules);
            Config {
                settings: Settings {
                    position: runtime.settings.position,
                    font: runtime.settings.font,
                    emoji_font: runtime.settings.emoji_font,
                    font_size: runtime.settings.font_size,
                    layout: runtime.settings.layout,
                    gap: runtime.settings.gap,
                    scale: runtime.settings.scale,
                    theme: runtime.settings.theme,
                    icons_dir: runtime.settings.icons_dir,
                    icon_weight: runtime.settings.icon_weight,
                    monitors: runtime.settings.monitors,
                    defocus_on_niri_events: runtime.settings.defocus_on_niri_events,
                    wrap_nav: runtime.settings.wrap_nav,
                },
                bar: BarDef {
                    modules,
                },
            }
        };

        // Resolve icons dir with fallback chain
        let mut config = config;
        config.settings.icons_dir = Self::resolve_icons_dir(config.settings.icons_dir);

        config.write_params();
        Ok(config)
    }

    fn merge_modules(
        builtins: HashMap<String, ModuleDef>,
        overrides: HashMap<String, serde_json::Value>,
    ) -> HashMap<String, ModuleDef> {
        let mut result = HashMap::new();

        // If no modules specified in config, include all builtins
        let include_all = overrides.is_empty();

        for (id, base_def) in &builtins {
            if !include_all && !overrides.contains_key(id) {
                continue;
            }

            let def = if let Some(override_val) = overrides.get(id) {
                let mut base_val = serde_json::to_value(base_def)
                    .expect("failed to serialize builtin module");
                json_merge(&mut base_val, override_val.clone());
                serde_json::from_value(base_val).unwrap_or_else(|e| {
                    log::error!("failed to merge module {id}: {e}");
                    let val = serde_json::to_value(base_def).unwrap();
                    serde_json::from_value(val).unwrap()
                })
            } else {
                let val = serde_json::to_value(base_def)
                    .expect("failed to serialize builtin module");
                serde_json::from_value(val).expect("failed to deserialize builtin module")
            };

            result.insert(id.clone(), def);
        }

        // User-defined modules not in builtins
        for (id, override_val) in &overrides {
            if builtins.contains_key(id) {
                continue;
            }
            let def = serde_json::from_value(override_val.clone()).unwrap_or_else(|e| {
                log::error!("failed to parse user module {id}: {e}");
                ModuleDef {
                    name: id.clone(),
                    ..Default::default()
                }
            });
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
            description: None,
            icon: None,
            source: None,
            badges: HashMap::new(),
            widget: None,
            label: None,
            hooks: Vec::new(),
            key_hints: Vec::new(),
            module_type: None,
            params: HashMap::new(),
            actions: Vec::new(),
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

/// Convert a TOML value tree to a serde_json value tree
fn toml_to_json(val: toml::Value) -> serde_json::Value {
    match val {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(a) => {
            serde_json::Value::Array(a.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(t) => {
            let map = t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect();
            serde_json::Value::Object(map)
        }
    }
}

// --- Defaults ---

fn default_position() -> Position { Position::Top }
fn default_font() -> String { "monospace".into() }
fn default_emoji_font() -> String { "Noto Emoji".into() }
fn default_font_size() -> f32 { 14.0 }
fn default_color() -> Rgba { Rgba::new(0x22, 0x22, 0x22, 255) }
fn default_opacity() -> f32 { 0.8 }
fn default_radius() -> f32 { 6.0 }
fn default_padding() -> f32 { 6.0 }
fn default_gap() -> f32 { 6.0 }
fn default_scale() -> f32 { 1.0 }
fn default_interval() -> u64 { 5 }
fn default_icon_weight() -> String { "light".into() }
fn default_hook_timeout() -> u64 { 5 }

impl Default for Position {
    fn default() -> Self {
        default_position()
    }
}
