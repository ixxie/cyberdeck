use clap::{Arg, Command};

use crate::ipc::{self, IpcRequest};
use crate::modlib;

include!(concat!(env!("OUT_DIR"), "/mod_meta.rs"));

pub struct Cli {
    pub config: Option<String>,
    pub cmd: Option<Cmd>,
}

pub enum Cmd {
    Init,
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
        .subcommand(Command::new("init").about("Create default config.toml"))
        .subcommand(Command::new("launcher").about("Toggle the launcher"))
        .subcommand(Command::new("dismiss").about("Dismiss the current view"))
        .subcommand(Command::new("state").about("Print bar state as JSON"))
        .subcommand(Command::new("style").about("Set bar style at runtime")
            .arg(Arg::new("name").required(true)
                .help("classic, floating, pills, or transparent")));

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
        Some(("init", _)) => Some(Cmd::Init),
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
        Cmd::Init => run_init(),
        Cmd::Launcher => ipc::send_request(&IpcRequest::Launcher),
        Cmd::Dismiss => ipc::send_request(&IpcRequest::Dismiss),
        Cmd::State => ipc::send_request(&IpcRequest::State),
        Cmd::Style(name) => ipc::send_request(&IpcRequest::SetStyle { style: name }),
        Cmd::Module(args) => run_module_cmd(&args),
    }
}

fn run_init() {
    let dir = crate::config::Config::config_dir();
    let path = dir.join("config.toml");
    if path.exists() {
        eprintln!("config already exists: {}", path.display());
        std::process::exit(1);
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("failed to create config dir: {e}");
        std::process::exit(1);
    }
    if let Err(e) = std::fs::write(&path, CONFIG_TEMPLATE) {
        eprintln!("failed to write config: {e}");
        std::process::exit(1);
    }
    eprintln!("created {}", path.display());
    eprintln!("edit the file to enable modules and customize settings");
}

const CONFIG_TEMPLATE: &str = r##"# Cyberdeck configuration
# Uncomment and modify options as needed.

[settings]
# position = "top"          # top or bottom
# font = "monospace"
# font-size = 14
# layout = "pills"          # classic, floating, pills, or transparent
# gap = 6                   # spacing between visual elements (px)
# scale = 1.0               # global UI scale
# icon-weight = "light"     # regular, bold, thin, light, fill, or duotone

# [settings.theme]
# color = "#222222"
# opacity = 0.8
# radius = 6
# padding = 6
#
# [settings.theme.track]    # bar background overrides
# color = "#222222"
# opacity = 1.0
#
# [settings.theme.pill]     # pill overrides
# opacity = 1.0
# radius = 8

# [settings.monitors.DP-1]
# scale = 1.5

# === Modules ===
# Uncomment a module section to enable it.
# Each module may require external tools — see notes below.

# --- No dependencies ---

[bar.modules.calendar]      # clock & calendar
[bar.modules.window]        # window title
[bar.modules.workspaces]    # workspace info (niri)
[bar.modules.system]        # cpu, memory, uptime
[bar.modules.storage]       # disk usage

# --- Optional dependencies ---

# [bar.modules.notifications]   # desktop notifications
                                # deps: libnotify

# [bar.modules.network]         # wifi & ethernet
                                # deps (apt): network-manager
                                # deps (pacman): networkmanager

# [bar.modules.bluetooth]       # bluetooth devices
                                # deps: bluez / bluez-utils

# [bar.modules.brightness]      # screen brightness
                                # deps: brightnessctl

# [bar.modules.outputs]         # audio output / speakers
                                # deps: wireplumber

# [bar.modules.inputs]          # audio input / microphone
                                # deps: wireplumber

# [bar.modules.media]           # now playing
                                # deps: playerctl

# [bar.modules.session]         # power & session controls
                                # deps: upower

# [bar.modules.weather]         # weather forecast
                                # deps: curl
# location = "London"           # optional: city name for wttr.in

# [bar.modules.screenshot]      # screenshots
                                # deps: grim, slurp, wl-clipboard

# [bar.modules.recording]      # screen recording
                                # deps: slurp, wl-screenrec

# [bar.modules.wallpaper]       # wallpaper cycling
                                # deps: swww
# dir = "~/Pictures/wallpapers"
"##;

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
        let _ = build_cli()
            .find_subcommand_mut(mod_name)
            .map(|c| c.print_help());
        return;
    }

    let action = rest[0].as_str();
    let extra = &rest[1..];

    // "open" is always available
    if action == "open" {
        ipc::send_request(&IpcRequest::Push {
            child: mod_name.clone(),
        });
        return;
    }

    if let Some(act) = module.action_by_name(action) {
        if act.run == "native" {
            let params: serde_json::Map<String, serde_json::Value> = module
                .params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            match crate::actions::exec_native(mod_name, action, extra, &params) {
                crate::actions::ActionResult::Ok { toast } => {
                    ipc::notify_bar(mod_name, &toast);
                }
                crate::actions::ActionResult::BarOnly => {
                    ipc::send_request(&IpcRequest::Action {
                        module: mod_name.clone(),
                        action: action.to_string(),
                        args: extra.to_vec(),
                    });
                }
                crate::actions::ActionResult::Unknown => {
                    eprintln!("unknown native action '{action}' for module '{mod_name}'");
                    std::process::exit(1);
                }
            }
        } else {
            let full = if extra.is_empty() {
                act.run.clone()
            } else {
                format!("{} {}", act.run, extra.join(" "))
            };
            exec_shell_and_notify(mod_name, &act.label, &full);
        }
        return;
    }

    eprintln!("unknown action '{action}' for module '{mod_name}'");
    eprintln!("run 'cyberdeck {mod_name} --help' to see available actions");
    std::process::exit(1);
}

fn exec_shell_and_notify(module: &str, label: &str, cmd: &str) {
    let status = std::process::Command::new("sh")
        .args(["-c", cmd])
        .status()
        .unwrap_or_else(|e| {
            eprintln!("failed to run '{cmd}': {e}");
            std::process::exit(1);
        });
    if status.success() {
        let toast = if label.is_empty() { module } else { label };
        ipc::notify_bar(module, toast);
    }
    std::process::exit(status.code().unwrap_or(1));
}

