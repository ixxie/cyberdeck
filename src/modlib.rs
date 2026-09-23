// corpus:article/architecture
// corpus:decision/26-03-26-module-manifests
// corpus:decision/26-03-21-tera-templates

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A malformed template only surfaces as a log line and a silently
    /// missing badge at runtime, so check them all up front.
    #[test]
    fn builtin_templates_compile() {
        for (id, def) in builtin_modules() {
            let mut tera = tera::Tera::default();
            let mut check = |what: &str, tpl: &str| {
                assert!(
                    tera.add_raw_template(&format!("{id}.{what}"), tpl).is_ok(),
                    "{id}: bad {what} template"
                );
            };
            if let Some(w) = &def.widget {
                check("widget", &w.template);
                if let Some(c) = &w.condition {
                    check("widget.cond", c);
                }
            }
            for (name, badge) in &def.badges {
                check(&format!("badge.{name}"), &badge.template);
                if let Some(c) = &badge.condition {
                    check(&format!("badge.{name}.cond"), c);
                }
            }
            for (i, hook) in def.hooks.iter().enumerate() {
                check(&format!("hook.{i}"), &hook.condition);
            }
        }
    }

    /// The states that matter for the radio kill switches: a badge must
    /// appear for both a full and a partial block, and bluetooth must name
    /// the block rather than reporting a plain "off".
    #[test]
    fn radio_block_states_render() {
        let mods = builtin_modules();
        let render = |def: &ModuleDef, what: &str, data: serde_json::Value| -> String {
            let tpl = match what {
                "widget" => def.widget.as_ref().unwrap().template.clone(),
                name => def.badges[name].condition.clone().unwrap(),
            };
            let ctx = tera::Context::from_value(data).unwrap();
            tera::Tera::one_off(&tpl, &ctx, false).unwrap().trim().to_string()
        };
        let shown = |s: String| !s.is_empty() && s != "false" && s != "0";

        let air = &mods["airplane"];
        assert!(shown(render(air, "default", serde_json::json!({"enabled": true, "partial": false}))));
        assert!(shown(render(air, "partial", serde_json::json!({"enabled": false, "partial": true}))));
        assert!(!shown(render(air, "default", serde_json::json!({"enabled": false, "partial": false}))));
        assert_eq!(
            render(air, "widget", serde_json::json!({
                "enabled": false, "partial": true, "blocked_kinds": ["bluetooth"],
            })),
            "bluetooth blocked"
        );

        let bt = &mods["bluetooth"];
        assert_eq!(
            render(bt, "widget", serde_json::json!({
                "blocked": true, "airplane": true, "powered": false, "devices": [],
            })),
            "airplane mode"
        );
        assert_eq!(
            render(bt, "widget", serde_json::json!({
                "blocked": true, "airplane": false, "powered": false, "devices": [],
            })),
            "blocked (rfkill)"
        );
        // Bluetooth stays quiet when the airplane badge already explains it.
        assert!(shown(render(bt, "blocked", serde_json::json!({"blocked": true, "airplane": false}))));
        assert!(!shown(render(bt, "blocked", serde_json::json!({"blocked": true, "airplane": true}))));

        let connected = serde_json::json!({"devices": [{"connected": true}, {"connected": false}]});
        assert!(shown(render(bt, "default", connected)));
        assert!(!shown(render(bt, "default", serde_json::json!({"devices": [{"connected": false}]}))));

        let storage = &mods["storage"];
        let disks = |a: i64, b: i64| serde_json::json!({"disks": [{"percent": a}, {"percent": b}]});
        assert!(shown(render(storage, "default", disks(20, 91))));
        assert!(!shown(render(storage, "default", disks(20, 30))));
    }
}
