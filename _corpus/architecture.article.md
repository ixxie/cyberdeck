---
title: Architecture
---

# Architecture

cyberdeck is one binary in two roles. Run with no subcommand it is the
**daemon**: it opens a layer-shell bar on every output, runs the module
sources, owns the notification bus name and the audio backend, and listens
on a Unix socket. Run with a subcommand it is a **one-shot client**: it
executes a module action itself or forwards a request to the daemon over the
socket, then exits.

```strata
title = "cyberdeck"

[palette]
sys = "#8a8f98"
backend = "#3b6ea5"
runtime = "#6b8e23"
ui = "#b5651d"

[[layers]]
name = "System"
modules = [
  { id = "niri", label = "niri IPC", icon = "squares-four", color = "sys" },
  { id = "pw", label = "PipeWire", icon = "broadcast", color = "sys" },
  { id = "dbus", label = "D-Bus: BlueZ, NM, logind", icon = "share-network", color = "sys" },
  { id = "kernel", label = "rfkill, sysfs", icon = "cpu", color = "sys" },
  { id = "tools", label = "CLI tools", icon = "terminal", color = "sys" },
]

[[layers]]
name = "Backends"
modules = [
  { id = "native", label = "native mods", icon = "puzzle-piece", color = "backend" },
  { id = "pwb", label = "pipewire backend", icon = "pulse", color = "backend" },
  { id = "cmd", label = "poll / subscribe", icon = "terminal-window", color = "backend" },
]

[[layers]]
name = "Runtime"
modules = [
  { id = "sources", label = "sources + state", icon = "queue", color = "runtime" },
  { id = "tera", label = "Tera templates", icon = "file-code", color = "runtime" },
  { id = "actions", label = "action registry", icon = "lightning", color = "runtime" },
]

[[layers]]
name = "UI"
modules = [
  { id = "bar", label = "bar + nav", icon = "layout", color = "ui" },
  { id = "cli", label = "CLI + IPC socket", icon = "plugs-connected", color = "ui" },
]

[[edges]]
from = "niri"
to = "native"
[[edges]]
from = "dbus"
to = "native"
[[edges]]
from = "kernel"
to = "native"
[[edges]]
from = "pw"
to = "pwb"
[[edges]]
from = "tools"
to = "cmd"
[[edges]]
from = "native"
to = "sources"
label = "JSON"
[[edges]]
from = "pwb"
to = "sources"
[[edges]]
from = "cmd"
to = "sources"
[[edges]]
from = "sources"
to = "bar"
[[edges]]
from = "tera"
to = "bar"
[[edges]]
from = "actions"
to = "bar"
[[edges]]
from = "actions"
to = "cli"
```

Read it bottom to top. The **system** layer is what cyberdeck talks to and
never replaces: Niri's IPC (`niri msg`, its event stream), the PipeWire
graph, BlueZ / NetworkManager / logind / power-profiles-daemon over D-Bus,
the kernel's rfkill device and sysfs, and ordinary command-line tools
(`wpctl`, `playerctl`, `grim`, `nmcli`…). **Backends** turn those into JSON:
native Rust module functions, the PipeWire backend, and shell commands run
by the source manager. The **runtime** holds each module's latest JSON, runs
it through Tera templates, and resolves actions. The **UI** is the bar and
its navigation, plus the CLI and the socket that reach the same actions from
outside.

## Source layout

```
build.rs            embeds mods/*.mod.toml into the binary; generates CLI metadata
mods/<id>.mod.toml  one manifest per builtin module
nix/module.nix      the NixOS module; nix/mods/<id>.nix per-module Nix defaults
src/
  main.rs           CLI dispatch, or the daemon's event loop
  cli.rs            clap CLI (built from the manifests), `init`, `pw`, module actions
  ipc.rs            the Unix socket: request types, dispatch into the bar
  config.rs         settings, module schema (ModuleDef), config loading and merge
  modlib.rs         the builtin manifests, parsed; template compile tests
  source.rs         poll / subscribe / file / native sources, per-module state
  template.rs       the Tera engine, its filters and the changed() function
  actions.rs        native actions executable outside the bar
  bar.rs            BarApp: Wayland state, one BarInstance per output, toasts, hooks
  nav.rs            navigation stack, text search, key handling
  view.rs           composes the bar content for the current nav state
  layout.rs         flex layout of spans (taffy) and hit areas
  render.rs         text shaping and measuring (cosmic-text)
  gpu.rs, gpu_render.rs   vello scene on wgpu, read back into the SHM buffer
  icons.rs, appicon.rs    Phosphor icons as private-use glyphs; app icons
  notifications.rs  the org.freedesktop.Notifications daemon
  pipewire.rs       native PipeWire backend: mirror, selection, façade, policy
  watchdog.rs       bluetooth audio watchdog
  mods/             native module code, one file per module (+ mod.rs registry)
```

## The daemon

`main` loads the config, connects to Wayland, and builds a calloop event
loop. Into it go the Wayland queue, the IPC listener, and one channel per
module source. It starts three background actors — the PipeWire monitor
thread, the PipeWire demand policy thread, and the bluetooth watchdog — then
constructs `BarApp` and loops: dispatch events for up to 16 ms, then
`maybe_redraw()`. `SIGINT`/`SIGTERM` end the loop and remove the socket.

`BarApp` implements smithay-client-toolkit's compositor, output, seat,
keyboard, pointer, layer-shell and SHM handlers. Each output gets a
`BarInstance`: a `Layer::Top` surface anchored to the top or bottom edge,
with its own scale (per-monitor `settings.monitors.<name>.scale`). Keyboard
interactivity is `None` at rest and `Exclusive` while the user navigates, so
the bar only takes the keyboard when summoned.

### Drawing

`maybe_redraw` runs when a source marked the state dirty, the navigation
changed, or an animation (toast fade, location dim) is in flight. For each
bar:

1. `view.rs` turns the nav state and module data into a `BarContent`: three
   groups of *spans* (pills) — left, centre, right — each a row of *elems*
   (text runs, icons, app-icon pixmaps).
2. `layout.rs` measures them (`render.rs`, cosmic-text) and places them with
   a flex layout (taffy), paginating with carets when they overflow, and
   records hit areas for the pointer.
3. `gpu_render.rs` builds a vello scene (pills, track, text, tinted icons),
   `gpu.rs` renders it on wgpu and reads the pixels back into a Wayland SHM
   buffer, which is attached and committed.

Icons are Phosphor SVGs from `settings.icons-dir` (the NixOS module points
it at the `phosphor-icons` flake input). `icons.rs` gives every icon name a
stable private-use codepoint, so templates emit icons as ordinary characters
(`{{ "wifi-high" | icon }}`) and the renderer swaps them for rasterised SVGs.

At rest the bar shows the launcher icon on the left, the *location* in the
centre (workspace number and focused window title, bright on change and
dimming after two seconds), and the modules' badges and per-app notification
icons on the right. Toasts and spotlights temporarily take the centre.

### Navigation

`NavState` is a stack of module ids plus a mode: `Visual` (a module's widget
or deep view) or `Text` (a search query). `cyberdeck launcher` opens text
mode; typing filters modules and desktop apps (`mods/launcher.rs` scans
`.desktop` files), a unique match of two or more characters is entered
automatically, `Enter` opens a module or launches an app through
`niri msg action spawn-sh`, and `Esc` goes back. Inside a module, keys are
matched against its actions' `key`s and key hints; an *interactive* module
gets them first through its `handle_key`.

## Modules

A module is data plus presentation plus actions. Its **manifest** is a
TOML file, `mods/<id>.mod.toml`, deserialised into `ModuleDef`:

| field | meaning |
|---|---|
| `name`, `description`, `icon` | identity; the icon shows in the breadcrumb |
| `type` | selects an interactive deep view implemented in Rust (`audio`, `bluetooth`, `calendar`, `wallpaper`, `actions`, …) |
| `[source]` | where the data comes from (below) |
| `[badges.<name>]` | root-bar elements: `template`, optional `condition`, optional `highlight` |
| `[widget]` | the text shown when the module is focused; `|||` splits it into elems |
| `[[hooks]]` | `condition` + `action` (`toast`, `show-badge:<name>`, `spotlight`, or a shell command) + `timeout` |
| `[[actions]]` | `name`, `run` (a shell command, or `"native"`), `label`, optional `key` and `icon` |
| `[[key-hints]]` | keys shown on the right while focused |
| anything else | a *param*, passed to the source and overridable from config |

`build.rs` reads every manifest at compile time: it embeds them with
`include_str!` (`BUILTIN_MODS`, parsed by `modlib.rs`) and generates
`MOD_META` — ids, names and action names — from which `cli.rs` builds a clap
subcommand per module. The binary therefore carries its modules; the manifests
are not read at runtime.

### Configuration and merge

The daemon reads `--config`, else `~/.config/cyberdeck/config.toml`, else
`config.json` (what the NixOS module writes). The runtime config is sparse:
`settings` plus `bar.modules.<id>` overrides. `Config::load` serialises each
builtin `ModuleDef` to JSON, deep-merges the override over it, and
deserialises the result. Listing a module enables it; an empty module list
enables all builtins. An id that is not a builtin is parsed as a complete
user-defined module, so a module that only needs shell commands and
templates can live entirely in config. Params are also written to
`~/.cache/cyberdeck/params/<id>/params.json` for scripts to read.

### Sources

`SourceDef` has four kinds:

- `poll` — run a command every `interval` seconds, parse stdout as JSON;
- `subscribe` — run a long-lived command, each stdout line is a JSON update;
- `file` — read JSON files on an interval;
- `native` — call Rust. `mods/mod.rs` maps the `kind` to a `poll` function
  and, for event-driven modules, a `subscribe` function.

Native sources run on their own threads and send `(module id, JSON)` over a
calloop channel; the main thread stores it in the module's `ModuleState`
and marks it dirty. A subscribe thread seeds with a poll, then subscribes
(to the Niri event stream, D-Bus signals, `/dev/rfkill`, the PipeWire
mirror's generation counter…). If the subscription returns — the service
went away — the harness waits five seconds, re-polls and re-subscribes;
only a closed channel (the bar is gone) ends it. Poll threads can be
*nudged* to refresh early after an action.

### Templates and hooks

Every template — badges, conditions, highlights, widgets, labels, hook
conditions and actions — is registered in one Tera instance at startup,
named `<id>.badge.<name>`, `<id>.widget` and so on, and rendered against the
module's JSON (plus `__output`, the output name, for per-monitor badges). A
condition is true unless it renders empty, `false` or `0`. The engine adds
filters — `icon`, `meter`, `bar`, `sparkline`, `human_bytes`,
`human_duration`, `pad_left`, `pad_right`, `color`, `dim` — and the
function `changed(key=…)`, true when that key differs from the previous
update. After each update `process_hooks` evaluates the module's hook
conditions against the new and previous data and fires their actions.

The syntax is Tera's, not Jinja's: Jinja-only constructs such as
`selectattr` fail to compile, and a template that fails to compile is only
logged — the badge silently never appears. `modlib::tests::builtin_templates_compile`
compiles every builtin template so that fails the test suite instead.

### Interactive modules

A manifest with a `type` gets a Rust deep view from
`mods::create_interactive`: a `Box<dyn InteractiveModule>` that renders the
centre pills, reports a cursor and tabs, handles keys (returning `Ignored`,
`Handled`, `Action` or `Dismiss`), and executes named actions. Audio
(speakers/microphones tabs), bluetooth, network, airplane, notifications,
calendar, keyboard, media, wallpaper, recording and the generic `actions`
palette are interactive.

### Actions

Every way into a module — a key in the bar, `cyberdeck <module> <action>`,
an IPC request — resolves through the module's `[[actions]]`. A shell action
runs through `sh -c`; the CLI then tells the daemon to toast its label.
A `native` action is matched in `actions::exec_native`, which runs it *in
the CLI process* where it can (bluetooth, airplane, session, profiles,
recording start/stop, denoise, wallpaper, workspace rename/focus), so the
command works even with no bar running; actions that need the bar's state
return `BarOnly` and are forwarded over the socket.

## IPC

The socket is `$XDG_RUNTIME_DIR/cyberdeck.sock`, mode 0600. A request is
one line of JSON tagged by `cmd` — `launcher`, `dismiss`, `push`, `pop`,
`navigate`, `state`, `type`, `key`, `theme`, `action`, `event` — and the
reply is one JSON object (`ok`, and `error`, `path`, `mode`, `query`,
`selected` as they apply). `key` and `type` inject synthetic keyboard input,
which is how tests and scripts can drive the navigation.

## Services the daemon owns

- **Notifications.** The daemon claims `org.freedesktop.Notifications` on
  the session bus (zbus) and renders notifications as toasts and per-app
  icons, with grouping and per-app mute. No other notification daemon can
  run beside it.
- **PipeWire backend.** A thread runs a PipeWire main loop and mirrors
  nodes, devices, defaults and streams into a shared `PwState`; reads are
  free, mutations go over a command channel as SPA pods. On top sit the
  selection orchestrator (`select_mic`, `select_speaker`,
  `switch_profile`), the per-device **façade** (a persistent virtual
  sink/source per bluetooth headset, run as a `pipewire -c` loopback child,
  so profile flips never interrupt apps), and the **demand policy** thread
  that engages the headset profile while something captures from the mic
  and restores A2DP ten seconds after the last capture ends.
  `cyberdeck pw status | select-mic | select-speaker | profile | watch`
  drives it standalone.
- **Watchdog.** Every three seconds it looks for bluez nodes stuck in
  `error` and for connected headsets with no headset profile registered, and
  recovers with a bounded disconnect/connect cycle (debounce, 60 s cooldown,
  two attempts per episode).

## Packaging

The flake builds with crane on the fenix stable toolchain, splitting the
dependency build for caching, and patches the dlopen'd Wayland, xkbcommon,
Vulkan and GL libraries onto the binary's rpath. `nixosModules.default` is
described in [the overview](corpus:article/overview); `install.sh` and
`cyberdeck.service` are the non-NixOS path. Adding a module is walked
through in [the guide](corpus:guide/adding-a-module).
