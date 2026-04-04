use std::collections::HashMap;
use crate::config::ModuleDef;

include!(concat!(env!("OUT_DIR"), "/mod_builtins.rs"));

pub fn builtin_modules() -> HashMap<String, ModuleDef> {
    BUILTIN_MODS.iter()
        .map(|(id, toml_str)| {
            let def: ModuleDef = toml::from_str(toml_str)
                .unwrap_or_else(|e| panic!("bad builtin mod {id}: {e}"));
            (id.to_string(), def)
        })
        .collect()
}
