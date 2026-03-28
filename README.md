> [!WARNING]  
> Experimental, vibe coded software. Works on my machine, but I'm no Rust/Wayland dev.

# `> cyberdeck`

A desktop shell for cyberpunks, on Niri on NixOS.

Philosophy:

- *streamlined* - all you need, nothing you don't
- *ergonomic* - first-class keyboard support
- *focused* - stays out of your way
- *turnkey* - doesn't take weeks to config

![desktop](screenshots/desktop.png)

When **idle** the bar tries to show as little clutter as possible:

![bar idle](screenshots/bar-idle.png)

When **activated** you get a module menu:

![bar with badges](screenshots/bar-badges.png)

This is also a **launcher** and will find apps when you type:

![bar launcher](screenshots/bar-launcher.png)

## Modules

| Module | Description | Dependencies |
|--------|-------------|-------------|
| calendar | Clock, weekly/monthly/yearly calendar navigation | — |
| window | Active window title | — |
| workspaces | Workspace indicators (hexagon toasts on switch) | — |
| system | CPU, memory, temperature, uptime | — |
| storage | Disk usage, alert when >80% full | — |
| notifications | Notification count, dismiss, clear | libnotify |
| outputs | Speaker volume & mute, device switching | wireplumber |
| inputs | Microphone volume & mute, denoise toggle | wireplumber, rnnoise |
| network | WiFi SSID/signal/IP, connect, scan | NetworkManager |
| bluetooth | Paired devices, connect/disconnect, scan | bluez |
| brightness | Screen brightness control | brightnessctl |
| media | MPRIS now playing, play/pause/skip | playerctl |
| session | Battery, suspend, shutdown, reboot, logout | upower |
| profiles | Power profiles (saver/balanced/performance) | power-profiles-daemon |
| weather | Temperature, humidity, conditions from wttr.in | curl |
| snip | Region/screen screenshot, screen recording | grim, slurp, wl-clipboard, wl-screenrec |
| wallpaper | Wallpaper cycling via swww | swww |
| keyboard | Layout indicator, cycle layouts | swaymsg |
| clipboard | Clipboard history, paste, clear | wl-clipboard, cliphist |
| mounts | Removable device status | udisks2 |

### Todo

- [ ] monitor management
- [ ] emoji module
- [ ] lockscreen
- [ ] greeter

Vision: all you need to work is Niri + Cyberdeck.

## Getting started

### Prerequisites

- A Wayland compositor (Niri officially supported, others may work)
- Linux with systemd

### Install (any distro)

```sh
curl -fsSL https://raw.githubusercontent.com/ixxie/cyberdeck/main/install.sh | sh
```

This installs the binary, Phosphor icons, a systemd service, and a default config. Then:

```sh
deck init            # create ~/.config/cyberdeck/config.toml (if not already present)
# edit config.toml to enable modules, install their deps
systemctl --user enable --now cyberdeck
```

### Install (NixOS)

Add cyberdeck to your flake inputs:

```nix
# flake.nix
{
  inputs.cyberdeck.url = "github:ixxie/cyberdeck";
}
```

Import the NixOS module and enable modules:

```nix
# configuration.nix
{ inputs, ... }: {
  imports = [ inputs.cyberdeck.nixosModules.default ];

  services.cyberdeck = {
    enable = true;
    settings = {
      font = "MonaspiceKr Nerd Font";
      layout = "pills";
      gap = 8;
    };
    mods = {
      calendar.enable = true;
      workspaces.enable = true;
      window.enable = true;
      network.enable = true;
      outputs.enable = true;
      inputs.enable = true;
      bluetooth.enable = true;
      brightness.enable = true;
      session.enable = true;
      system.enable = true;
      notifications.enable = true;
      media.enable = true;
      weather = {
        enable = true;
        location = "London";
      };
      storage.enable = true;
      snip.enable = true;
      wallpaper = {
        enable = true;
        dir = "~/Pictures/wallpapers";
      };
    };
  };
}
```

Rebuild your system and the bar will start automatically.

### Keybinding

Add a toggle keybinding in your compositor config. For niri:

```nix
"Mod+Space".action.spawn = ["cyberdeck" "launcher"];
```

## Configuration

For TOML config (non-NixOS), run `deck init` to generate a commented template at `~/.config/cyberdeck/config.toml`.

### Settings

```toml
[settings]
position = "top"              # top or bottom
font = "monospace"            # any monospace font
font-size = 14
layout = "pills"              # classic, floating, pills, or transparent
gap = 6                       # spacing between elements (px)
scale = 1.0                   # global UI scale
icon-weight = "light"         # regular, bold, thin, light, fill, or duotone

[settings.theme]
color = "#222222"
opacity = 0.8
radius = 6
padding = 6

[settings.theme.track]        # bar background overrides (omit for no track)
# opacity = 1.0

[settings.theme.pill]         # pill overrides
# radius = 8

[settings.monitors.DP-1]
scale = 0.8
```

### Layouts

| Layout | Description |
|--------|-------------|
| classic | Solid bar attached to screen edge, no margin |
| floating | Solid bar with margin, rounded corners |
| pills | Transparent bar, solid floating pills (default) |
| transparent | Everything transparent |

The `gap` parameter controls all visual spacing: margin, track padding, and inter-pill gaps.

### Modules

Each module can define:

- **`icon`** — module icon shown in the breadcrumb when focused
- **`badges`** — named map of badges shown on the root bar (see below)
- **`widget`** — content shown when the module is focused
- **`hooks`** — trigger toasts or badge overrides on state changes
- **`key-hints`** — keyboard shortcuts shown on the right when focused
- **`type`** — set to `"calendar"`, `"bluetooth"`, `"wallpaper"`, or `"actions"` for interactive modules with rich keyboard navigation

### Badge system

Badges are the icons/text shown on the root bar for each module. Each module can define multiple named badges:

```nix
badges = {
  # Always-visible badge (no condition)
  battery = {
    template = "{{ battery_pct }}%";
  };
  # Alert badge (only shown when condition is true)
  low-battery = {
    template = "{{ \"battery-warning\" | icon }} LOW";
    condition = "{{ battery_pct < 20 }}";
    highlight = "{{ \"battery-warning\" | icon }} {{ battery_pct }}%";
  };
};
```

- **Badge** — always visible on the root bar (no `condition`)
- **Alert badge** — only appears when its `condition` template evaluates truthy

Each badge has:
- `template` — Tera template for the badge content
- `condition` (optional) — if present, badge only shows when this renders truthy
- `highlight` (optional) — alternative template used when a hook forces the badge visible
- `icon-scale` (optional) — scale multiplier for icons within this badge

### Hooks

Hooks trigger actions when module state changes:

```nix
hooks = [
  {
    condition = "{{ changed(key=\"muted\") and muted }}";
    action = "show-badge:muted";  # force-show a specific badge
    timeout = 3;
  }
  {
    condition = "{{ changed(key=\"volume\") }}";
    action = "toast";  # show a transient notification
    timeout = 3;
  }
];
```

Hook actions:
- `"toast"` — show a transient message on the left of the bar
- `"show-badge:<badge-name>"` — force-show a specific badge temporarily (overrides condition)
- Any other string — executed as a shell command

### IPC

The bar exposes a Unix socket at `$XDG_RUNTIME_DIR/cyberdeck.sock`. Commands:

```sh
cyberdeck launcher       # toggle the launcher
cyberdeck push <module>  # navigate to a module
cyberdeck run <mod> <key> # run a module's key-hint action
cyberdeck state          # get current navigation state
cyberdeck dismiss        # reset to root
```

Use `cyberdeck run` in compositor keybindings for volume/brightness control with instant feedback.

## Developing

### Source layout

```
src/
  main.rs       — entry point, CLI parsing, event loop
  bar.rs        — Wayland integration, BarApp struct, delegates
  nav.rs        — navigation state machine, input handling
  view.rs       — layout composition, text search
  config.rs     — configuration schema (JSON deserialization)
  source.rs     — data source management (poll, subscribe, file, native)
  template.rs   — Tera template engine, filters, functions
  layout.rs     — flex layout engine, hit area tracking
  render.rs     — text rendering (cosmic-text), icon compositing
  icons.rs      — SVG icon loading (Phosphor icons)
  ipc.rs        — Unix socket IPC protocol
  color.rs      — RGBA color type
  mods/
    mod.rs      — InteractiveModule trait, native source registry
    calendar.rs — clock + calendar navigation
    launcher.rs — desktop application scanner
    wallpaper.rs — wallpaper management via swww
    ...         — audio, bluetooth, system, etc.
```

### Key concepts

- **Module** (`ModuleDef`) — a data source + display configuration. Defined in `bar.modules`.
- **Badge** (`BadgeDef`) — a small status element on the root bar. Each module can have multiple named badges.
- **Widget** (`WidgetDef`) — expanded content shown when a module is focused.
- **InteractiveModule** — trait for modules with rich keyboard-driven navigation (calendar, bluetooth, wallpaper, actions).
- **Source** — how a module gets data: `poll` (periodic command), `subscribe` (streaming), `file` (JSON files), or `native` (Rust implementation).
- **Toast** — transient notification shown on the left of the bar.

### Data flow

1. Sources poll/stream data in background threads
2. JSON updates sent to main thread via calloop channels
3. `process_hooks()` evaluates hook conditions on state changes
4. `maybe_redraw()` renders all bar instances via the template engine
5. Tera templates produce text + icon sequences
6. Flex layout positions three groups (left, center, right)
7. Renderer shapes text (cosmic-text) and composites icons (tiny-skia)
8. Output copied to Wayland shared memory buffer

### Building

```sh
# NixOS
nix build            # build the package
nix develop          # dev shell with cargo, rust-analyzer

# Other distros (needs wayland, xkbcommon, fontconfig, freetype dev headers)
cargo build --release
```

## License

MIT
