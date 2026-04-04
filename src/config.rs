use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::color::Rgba;

// --- Spacing (CSS-style: 1, 2, or 4 values) ---

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub fn uniform(v: f32) -> Self {
        Self { top: v, right: v, bottom: v, left: v }
    }

    pub fn axes(y: f32, x: f32) -> Self {
        Self { top: y, right: x, bottom: y, left: x }
    }

    pub fn y(&self) -> f32 { self.top + self.bottom }

    pub fn scale(&self, s: f32) -> Self {
        Self { top: self.top * s, right: self.right * s, bottom: self.bottom * s, left: self.left * s }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum Spacing {
    Uniform(f32),
    Axes([f32; 2]),
    Sides([f32; 4]),
}

impl Spacing {
    pub fn resolve(&self) -> Edges {
        match self {
            Spacing::Uniform(v) => Edges::uniform(*v),
            Spacing::Axes([y, x]) => Edges::axes(*y, *x),
            Spacing::Sides([t, r, b, l]) => Edges { top: *t, right: *r, bottom: *b, left: *l },
        }
    }
}

// --- Material ---

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MaterialType {
    #[default]
    Solid,
    Glass,
}

/// Material config: either a string shorthand ("solid"/"glass") or a struct.
#[derive(Debug, Clone)]
pub struct MaterialCfg {
    pub material_type: MaterialType,
    pub opacity: Option<f32>,
    pub color: Option<Rgba>,
}

impl Default for MaterialCfg {
    fn default() -> Self {
        Self { material_type: MaterialType::Solid, opacity: None, color: None }
    }
}

impl<'de> Deserialize<'de> for MaterialCfg {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de;

        struct MaterialVisitor;
        impl<'de> de::Visitor<'de> for MaterialVisitor {
            type Value = MaterialCfg;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a material string or object")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<MaterialCfg, E> {
                let material_type = match v {
                    "solid" => MaterialType::Solid,
                    "glass" => MaterialType::Glass,
                    _ => return Err(E::custom(format!("unknown material: {v}"))),
                };
                Ok(MaterialCfg { material_type, opacity: None, color: None })
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<MaterialCfg, A::Error> {
                let mut material_type = MaterialType::Solid;
                let mut opacity = None;
                let mut color = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => material_type = map.next_value()?,
                        "opacity" => opacity = Some(map.next_value()?),
                        "color" => color = Some(map.next_value()?),
                        _ => { let _ = map.next_value::<serde::de::IgnoredAny>()?; }
                    }
                }
                Ok(MaterialCfg { material_type, opacity, color })
            }
        }
        deserializer.deserialize_any(MaterialVisitor)
    }
}

impl MaterialCfg {
    pub fn resolve(&self, fallback_color: Rgba) -> ResolvedMaterial {
        let default_opacity = match self.material_type {
            MaterialType::Solid => 0.8,
            MaterialType::Glass => 0.0,
        };
        ResolvedMaterial {
            material_type: self.material_type,
            opacity: self.opacity.unwrap_or(default_opacity),
            color: self.color.unwrap_or(fallback_color),
        }
    }
}

pub struct ResolvedMaterial {
    #[allow(dead_code)]
    pub material_type: MaterialType,
    pub opacity: f32,
    pub color: Rgba,
}

// --- Theme ---

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Classic,
    Floating,
    Pills,
    Transparent,
}

impl Theme {
    pub fn bar_enabled(self) -> bool {
        matches!(self, Theme::Classic | Theme::Floating)
    }
    pub fn bar_floating(self) -> bool {
        matches!(self, Theme::Floating)
    }
    pub fn pill_enabled(self) -> bool {
        matches!(self, Theme::Pills | Theme::Transparent)
    }
}

// --- Bar & Pill config ---

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct BarCfg {
    pub enabled: Option<bool>,
    pub floating: Option<bool>,
    pub color: Option<Rgba>,
    pub opacity: Option<f32>,
    pub radius: Option<f32>,
    pub padding: Option<Spacing>,
    pub margin: Option<Spacing>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct PillCfg {
    pub enabled: Option<bool>,
    pub color: Option<Rgba>,
    pub opacity: Option<f32>,
    pub radius: Option<f32>,
    pub padding: Option<Spacing>,
    pub max_chars: Option<usize>,
}

// --- Resolved values ---

pub struct ResolvedBar {
    pub color: Rgba,
    pub opacity: f32,
    pub radius: f32,
    pub padding: Edges,
    pub margin: Edges,
}

pub struct ResolvedPill {
    pub color: Rgba,
    pub opacity: f32,
    pub radius: f32,
    pub padding: Edges,
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
    #[serde(default)]
    pub theme: Theme,
    #[serde(default = "default_position")]
    pub position: Position,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default = "default_emoji_font")]
    pub emoji_font: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub material: MaterialCfg,
    #[serde(default = "default_color")]
    pub color: Rgba,
    #[serde(default = "default_radius")]
    pub radius: f32,
    #[serde(default)]
    pub bar: BarCfg,
    #[serde(default)]
    pub pill: PillCfg,
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
    pub fn monitor_scale(&self, name: Option<&str>) -> f32 {
        name.and_then(|n| self.monitors.get(n))
            .and_then(|m| m.scale)
            .unwrap_or(self.scale)
    }

    pub fn resolved_material(&self) -> ResolvedMaterial {
        self.material.resolve(self.color)
    }

    pub fn bar_enabled(&self) -> bool {
        self.bar.enabled.unwrap_or_else(|| self.theme.bar_enabled())
    }

    pub fn bar_floating(&self) -> bool {
        self.bar.floating.unwrap_or_else(|| self.theme.bar_floating())
    }

    pub fn pill_enabled(&self) -> bool {
        self.pill.enabled.unwrap_or_else(|| self.theme.pill_enabled())
    }

    pub fn resolve_bar(&self) -> ResolvedBar {
        let mat = self.resolved_material();
        let enabled = self.bar_enabled();
        let floating = self.bar_floating();
        let opacity = if enabled {
            self.bar.opacity.unwrap_or(mat.opacity)
        } else {
            0.0
        };
        let radius = if floating {
            self.bar.radius.unwrap_or(self.radius)
        } else {
            self.bar.radius.unwrap_or(0.0)
        };
        let padding = self.bar.padding.as_ref()
            .map(|s| s.resolve())
            .unwrap_or_else(|| if enabled {
                Edges::uniform(self.gap)
            } else {
                // Invisible track: gap on screen edges, 0 on content-facing edge
                // (workspace padding already spaces windows from the bar zone)
                let g = self.gap;
                match self.position {
                    Position::Bottom => Edges { top: 0.0, right: g, bottom: g, left: g },
                    Position::Top => Edges { top: g, right: g, bottom: 0.0, left: g },
                }
            });
        let margin = if floating {
            self.bar.margin.as_ref()
                .map(|s| s.resolve())
                .unwrap_or_else(|| {
                    // Floating: gap on screen edges, 0 on content-facing edge
                    let g = self.gap;
                    match self.position {
                        Position::Bottom => Edges { top: 0.0, right: g, bottom: g, left: g },
                        Position::Top => Edges { top: g, right: g, bottom: 0.0, left: g },
                    }
                })
        } else {
            self.bar.margin.as_ref()
                .map(|s| s.resolve())
                .unwrap_or_else(|| Edges::uniform(0.0))
        };
        ResolvedBar {
            color: self.bar.color.unwrap_or(mat.color),
            opacity,
            radius,
            padding,
            margin,
        }
    }

    pub fn resolve_pill(&self) -> ResolvedPill {
        let mat = self.resolved_material();
        let enabled = self.pill_enabled();
        let default_opacity = match self.theme {
            Theme::Transparent => 0.0,
            _ => mat.opacity,
        };
        let opacity = if enabled {
            self.pill.opacity.unwrap_or(default_opacity)
        } else {
            0.0
        };
        let bar_is_container = self.bar_enabled();
        let padding = self.pill.padding.as_ref()
            .map(|s| s.resolve())
            .unwrap_or_else(|| if enabled && !bar_is_container {
                Edges::uniform(self.gap)
            } else {
                Edges::uniform(0.0)
            });
        ResolvedPill {
            color: self.pill.color.unwrap_or(mat.color),
            opacity,
            radius: self.pill.radius.unwrap_or(self.radius),
            padding,
            max_chars: self.pill.max_chars.unwrap_or(48),
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
    #[serde(default)]
    theme: Theme,
    #[serde(default = "default_position")]
    position: Position,
    #[serde(default = "default_font")]
    font: String,
    #[serde(default = "default_emoji_font")]
    emoji_font: String,
    #[serde(default = "default_font_size")]
    font_size: f32,
    #[serde(default = "default_gap")]
    gap: f32,
    #[serde(default = "default_scale")]
    scale: f32,
    #[serde(default)]
    material: MaterialCfg,
    #[serde(default = "default_color")]
    color: Rgba,
    #[serde(default = "default_radius")]
    radius: f32,
    #[serde(default)]
    bar: BarCfg,
    #[serde(default)]
    pill: PillCfg,
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
                    theme: runtime.settings.theme,
                    position: runtime.settings.position,
                    font: runtime.settings.font,
                    emoji_font: runtime.settings.emoji_font,
                    font_size: runtime.settings.font_size,
                    gap: runtime.settings.gap,
                    scale: runtime.settings.scale,
                    material: runtime.settings.material,
                    color: runtime.settings.color,
                    radius: runtime.settings.radius,
                    bar: runtime.settings.bar,
                    pill: runtime.settings.pill,
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
fn default_radius() -> f32 { 6.0 }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Layout model (bottom position, gap=8, radius=6):
    //
    // Classic:
    //   <track opacity="0.8" radius="0" margin="0" padding="8">
    //     <pill opacity="0" padding="0" />
    //   </track>
    //
    // Floating:
    //   <track opacity="0.8" radius="6" margin="0 8 8 8" padding="8">
    //     <pill opacity="0" padding="0" />
    //   </track>
    //
    // Pills:
    //   <track opacity="0" margin="0" padding="0 8 8 8">
    //     <pill opacity="0.8" radius="6" padding="8" />
    //   </track>
    //
    // Transparent (= pills with invisible pill backgrounds):
    //   <track opacity="0" margin="0" padding="0 8 8 8">
    //     <pill opacity="0" radius="6" padding="8" />
    //   </track>

    fn settings(theme: Theme) -> Settings {
        Settings {
            theme,
            position: Position::Bottom,
            font: "mono".into(),
            emoji_font: "emoji".into(),
            font_size: 14.0,
            gap: 8.0,
            scale: 1.0,
            material: MaterialCfg::default(),
            color: Rgba::new(0x22, 0x22, 0x22, 255),
            radius: 6.0,
            bar: BarCfg::default(),
            pill: PillCfg::default(),
            icons_dir: None,
            icon_weight: "light".into(),
            monitors: HashMap::new(),
            defocus_on_niri_events: true,
            wrap_nav: true,
        }
    }

    const G: f32 = 8.0;
    const SCREEN_EDGES: Edges = Edges { top: 0.0, right: G, bottom: G, left: G };

    // -- Theme defaults --

    #[test]
    fn classic_bar() {
        let s = settings(Theme::Classic);
        let bar = s.resolve_bar();
        assert_eq!(bar.opacity, 0.8);
        assert_eq!(bar.radius, 0.0);
        assert_eq!(bar.padding, Edges::uniform(G));
        assert_eq!(bar.margin, Edges::uniform(0.0));
    }

    #[test]
    fn classic_pill() {
        let s = settings(Theme::Classic);
        let pill = s.resolve_pill();
        assert_eq!(pill.opacity, 0.0);
        assert_eq!(pill.padding, Edges::uniform(0.0));
    }

    #[test]
    fn floating_bar() {
        let s = settings(Theme::Floating);
        let bar = s.resolve_bar();
        assert_eq!(bar.opacity, 0.8);
        assert_eq!(bar.radius, 6.0);
        assert_eq!(bar.padding, Edges::uniform(G));
        assert_eq!(bar.margin, SCREEN_EDGES);
    }

    #[test]
    fn floating_pill() {
        let s = settings(Theme::Floating);
        let pill = s.resolve_pill();
        assert_eq!(pill.opacity, 0.0);
        assert_eq!(pill.padding, Edges::uniform(0.0));
    }

    #[test]
    fn pills_bar() {
        let s = settings(Theme::Pills);
        let bar = s.resolve_bar();
        assert_eq!(bar.opacity, 0.0);
        assert_eq!(bar.padding, SCREEN_EDGES);
        assert_eq!(bar.margin, Edges::uniform(0.0));
    }

    #[test]
    fn pills_pill() {
        let s = settings(Theme::Pills);
        let pill = s.resolve_pill();
        assert_eq!(pill.opacity, 0.8);
        assert_eq!(pill.padding, Edges::uniform(G));
    }

    #[test]
    fn transparent_bar() {
        let s = settings(Theme::Transparent);
        let bar = s.resolve_bar();
        assert_eq!(bar.opacity, 0.0);
        assert_eq!(bar.padding, SCREEN_EDGES);
        assert_eq!(bar.margin, Edges::uniform(0.0));
    }

    #[test]
    fn transparent_pill() {
        let s = settings(Theme::Transparent);
        let pill = s.resolve_pill();
        assert_eq!(pill.opacity, 0.0);
        assert_eq!(pill.padding, Edges::uniform(G));
    }

    // -- Position flips content-facing edge --

    #[test]
    fn floating_top_margin() {
        let mut s = settings(Theme::Floating);
        s.position = Position::Top;
        let bar = s.resolve_bar();
        assert_eq!(bar.margin, Edges { top: G, right: G, bottom: 0.0, left: G });
    }

    #[test]
    fn transparent_top_padding() {
        let mut s = settings(Theme::Transparent);
        s.position = Position::Top;
        let bar = s.resolve_bar();
        assert_eq!(bar.padding, Edges { top: G, right: G, bottom: 0.0, left: G });
    }

    // -- Overrides --

    #[test]
    fn override_bar_enabled_on_pills_theme() {
        let mut s = settings(Theme::Pills);
        s.bar.enabled = Some(true);
        let bar = s.resolve_bar();
        assert_eq!(bar.opacity, 0.8);
        assert_eq!(bar.padding, Edges::uniform(G));
        let pill = s.resolve_pill();
        assert_eq!(pill.padding, Edges::uniform(0.0));
    }

    #[test]
    fn override_pill_enabled_on_classic_theme() {
        let mut s = settings(Theme::Classic);
        s.pill.enabled = Some(true);
        let pill = s.resolve_pill();
        assert_eq!(pill.opacity, 0.8);
        assert_eq!(pill.padding, Edges::uniform(0.0));
    }

    #[test]
    fn override_bar_opacity() {
        let mut s = settings(Theme::Classic);
        s.bar.opacity = Some(0.5);
        assert_eq!(s.resolve_bar().opacity, 0.5);
    }

    #[test]
    fn override_bar_padding() {
        let mut s = settings(Theme::Transparent);
        s.bar.padding = Some(Spacing::Uniform(4.0));
        assert_eq!(s.resolve_bar().padding, Edges::uniform(4.0));
    }

    #[test]
    fn override_pill_padding() {
        let mut s = settings(Theme::Pills);
        s.pill.padding = Some(Spacing::Axes([2.0, 4.0]));
        assert_eq!(s.resolve_pill().padding, Edges::axes(2.0, 4.0));
    }

    #[test]
    fn override_floating_on_classic() {
        let mut s = settings(Theme::Classic);
        s.bar.floating = Some(true);
        let bar = s.resolve_bar();
        assert_eq!(bar.radius, 6.0);
        assert_eq!(bar.margin, SCREEN_EDGES);
    }

    #[test]
    fn override_material_opacity_flows_to_bar() {
        let mut s = settings(Theme::Classic);
        s.material.opacity = Some(0.6);
        assert_eq!(s.resolve_bar().opacity, 0.6);
    }

    #[test]
    fn bar_opacity_overrides_material() {
        let mut s = settings(Theme::Classic);
        s.material.opacity = Some(0.6);
        s.bar.opacity = Some(0.3);
        assert_eq!(s.resolve_bar().opacity, 0.3);
    }

    #[test]
    fn disabled_bar_ignores_opacity_override() {
        let mut s = settings(Theme::Transparent);
        s.bar.opacity = Some(0.9);
        assert_eq!(s.resolve_bar().opacity, 0.0);
    }
}
