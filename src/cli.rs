use clap::{Arg, Command};

use crate::ipc::{self, IpcRequest};
use crate::modlib;

include!(concat!(env!("OUT_DIR"), "/mod_meta.rs"));

pub struct Cli {
    pub config: Option<String>,
    pub json: bool,
    pub cmd: Option<Cmd>,
}

pub enum Cmd {
    Init,
    Launcher,
    Dismiss,
    State,
    Theme(String),
    Pw(Vec<String>),
    Module(Vec<String>),
}

pub fn build_cli() -> Command {
    let mut cmd = Command::new("cyberdeck")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Wayland status bar")
        .arg(Arg::new("config").long("config").help("Config file path"))
        .arg(Arg::new("json").long("json").action(clap::ArgAction::SetTrue)
            .help("Output raw JSON response"))
        .subcommand(Command::new("init").about("Create default config.toml"))
        .subcommand(Command::new("launcher").about("Toggle the launcher"))
        .subcommand(Command::new("dismiss").about("Dismiss the current view"))
        .subcommand(Command::new("state").about("Print bar state as JSON"))
        .subcommand(Command::new("theme").about("Set bar theme at runtime")
            .arg(Arg::new("name").required(true)
                .help("classic, floating, pills, or transparent")))
        .subcommand(Command::new("pw")
            .about("Audio backend debug: status | select-mic <name> | select-speaker <name> | profile <a2dp|hfp> <name>")
            .arg(Arg::new("args").num_args(..).trailing_var_arg(true)))
        .subcommand(Command::new("workspace").about("Workspace commands (alias for workspaces)")
            .subcommand(Command::new("overview").about("overview"))
            .subcommand(Command::new("rename").about("rename"))
            .subcommand(Command::new("focus").about("focus"))
            .subcommand(Command::new("open").about("Open in bar"))
            .arg(Arg::new("args").num_args(..).trailing_var_arg(true)));

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
                sub = sub.subcommand(Command::new(*action).about(*action)
                    .arg(Arg::new("args").num_args(..).trailing_var_arg(true)));
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
        Some(("theme", sub)) => {
            let name = sub.get_one::<String>("name").cloned().unwrap_or_default();
            Some(Cmd::Theme(name))
        }
        Some(("pw", sub)) => {
            let args = sub
                .get_many::<String>("args")
                .map(|v| v.cloned().collect())
                .unwrap_or_default();
            Some(Cmd::Pw(args))
        }
        Some((name, sub)) => {
            // Resolve singular aliases to module names
            let module_name = match name {
                "workspace" => "workspaces",
                other => other,
            };
            let mut args = vec![module_name.to_string()];
            if let Some((action, action_matches)) = sub.subcommand() {
                args.push(action.to_string());
                if let Some(vals) = action_matches.get_many::<String>("args") {
                    args.extend(vals.cloned());
                }
            } else if let Some(vals) = sub.get_many::<String>("args") {
                args.extend(vals.cloned());
            }
            Some(Cmd::Module(args))
        }
        None => None,
    };

    let json = matches.get_flag("json");
    Cli { config, json, cmd }
}

pub fn run_cmd(cmd: Cmd, json: bool) {
    match cmd {
        Cmd::Init => run_init(),
        Cmd::Launcher => ipc::send_request(&IpcRequest::Launcher, json),
        Cmd::Dismiss => ipc::send_request(&IpcRequest::Dismiss, json),
        Cmd::State => ipc::send_request(&IpcRequest::State, json),
        Cmd::Theme(name) => ipc::send_request(&IpcRequest::SetTheme { theme: name }, json),
        Cmd::Pw(args) => run_pw_cmd(&args),
        Cmd::Module(args) => run_module_cmd(&args, json),
    }
}

// Talks to PipeWire directly (not the running bar) — a standalone harness for
// the audio backend, usable even when the daemon is down or suspect.
fn run_pw_cmd(args: &[String]) {
    use crate::pipewire::{self, ProfileKind, Target};

    pipewire::start();
    if !pipewire::wait_ready(std::time::Duration::from_secs(3)) {
        eprintln!("timed out waiting for the pipewire graph");
        std::process::exit(1);
    }
    pipewire::wait_settled(
        std::time::Duration::from_millis(300),
        std::time::Duration::from_secs(3),
    );
    let state = pipewire::query();

    let find = |sink: bool, needle: &str| -> Option<Target> {
        let needle = needle.to_lowercase();
        let list = if sink { &state.outputs } else { &state.inputs };
        list.iter()
            .find(|n| n.name.to_lowercase().contains(&needle))
            .map(|n| Target::Node(n.id))
            .or_else(|| {
                state
                    .bt
                    .iter()
                    .find(|b| b.name.to_lowercase().contains(&needle))
                    .map(|b| Target::BtDev(b.device_id))
            })
    };

    let report = |res: Result<String, String>| match res {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    match args.first().map(String::as_str) {
        None | Some("status") => {
            println!("sinks:");
            for n in &state.outputs {
                let def = if n.is_default { "*" } else { " " };
                println!("  {def} {:>3}  {}  vol {}% {} [{}]", n.id, n.name, n.volume,
                    if n.muted { "muted" } else { "" }, n.state);
            }
            println!("sources:");
            for n in &state.inputs {
                let def = if n.is_default { "*" } else { " " };
                println!("  {def} {:>3}  {}  vol {}% {} [{}]", n.id, n.name, n.volume,
                    if n.muted { "muted" } else { "" }, n.state);
            }
            println!("bluetooth:");
            for b in &state.bt {
                println!("    {:>3}  {}  {}  profile {}", b.device_id, b.name,
                    if b.connected { "connected" } else { "disconnected" }, b.profile);
                for p in &b.profiles {
                    println!("           {} {} {}", p.index, p.name,
                        if p.available { "" } else { "(unavailable)" });
                }
            }
        }
        Some(cmd @ ("select-mic" | "select-speaker")) => {
            let sink = cmd == "select-speaker";
            let needle = args[1..].join(" ");
            let Some(target) = find(sink, &needle) else {
                eprintln!("no device matching '{needle}'");
                std::process::exit(1);
            };
            report(if sink {
                pipewire::select_speaker(target)
            } else {
                pipewire::select_mic(target)
            });
        }
        // Foreground demand-policy harness: set the named device as preferred
        // mic, run the policy, and narrate transitions. Only for debugging —
        // don't run it while a bar with its own policy actor is live.
        Some("watch") => {
            let needle = args[1..].join(" ");
            let Some(target) = find(false, &needle) else {
                eprintln!("no device matching '{needle}'");
                std::process::exit(1);
            };
            report(pipewire::select_mic(target));
            pipewire::start_policy();
            let mut last = String::new();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let s = pipewire::query();
                let demand = s.capture.iter().filter(|c| !c.passive && !c.monitor).count();
                let profile = s
                    .bt
                    .iter()
                    .find(|b| b.name.to_lowercase().contains(&needle.to_lowercase()))
                    .map(|b| b.profile.clone())
                    .unwrap_or_default();
                let line = format!("profile={profile} capture_streams={demand}");
                if line != last {
                    println!("{line}");
                    last = line;
                }
            }
        }
        Some("profile") => {
            let kind = match args.get(1).map(String::as_str) {
                Some("a2dp") => ProfileKind::A2dp,
                Some("hfp") => ProfileKind::Hfp,
                _ => {
                    eprintln!("usage: cyberdeck pw profile <a2dp|hfp> <device name>");
                    std::process::exit(2);
                }
            };
            let needle = args[2..].join(" ").to_lowercase();
            let Some(bt) = state.bt.iter().find(|b| b.name.to_lowercase().contains(&needle)) else {
                eprintln!("no bluetooth device matching '{needle}'");
                std::process::exit(1);
            };
            report(pipewire::switch_profile(bt.device_id, kind));
        }
        Some(other) => {
            eprintln!("unknown pw command '{other}' — try status, select-mic, select-speaker, profile");
            std::process::exit(2);
        }
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
# theme = "classic"         # classic, floating, pills, or transparent
# position = "top"          # top or bottom
# font = "monospace"
# font-size = 14
# gap = 6                   # spacing (px) — default for all padding/margin
# scale = 1.0               # global UI scale
# material = "solid"        # "solid", "glass", or { type = "glass", opacity = 0.3, color = "#222222" }
# color = "#222222"         # fallback color
# radius = 6                # default corner radius
# icon-weight = "light"     # regular, bold, thin, light, fill, or duotone
# [settings.bar]             # overrides (theme sets defaults)
# padding = 6               # or [y, x] or [top, right, bottom, left]
# margin = 6                # or [y, x] or [top, right, bottom, left]
#
# [settings.pill]            # overrides (theme sets defaults)
# padding = 6               # or [y, x] or [top, right, bottom, left]
# radius = 6

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

# [bar.modules.audio]           # speakers, microphones, and audio routing
                                # deps: wireplumber, rnnoise-plugin (optional)

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
                                # deps: swww, xdg-user-dirs
# dir defaults to $(xdg-user-dir PICTURES)/wallpapers
"##;

fn run_module_cmd(args: &[String], json: bool) {
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
        }, json);
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
                    }, json);
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

