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

        let cmds: Vec<String> = table
            .get("commands")
            .and_then(|v| v.as_table())
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();

        entries.push((id, name, cmds));
        println!("cargo:rerun-if-changed={}", path.display());
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut code = String::from("pub const MOD_META: &[(&str, &str, &[&str])] = &[\n");
    for (id, name, cmds) in &entries {
        let cmds_str: Vec<String> = cmds.iter().map(|c| format!("\"{}\"", c)).collect();
        code.push_str(&format!(
            "    (\"{id}\", \"{name}\", &[{}]),\n",
            cmds_str.join(", ")
        ));
    }
    code.push_str("];\n");

    fs::write(&dest, code).unwrap();
}
