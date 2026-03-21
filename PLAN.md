# Retrobar: A Terminal-Aesthetic Wayland Bar

A Wayland layer-shell bar that renders like a terminal — monospace grid, cell-based layout, TUI aesthetic — but is actually a native graphical surface with full control over transparency, sizing, and input. Designed keyboard-first with a fully declarative, Nix-driven module system.

## Core Idea

Instead of running inside a terminal emulator, we *are* the renderer. We maintain a cell grid (like a terminal's internal buffer), lay out content into it, and render that grid to a pixel-backed Wayland surface using a monospace font. The bar is always a single row.

The bar binary is a **generic rendering engine**. It knows how to: manage a Wayland surface, render a cell grid, evaluate Tera templates, manage child processes, and parse JSON. It does *not* know what a workspace is, what a battery is, or how to talk to NetworkManager. All domain logic lives in declarative module definitions that Nix generates.

## Concepts

### Bar States

The bar is always a single row. That row can be in one of three states:

**Passive** — the default. A dense, glanceable display of widgets. No keyboard capture. `keyboard_interactivity: none`.

**Active** — entered via `Super+B`. A fuzzy-filter menu over module names. Typing filters, Enter selects a module and enters its mode. Escape returns to passive. `keyboard_interactivity: on_demand`.

**Mode** — a module's detailed view, rendered inline in the bar's single row. Shows the module's information and responds to its keybindings. Entered by selecting a module from active, clicking a widget, or pressing a dedicated mode key. Escape returns to passive.

```
Passive (1 row: widgets)
    |
    +-- Super+B -> Active (1 row: fuzzy filter)
    |                 +-- select -> Mode (1 row: module view)
    |
    +-- Super+D -> launcher Mode (directly via mode key)
    +-- Super+P -> power Mode (directly via mode key)
    |
    +-- click widget -> that module's Mode
                           |
                           +-- Escape -> Passive
```

### Components

Two component types, plus an event system:

**Widget** — A small component shown in the passive bar. Defined as a Tera template with an optional condition. A module may expose one or more named widgets. The first widget is the `default` and can be referenced by just the module name (e.g. `"workspaces"` instead of `"workspaces:default"`). Clicking any widget opens its module's mode. Widgets cover everything from workspace pills to battery indicators to the clock. A widget may declare a **spotlight**: when a referenced event fires, the widget temporarily replaces all other content in its bar section for a set timeout.

**Mode** — A module's detailed view, rendered in the bar's single row when that module is focused. Declared as a Tera template with keybindings that map to shell commands. A mode may include a **filter** for interactive searching over a list of entries. Every module must define a mode.

**Event** — A named condition evaluated after each state update. Fires on false->true transition (edge-triggered). Can trigger a shell command (`on_trigger`) and/or activate a widget's spotlight. Events enable reactions to state transitions — a threshold being crossed, a new notification arriving, a track changing — without any special-case logic in the bar.

### Component Hierarchy

```
Module (declarative definition)
+-- name: String                 -- REQUIRED -- used in active menu and mode header
+-- source: Source               -- REQUIRED -- how to get data (JSON)
+-- deps: [Derivation]          -- REQUIRED -- runtime dependencies (Nix packages)
+-- mode: ModeDef                -- REQUIRED -- detailed view when focused
+-- widgets: {String: WidgetDef} -- OPTIONAL -- named widgets for passive bar
+-- events: [EventDef]          -- OPTIONAL -- edge-triggered reactions to state
```

## Interaction Model

### Passive

```
+------------------------------------------------------------------------------+
| [1] 2 3 . | hx modules/power.nix   15:42    . .XM4 x > .eduroam .12% .87%  |
+------------------------------------------------------------------------------+
  widgets                           widget              widgets
  left section                      center               right section
```

- Three configurable sections: left, center, right
- Each section contains widgets and optional `|` dividers
- `"*"` in a section collects all widgets not explicitly placed elsewhere — the right section defaults to this
- Widgets range from complex (workspace pills, window title) to compact (battery icon + percentage)
- A widget with a spotlight temporarily replaces its section's content when its event fires
- Click any widget -> enters that module's mode

### Active

```
+------------------------------------------------------------------------------+
| po_         workspaces window time system network power audio ...       2/14  |
+------------------------------------------------------------------------------+
  query                  centered module names (filtered)            match count
```

1. `Super+B` activates — all passive content hidden
2. All module names shown centered
3. Typing filters via fuzzy match, matched chars highlighted
4. Arrow/Tab cycles selection, Enter enters that module's mode
5. Single match with >=2 chars typed -> auto-enter mode
6. Escape -> always returns to passive

### Mode

When a module is selected, its mode takes over the bar's row. The mode template renders the module's detailed information.

#### Navigation Principle

Mode content is divided into **subsections** separated by `|`. The user navigates between subsections with **left/right arrows**. The focused subsection is visually highlighted.

Within a focused subsection, **up/down arrows mutate** the value:
- **Linear values** (volume, brightness): up/down directly increments/decrements
- **Discrete options** (power profile, layout): Enter opens a **selection submode** where the options are laid out inline in the bar, left/right picks one, Enter confirms, Escape cancels

This creates a consistent, discoverable interaction model across all modules — left/right to navigate, up/down to change, Enter for discrete choices.

#### Examples

```
+------------------------------------------------------------------------------+
| power | [......... 87% discharging 4h12m] | 1 power-saver  2 balanced.  3 perf|
+------------------------------------------------------------------------------+
  name    focused subsection (info)            profile subsection
          up/down: n/a (read-only)             Enter: selection submode
```

```
+------------------------------------------------------------------------------+
| audio | [.... 72%] | sink WH-1000XM4 | muted: no                             |
+------------------------------------------------------------------------------+
          focused      sink subsection    toggle
          up/down: +/- Enter: pick sink   Enter: toggle
```

```
+------------------------------------------------------------------------------+
| system | cpu ..... 12%  mem ..... 3.2/16G  swap ... 0.1/8G  52C  load 1.2    |
+------------------------------------------------------------------------------+
```

```
+------------------------------------------------------------------------------+
| workspaces | 1 term.  2 web(3)  3 chat(1)  4 . | o overview  l layout  s snap|
+------------------------------------------------------------------------------+
  name         workspace list with focus/counts    action keys
```

Modules should design their modes to be terse enough to fit in one row by default. If a module has more content than fits, pagination wraps across subsections.

**Modes with a filter** support text input for interactive searching. The launcher module uses this, but any module can — bluetooth device search, notification history, etc.

```
+------------------------------------------------------------------------------+
| launcher | fire_   firefox  firewall-config  firenvim                  3/148  |
+------------------------------------------------------------------------------+
  name       query   matching entries (fuzzy)                         match count
```

Escape always returns to passive from any mode.

## Declarative Module System

### Philosophy

No Rust code is needed to define a module. A module is pure data: where to get JSON, how to template it, what commands to run on keypress. The bar binary is the interpreter; modules are the program.

Modules are defined as Nix attribute sets and compiled into a JSON config that the bar binary reads at startup. Nix gives us: type-checked structure, script derivations for source commands, and the ability to compose modules from the NixOS ecosystem (e.g. referencing `pkgs.niri-msg`, `pkgs.bluetoothctl`).

### Module Definition Schema

```nix
{
  # -- REQUIRED --

  name = "power";

  deps = [ pkgs.upower pkgs.power-profiles-daemon ];

  source = {
    type = "poll";
    command = "${power-stats}/bin/power-stats";
    interval = 10;
  };

  mode = {
    template = "......... {{ capacity }}% {{ status | lower }} {{ time_remaining_secs | human_duration }} | 1 power-saver  2 balanced{{ if profiles.1.active }}.{{ end }}  3 perf";
    actions = {
      "1" = "powerprofilesctl set power-saver";
      "2" = "powerprofilesctl set balanced";
      "3" = "powerprofilesctl set performance";
    };
  };

  # -- OPTIONAL --

  widgets = {
    default = {
      template = "{% if charging %}{{ icon(name='lightning') }}{% elif capacity < 20 %}{{ icon(name='battery-warning') }}{% else %}{{ icon(name='battery-half') }}{% endif %}{{ capacity }}%";
      style = "{{ if capacity < 5 then 'alert' elif capacity < 20 then 'warn' elif charging then 'ok' else 'ok' }}";
    };
    warning = {
      template = "{{ icon(name='warning') }} BATTERY {{ capacity }}%";
      condition = "{{ capacity < 5 and not charging }}";
      style = "alert";
      spotlight = { event = "critical"; timeout = 10; };
    };
  };

  events = [
    { name = "low"; condition = "{{ capacity < 20 and not charging }}"; on_trigger = "notify-send -u normal 'power' 'Battery at {{ capacity }}%'"; }
    { name = "critical"; condition = "{{ capacity < 5 and not charging }}"; on_trigger = "notify-send -u critical 'power' 'Battery critical: {{ capacity }}%'"; }
  ];
}
```

### Source Types

All sources must produce a JSON object. This object becomes the Tera template context for everything in the module — widgets, mode, event conditions.

**poll** — Run a command every `interval` seconds. Parse stdout as JSON.

**subscribe** — Start a long-running process. Each line of stdout is a JSON update that *replaces* the current state.

**file** — Watch a file (or set of files) for changes. Content parsed as JSON. Key = basename of path.

### Tera Templates

The bar embeds a Tera engine. Every template string is evaluated against the module's JSON state.

**Built-in filters and functions**:

```
{{ value | meter(max=100, width=12) }}    -> ............
{{ value | bar(max=4) }}                  -> ....
{{ bytes | human_bytes }}                 -> 3.2G
{{ duration | human_duration }}           -> 4h 12m
{{ value | pad_left(width=6) }}           -> "   87%"
{{ value | color(fg="green") }}           -> styled output
{{ timestamp | format_time(fmt="%H:%M") }}-> 15:42
{{ value | changed }}                     -> true if value differs from previous state
{{ icon(name="battery-half") }}           -> renders Phosphor icon by name
```

**Icons** use Phosphor, a flexible icon family available as a font. The `icon` function maps a standard Phosphor icon name to the corresponding glyph.

The **`changed`** filter is key to the event system. The bar tracks the previous JSON state for each module. When evaluating `{{ x | changed }}`, it compares `x` in the current state against `x` in the previous state, returning true when they differ.

### Mode Definition

A mode is a Tera template that renders the module's detailed view into the bar's single row.

```nix
mode = {
  template = "cpu ..... {{ cpu_percent }}%  mem {{ mem_used_bytes | human_bytes }}/{{ mem_total_bytes | human_bytes }}  {{ temp }}C";
  actions = {
    "r" = "systemctl restart some-service";
  };
};
```

**Modes with pagination** can declare multiple pages. Up/down arrows navigate between them. `template` and `pages` are mutually exclusive.

**Modes with a filter** support interactive text input:

```nix
mode = {
  filter = {
    source = "entries";
    field = "name";
    template = "{{ name }}";
    on_select = "{{ exec }}";
  };
  actions = {};
};
```

### State Management

Each module has exactly one JSON blob as its state.

```
Source command (stdout) -> JSON parse -> Module state -+-- Tera render
                                                      |   +-- widget templates + conditions
                                                      |   +-- mode template
                                                      +-- Event evaluation
                                                          +-- compare conditions against previous state
                                                              +-- on_trigger commands
                                                              +-- spotlight activations
```

The bar retains the previous state for each module. After each state update, it evaluates all event conditions against both the new and previous state to detect false->true transitions.

### Events

Events are named conditions evaluated after each state update. An event **fires** when its condition transitions from false to true — a rising edge.

```nix
events = [
  {
    name = "overheating";
    condition = "{{ temp > 90 }}";
    on_trigger = "notify-send 'system' 'CPU temp: {{ temp }}C'";
  }
];
```

### Spotlight

A widget may declare a **spotlight**. When the referenced event fires, the widget temporarily replaces all other content in its bar section.

```nix
widgets.alert = {
  template = "{{ latest.app }} . {{ latest.summary | truncate(length=50) }}";
  condition = "{{ latest != null }}";
  spotlight = {
    event = "received";
    timeout = 5;
  };
};
```

### Calendar: A Built-in Mode Type

The time module's calendar is the one case that's hard to express as a single-row template. The bar provides a built-in `calendar` mode type.

## Module Catalog

### workspaces
Compositor workspace management and niri controls.

### window
Active window information and controls.

### time
Time display with calendar.

### system
CPU, memory, and thermal monitoring.

### network
Network status and connection info.

### power
Battery and power profile management.

### audio
Volume and sink control via wireplumber.

### bluetooth
Device listing and connection.

### display
Brightness and night light control.

### notifications
Notification history with spotlight alerts.

### storage
Disk usage and I/O monitoring.

### media
MPRIS media player control.

### weather
Weather display with forecast.

### launcher
Application launcher with fuzzy search over desktop entries.

## Architecture

```
+-----------------------------------------------------------+
|                       retrobar                             |
|                  (generic rendering engine)                |
|                                                           |
|  +------------+  +----------+  +-----------------------+  |
|  |  Module     |  |  Tera    |  |   Renderer            |  |
|  |  Loader     |--|  Engine  |--|   (cosmic-text +      |  |
|  |  (JSON cfg) |  |          |  |    tiny-skia +        |  |
|  +------------+  +----------+  |    softbuffer)         |  |
|                                +-----------+------------+  |
|  +----------------------------------------|               |
|  |  Wayland Layer (smithay-client-toolkit) |               |
|  |  +-----------------------------+       |               |
|  |  | Single layer-shell surface  |       |               |
|  |  | Always 1 row, anchored top  |       |               |
|  |  +-----------------------------+       |               |
|  +----------------------------------------+               |
|                                                           |
|  +--------------------------------------------------------|
|  |  State Controller                                      |
|  |  Passive <-> Active <-> Mode                           |
|  |  fuzzy filter, mode keys, spotlight timers             |
|  +--------------------------------------------------------|
|                                                           |
|  +--------------------------------------------------------|
|  |  Process Manager (calloop)                             |
|  |  Spawns source commands, manages lifecycles            |
|  |  poll timers, subscribe stdout watchers, file watchers |
|  +--------------------------------------------------------+
+-----------------------------------------------------------+
```

### What the Rust Binary Does

- Single Wayland layer-shell surface (always 1 row, anchored top)
- Cell grid buffer and rendering (cosmic-text + tiny-skia + softbuffer)
- Config loading: read compiled module definitions (JSON)
- Process manager: spawn poll/subscribe commands, manage lifecycles
- JSON parsing: parse stdout from source commands into serde_json::Value
- Tera engine: compile and evaluate templates against JSON state
- Built-in filters: `meter`, `bar`, `human_bytes`, `human_duration`, `changed`, `icon`, etc.
- Event system: track previous state, evaluate conditions, detect false->true edges, spawn on_trigger commands
- Spotlight: temporarily replace section content when an event fires, restore after timeout
- Layout engine: passive sections with widget placement and `*` collection
- State controller: passive/active/mode state machine, keyboard routing, mode keys
- Filter UX: fuzzy matching, selection, query input (reused for active menu and filterable modes)
- Built-in calendar mode type
- Pagination (up/down arrows for multi-page modes)

### What the Rust Binary Does NOT Do

- Know what a workspace, battery, or SSID is
- Import domain-specific crates
- Contain any module-specific logic
- Need recompilation to add new modules

### Crate Selection

| Purpose | Crate |
|---|---|
| Wayland client | `smithay-client-toolkit` |
| Text shaping | `cosmic-text` |
| CPU rendering | `tiny-skia` + `softbuffer` |
| Templating | `tera` |
| JSON | `serde_json` |
| Event loop | `calloop` (via SCTK) |
| Process management | `std::process` + calloop |
| Icons | Phosphor Icons (font) |

## Nix Integration

The NixOS module provides:

```nix
{
  services.retrobar = {
    enable = true;
    settings = {
      position = "top";
      font = "JetBrains Mono";
      font_size = 14;
      icon_weight = "light";
      activate_key = "Super+B";
      background = { color = "#1e1e2e"; opacity = 0.7; blur = true; };
      passive.left   = ["workspaces" "|" "window"];
      passive.center = ["time" "notifications:alert"];
      passive.right  = ["*"];
      mode_keys = {
        "Super+D" = "launcher";
        "Super+P" = "power";
      };
    };
    modules = { ... };
  };
}
```

Nix evaluates all module definitions, builds helper scripts as derivations, and produces a single JSON config. Each module declares its own `deps` — the NixOS module collects all deps and ensures they're available at runtime.
