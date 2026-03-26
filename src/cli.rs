use clap::{Arg, Command};

use crate::ipc::{self, IpcRequest};
use crate::modlib;

include!(concat!(env!("OUT_DIR"), "/mod_meta.rs"));

pub struct Cli {
    pub config: Option<String>,
    pub cmd: Option<Cmd>,
}

pub enum Cmd {
    Launcher,
    Dismiss,
    State,
    Style(String),
    Module(Vec<String>),
}

pub fn build_cli() -> Command {
    let mut cmd = Command::new("cyberdeck")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Wayland status bar")
        .arg(Arg::new("config").long("config").help("Config file path"))
        .subcommand(Command::new("launcher").about("Toggle the launcher"))
        .subcommand(Command::new("dismiss").about("Dismiss the current view"))
        .subcommand(Command::new("state").about("Print bar state as JSON"))
        .subcommand(Command::new("style").about("Set bar style at runtime")
            .arg(Arg::new("name").required(true)
                .help("floating, attached, neumorphic, or glass")));

    for (id, name, cmds) in MOD_META {
        let mut sub = Command::new(*id)
            .about(format!("{name} module"))
            .arg(Arg::new("args").num_args(..).trailing_var_arg(true));

        sub = sub.subcommand(Command::new("open").about("Open in bar"));
        let mut multi_word: Vec<&str> = Vec::new();
        for action in *cmds {
            if action.contains(' ') {
                multi_word.push(action);
            } else {
                sub = sub.subcommand(Command::new(*action).about(*action));
            }
        }
        if !multi_word.is_empty() {
            let extra: Vec<String> = multi_word.iter()
                .map(|a| format!("  {a}"))
                .collect();
            sub = sub.after_help(format!(
                "Multi-word commands (pass as separate args):\n{}",
                extra.join("\n")
            ));
        }

        cmd = cmd.subcommand(sub);
    }

    cmd
}

pub fn parse() -> Cli {
    let matches = build_cli().get_matches();
    let config = matches.get_one::<String>("config").cloned();

    let cmd = match matches.subcommand() {
        Some(("launcher", _)) => Some(Cmd::Launcher),
        Some(("dismiss", _)) => Some(Cmd::Dismiss),
        Some(("state", _)) => Some(Cmd::State),
        Some(("style", sub)) => {
            let name = sub.get_one::<String>("name").cloned().unwrap_or_default();
            Some(Cmd::Style(name))
        }
        Some((name, sub)) => {
            let mut args = vec![name.to_string()];
            if let Some((action, _)) = sub.subcommand() {
                args.push(action.to_string());
            } else if let Some(vals) = sub.get_many::<String>("args") {
                args.extend(vals.cloned());
            }
            Some(Cmd::Module(args))
        }
        None => None,
    };

    Cli { config, cmd }
}

pub fn run_cmd(cmd: Cmd) {
    match cmd {
        Cmd::Launcher => ipc::send_request(&IpcRequest::Launcher),
        Cmd::Dismiss => ipc::send_request(&IpcRequest::Dismiss),
        Cmd::State => ipc::send_request(&IpcRequest::State),
        Cmd::Style(name) => ipc::send_request(&IpcRequest::SetStyle { style: name }),
        Cmd::Module(args) => run_module_cmd(&args),
    }
}

fn run_module_cmd(args: &[String]) {
    if args.is_empty() {
        eprintln!("usage: cyberdeck <module> <action> [args...]");
        std::process::exit(1);
    }

    let mod_name = &args[0];
    let builtins = modlib::builtin_modules();

    let module = match builtins.get(mod_name.as_str()) {
        Some(m) => m,
        None => {
            eprintln!("unknown module: {mod_name}");
            std::process::exit(1);
        }
    };

    let rest = &args[1..];

    if rest.is_empty() {
        // clap handles --help; bare module name with no action shows help too
        let _ = build_cli()
            .find_subcommand_mut(mod_name)
            .map(|c| c.print_help());
        return;
    }

    let action = rest[0].as_str();
    let extra = &rest[1..];

    // Native mod commands
    if mod_name == "inputs" && action == "denoise" {
        crate::mods::inputs::cli_toggle_denoise();
        return;
    }
    if mod_name == "notifications" && action == "clear" {
        crate::notifications::STORE.lock().unwrap().clear_all();
        eprintln!("notifications cleared");
        return;
    }
    if mod_name == "wallpaper" {
        let params: serde_json::Map<String, serde_json::Value> = module
            .params
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        match action {
            "shuffle" => {
                let group = extra.first().map(|s| s.as_str());
                crate::mods::wallpaper::shuffle(&params, group);
                return;
            }
            "init" => {
                crate::mods::wallpaper::init(&params);
                return;
            }
            _ => {}
        }
    }

    // Implicit "open" action
    if action == "open" {
        ipc::send_request(&IpcRequest::Push {
            child: mod_name.clone(),
        });
        return;
    }

    // Try matching against commands (greedy: join words to match multi-word keys)
    if let Some(cmd) = find_command(&module.commands, rest) {
        exec_shell(cmd);
        return;
    }

    // Single-word command fallback
    if let Some(cmd) = module.commands.get(action) {
        let full = if extra.is_empty() {
            cmd.clone()
        } else {
            format!("{cmd} {}", extra.join(" "))
        };
        exec_shell(&full);
        return;
    }

    eprintln!("unknown action '{action}' for module '{mod_name}'");
    eprintln!("run 'cyberdeck {mod_name} --help' to see available actions");
    std::process::exit(1);
}

/// Try to match a multi-word command key against the args.
/// e.g. args ["vol", "up"] matches command key "vol up"
fn find_command<'a>(
    commands: &'a std::collections::HashMap<String, String>,
    args: &[String],
) -> Option<&'a str> {
    for n in (2..=args.len()).rev() {
        let key = args[..n].join(" ");
        if let Some(cmd) = commands.get(&key) {
            return Some(cmd.as_str());
        }
    }
    None
}

fn exec_shell(cmd: &str) {
    let status = std::process::Command::new("sh")
        .args(["-c", cmd])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("failed to run '{cmd}': {e}");
            std::process::exit(1);
        });
    std::process::exit(status.code().unwrap_or(1));
}

