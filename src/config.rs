use serde::Deserialize;
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

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BadgeDef {
    pub template: String,
    pub condition: Option<String>,
    pub highlight: Option<String>,
    #[serde(default)]
    pub icon_scale: Option<f32>,
}

// --- Hooks ---

#[derive(Debug, Deserialize)]
pub struct HookDef {
    pub condition: String,
    pub action: String,
    #[serde(default = "default_hook_timeout")]
    pub timeout: u64,
}

// --- Key hints ---

#[derive(Debug, Deserialize, Clone)]
pub struct KeyHintDef {
    pub key: String,
    pub action: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LabelDef {
    pub template: String,
}

// --- Source ---

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
pub struct WidgetDef {
    pub template: String,
    pub condition: Option<String>,
}

// --- Loading ---

impl Config {
    pub fn load(path: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
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
        let config: Config = serde_json::from_str(&data)?;
        config.write_params();
        Ok(config)
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

    pub fn find_command(&self, mod_name: &str, cmd_name: &str) -> Option<&str> {
        let module = self.bar.modules.get(mod_name)?;
        module.commands.get(cmd_name).map(|s| s.as_str())
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
