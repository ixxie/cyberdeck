use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn main() {
    let mods_dir = Path::new("mods");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("mod_meta.rs");

    let mut entries: Vec<(String, String, Vec<String>)> = Vec::new();

    for entry in fs::read_dir(mods_dir).expect("cannot read mods/") {
        let entry = entry.unwrap();
        let path = entry.path();
        let fname = path.file_name().unwrap().to_str().unwrap();
        if !fname.ends_with(".mod.toml") {
            continue;
        }
        let id = fname.strip_suffix(".mod.toml").unwrap().to_string();

        let content = fs::read_to_string(&path).unwrap();
        let table: HashMap<String, toml::Value> = toml::from_str(&content).unwrap();

        let name = table
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();

        // Prefer [[actions]] names, fall back to [commands] keys
        let cmds: Vec<String> = table
            .get("actions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .or_else(|| {
                table.get("commands")
                    .and_then(|v| v.as_table())
                    .map(|t| t.keys().cloned().collect())
            })
            .unwrap_or_default();

        entries.push((id, name, cmds));
        println!("cargo:rerun-if-changed={}", path.display());
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Generate MOD_META for CLI (included by cli.rs)
    let mut meta = String::from("pub const MOD_META: &[(&str, &str, &[&str])] = &[\n");
    for (id, name, cmds) in &entries {
        let cmds_str: Vec<String> = cmds.iter().map(|c| format!("\"{}\"", c)).collect();
        meta.push_str(&format!(
            "    (\"{id}\", \"{name}\", &[{}]),\n",
            cmds_str.join(", ")
        ));
    }
    meta.push_str("];\n");
    fs::write(&dest, meta).unwrap();

    // Generate BUILTIN_MODS for modlib (included by modlib.rs)
    let mods_abs = fs::canonicalize(mods_dir).expect("cannot canonicalize mods/");
    let mut builtins = String::from("pub const BUILTIN_MODS: &[(&str, &str)] = &[\n");
    for (id, _, _) in &entries {
        let abs = mods_abs.join(format!("{id}.mod.toml"));
        builtins.push_str(&format!(
            "    (\"{id}\", include_str!(\"{}\")),\n",
            abs.display()
        ));
    }
    builtins.push_str("];\n");
    let builtins_dest = Path::new(&out_dir).join("mod_builtins.rs");
    fs::write(&builtins_dest, builtins).unwrap();
}
