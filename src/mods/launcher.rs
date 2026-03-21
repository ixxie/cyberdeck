use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let dirs = scan_dirs();
    // Dedup by desktop ID (filename); later dirs override earlier ones
    let mut seen: HashMap<String, (String, String)> = HashMap::new();

    for dir in dirs {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let desktop_id = path.file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_string();
            if let Some((name, exec)) = parse_desktop(&path) {
                seen.insert(desktop_id.clone(), (name, exec));
            }
        }
    }

    let mut entries: Vec<Value> = seen
        .into_iter()
        .map(|(desktop_id, (name, exec))| json!({"name": name, "exec": exec, "desktop_id": desktop_id}))
        .collect();
    entries.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });

    json!({ "entries": entries })
}

fn scan_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/run/current-system/sw/share/applications"),
    ];

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".nix-profile/share/applications"));
    }

    if let Ok(xdg) = env::var("XDG_DATA_DIRS") {
        for base in xdg.split(':') {
            let p = PathBuf::from(base).join("applications");
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    }

    dirs
}

fn parse_desktop(path: &PathBuf) -> Option<(String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut no_display = false;
    let mut hidden = false;
    let mut in_entry = false;

    for line in content.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_entry = true;
            continue;
        }
        if line.starts_with('[') {
            if in_entry {
                break;
            }
            continue;
        }
        if !in_entry {
            continue;
        }

        if let Some((key, val)) = line.split_once('=') {
            match key.trim() {
                "Name" if name.is_none() => name = Some(val.trim().to_string()),
                "Exec" => exec = Some(strip_field_codes(val.trim())),
                "NoDisplay" if val.trim() == "true" => no_display = true,
                "Hidden" if val.trim() == "true" => hidden = true,
                _ => {}
            }
        }
    }

    if no_display || hidden {
        return None;
    }

    Some((name?, exec?))
}

fn strip_field_codes(exec: &str) -> String {
    let mut result = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(&next) = chars.peek() {
                if "fFuUdDnNickvm".contains(next) {
                    chars.next();
                    // trim trailing space left by removal
                    if result.ends_with(' ') && chars.peek().map_or(true, |c| *c == ' ') {
                        result.pop();
                    }
                    continue;
                }
            }
        }
        result.push(ch);
    }
    result.trim().to_string()
}
