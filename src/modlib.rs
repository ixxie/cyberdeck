use std::collections::HashMap;
use crate::config::ModuleDef;

const MODS: &[(&str, &str)] = &[
    ("bluetooth", include_str!("../mods/bluetooth.mod.toml")),
    ("brightness", include_str!("../mods/brightness.mod.toml")),
    ("calendar", include_str!("../mods/calendar.mod.toml")),
    ("clipboard", include_str!("../mods/clipboard.mod.toml")),
    ("keyboard", include_str!("../mods/keyboard.mod.toml")),

    ("inputs", include_str!("../mods/inputs.mod.toml")),
    ("media", include_str!("../mods/media.mod.toml")),
    ("mounts", include_str!("../mods/mounts.mod.toml")),
    ("network", include_str!("../mods/network.mod.toml")),
    ("notifications", include_str!("../mods/notifications.mod.toml")),
    ("outputs", include_str!("../mods/outputs.mod.toml")),
    ("profiles", include_str!("../mods/profiles.mod.toml")),
    ("session", include_str!("../mods/session.mod.toml")),
    ("storage", include_str!("../mods/storage.mod.toml")),
    ("system", include_str!("../mods/system.mod.toml")),
    ("wallpaper", include_str!("../mods/wallpaper.mod.toml")),
    ("weather", include_str!("../mods/weather.mod.toml")),
    ("window", include_str!("../mods/window.mod.toml")),
    ("workspaces", include_str!("../mods/workspaces.mod.toml")),
];

pub fn builtin_modules() -> HashMap<String, ModuleDef> {
    MODS.iter()
        .map(|(id, toml_str)| {
            let def: ModuleDef = toml::from_str(toml_str)
                .unwrap_or_else(|e| panic!("bad builtin mod {id}: {e}"));
            (id.to_string(), def)
        })
        .collect()
}
