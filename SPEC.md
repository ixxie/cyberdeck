# Retrobar Specification

A language-agnostic specification for a minimal, recursively composable Wayland status bar that renders to a single-row character grid with SVG icon support, transparent backgrounds, and a modal navigation system.

---

## Table of Contents

1. [Overview and Goals](#1-overview-and-goals)
2. [Architecture](#2-architecture)
3. [Configuration](#3-configuration)
4. [State Machine](#4-state-machine)
5. [Source System](#5-source-system)
6. [Template Engine](#6-template-engine)
7. [Rendering Pipeline](#7-rendering-pipeline)
8. [IPC Protocol](#8-ipc-protocol)
9. [Definition of Done](#9-definition-of-done)
- [Appendix A: Design Decision Rationale](#appendix-a-design-decision-rationale)
- [Appendix B: Default Module Tree](#appendix-b-default-module-tree)

---

## 1. Overview and Goals

### 1.1 Problem Statement

Wayland status bars (waybar, yambar, eww) are either over-engineered widget toolkits or rigid format strings. They conflate data acquisition, layout, and rendering into monolithic configs, making it hard to add a new data source without touching the bar itself. None provide a keyboard-driven mode system for interacting with module data beyond passive display, and none support recursive composition — drilling into nested module hierarchies inline.

Retrobar separates these concerns: external scripts produce JSON, Tera templates format it, and a fixed-cell grid renders it. Modules form a recursive tree where each node is either a sub-bar (containing further modules) or a command (executing an action). All control flows through a Unix socket IPC.

### 1.2 Design Principles

**Recursive composable modules.** The bar is modeled as a tree of *modules* and module can either contain *submodules* or execute a *command* but never both.

**Modal UX.** The bar is modal meaning it switches between modes. In *visual* mode, *widgets* is rendered in the left, center or right *sections* of the bar. In *text* mode, a list of sub,modules are aligned to the center of the bar, and the search query is shown on the left while the right shows special shortcuts and modes, for example a recursive search mode.

**Scriptable state.** The layout of mode can be controlled via *render* scripts. These can produce a template or data structure that drives the layout of the mode. The format depends on the mode.

**Nix native configs.** Configuration is through Nix. No alternative is offered and full advantage of Nix's power can be used.

**CLI with IPC over signals.** A friendly CLI allows controlling the bar. All external control (activate, dismiss, mode entry, key injection) goes through a Unix domain socket with a line-based JSON protocol. No SIGUSR hacks, no pid files.

**Grid-native rendering.** The bar is a single-row fixed-width character grid. Every glyph occupies exactly one cell. Layout is column arithmetic, not floating-point geometry. This makes hit-testing trivial and rendering deterministic.

**Scripts are the data layer.** The bar never parses `/proc`, calls D-Bus, or opens sockets to services. External scripts (shell, Python, etc.) produce JSON on stdout. The bar consumes it. This makes modules testable in isolation and language-agnostic.

**Contrast-adaptive rendering.** Text and icons render with an automatic luminance-inverted outline, ensuring readability on any wallpaper at any background opacity, including fully transparent.

### 1.3 Reference Projects

- **waybar** -- GTK-based Wayland bar with extensive built-in modules. Study: module config schema.
- **eww** -- Lisp-configured widget system. Study: reactive data binding model.
- **dmenu/rofi** -- Fuzzy selectors. Study: keyboard-driven item selection UX.

---

## 2. Architecture

### 2.1 Component Diagram

```
+-----------------------------------------------------------+
|  retrobar binary                                          |
|                                                           |
|  +-------------+  +-------------+  +------------------+  |
|  | SourceMgr   |  | TemplateEng |  | Renderer         |  |
|  | (poll/sub/  |  | (Tera +     |  | (cosmic-text +   |  |
|  |  file)      |  |  filters)   |  |  tiny-skia +     |  |
|  +------+------+  +------+------+  |  resvg icons)    |  |
|         |                |         +--------+---------+  |
|         v                v                  ^            |
|  +------+----------------+------+           |            |
|  |        BarApp (state)        +-----------+            |
|  |  module_tree, nav_state,     |                        |
|  |  module_states, spotlights   |                        |
|  +------+-----------------------+                        |
|         |         ^                                      |
|         v         |                                      |
|  +------+---------+------+    +--------------------+     |
|  | Wayland Layer Surface |    | IPC Listener       |     |
|  | (SCTK + calloop)      |    | (Unix socket)      |     |
|  +-----------------------+    +--------------------+     |
+-----------------------------------------------------------+
```

### 2.2 Module Tree

At startup, the bar resolves the config into a recursive module tree. The root module is a bar-module whose groups define the initial visual layout. Each bar-module can contain children that are themselves bar-modules or command-modules.

Module IDs use dot-separated paths reflecting tree position: `"power"`, `"power.profiles"`, `"power.profiles.saver"`. The tree is walked recursively to register all sources and compile all templates.

### 2.3 Event Loop

The application uses a single-threaded `calloop` event loop. All I/O sources register as calloop event sources:

- **WaylandSource** -- Wayland protocol events (configure, keyboard, pointer, frame)
- **Timer sources** -- Poll and file source intervals, spotlight timeouts
- **Generic FD sources** -- Subscribe source stdout pipes, IPC listener socket
- **BarApp** -- The shared mutable state passed as calloop's `D` parameter

Source registration walks the module tree recursively, registering calloop sources for every module that declares a `source`. Each source is keyed by its dot-path ID.

The main loop dispatches at 16ms intervals. After each dispatch, `maybe_redraw()` checks the dirty flag and redraws all configured bar instances if needed.

### 2.4 Multi-Output

The bar creates one `BarInstance` per Wayland output. Each instance has its own layer surface, slot pool, grid, icon set, and scale factor. All instances share the same `BarApp` state and render the same content (except output-filtered widgets using `__output`).

---

## 3. Configuration

### 3.1 File Location

Config loads from `--config <path>` if provided, otherwise `$XDG_CONFIG_HOME/retrobar/config.json` (defaulting to `~/.config/retrobar/config.json`).

### 3.2 Top-Level Structure

```
RECORD Config:
    settings : Settings           -- global bar settings
    bar      : BarModuleDef       -- root bar (no widget, just groups + children)
```

The root `bar` is a bar-module that defines the top-level visual layout. It has no widget (it *is* the bar) and its groups determine which children appear where.

### 3.3 Settings

| Key             | Type    | Default       | Description                                  |
|-----------------|---------|---------------|----------------------------------------------|
| `position`      | String  | `"top"`       | Bar anchor: `"top"` or `"bottom"`            |
| `font`          | String  | `"monospace"` | Font family name                             |
| `font-size`     | Float   | `14.0`        | Font size in logical pixels                  |
| `padding`       | Float   | `6.0`         | Padding around the grid in logical pixels    |
| `background`    | Record  | see below     | Background color and opacity                 |
| `icons-dir`     | String  | unset         | Path to Phosphor Icons SVG directory         |
| `icon-weight`   | String  | `"light"`     | Default icon weight                          |
| `toggle-key`    | String  | `"Super+B"`   | Reserved for future keybind integration      |

**Background:**

| Key       | Type    | Default     | Description                              |
|-----------|---------|-------------|------------------------------------------|
| `color`   | String  | `"#ffffff"` | Hex color string (`#RRGGBB` or `#RRGGBBAA`) |
| `opacity` | Float   | `0.0`       | Background opacity (0.0 = transparent, 1.0 = opaque) |
| `blur`    | Boolean | `false`     | Reserved for compositor blur integration |

### 3.4 Module Definitions

Modules come in two kinds, distinguished by the presence of `groups` (bar-module) or `command` (command-module).

```
TYPE ModuleDef = BarModuleDef | CommandModuleDef
```

#### 3.4.1 Bar Module

A bar-module contains submodules arranged in groups. Clicking its widget navigates *into* it, replacing the bar content with its own groups.

```
RECORD BarModuleDef:
    name      : String                    -- human-readable display name
    type      : String | None             -- special type (e.g. "calendar"), None for normal
    source    : SourceDef | None          -- data acquisition (None for root or static modules)
    widget    : WidgetDef | None          -- visual representation (None for root)
    label     : LabelDef | None           -- hover label
    groups    : List<GroupDef>           -- ordered widget groups, space-between layout
    children  : Map<String, ModuleDef>   -- child modules keyed by local ID
    render    : RenderDef | None          -- dynamic submodule generation
    text_mode : TextModeDef | None        -- text-mode configuration
    events    : List<EventDef>            -- reactive event triggers (default: empty)
```

#### 3.4.2 Command Module

A command-module is a leaf. Clicking its widget (or selecting it in text mode) executes a shell command.

```
RECORD CommandModuleDef:
    name      : String                    -- human-readable display name
    source    : SourceDef | None          -- data acquisition
    widget    : WidgetDef | None          -- visual representation
    label     : LabelDef | None           -- hover label
    command   : String                    -- shell command to execute on activation
    events    : List<EventDef>            -- reactive event triggers (default: empty)
```

#### 3.4.3 Group Layout

Groups are ordered collections of widgets distributed across the bar with equal spacing (space-between). Each group is internally a fixed character grid.

```
RECORD GroupDef:
    modules : List<String>       -- child IDs to render in this group
    max     : Integer | None     -- max columns (None = no limit)
```

Each entry in `modules` is one of:
- `"child_id"` -- renders the child module's widget
- `"|"` -- renders a vertical divider character

Content exceeding `max` is truncated with `…` at the boundary. Groups without `max` grow freely.

**Layout algorithm:**

1. Render each group's widget content into cells.
2. Truncate any group exceeding its `max`.
3. Compute `gap = (grid_columns - sum(group_widths)) / (num_groups - 1)`.
4. Place groups left-to-right with `gap` spacing.

**Breadcrumb injection:** The home button is always prepended to the first group. At root depth, it is just the home icon. At deeper levels, the full breadcrumb path (`home > power > profiles`) is prepended. Each breadcrumb segment is clickable — clicking it navigates to that level. See [7.3 Breadcrumb](#73-breadcrumb) for rendering details.

### 3.5 Widget Definition

Each module has at most one widget (unlike the old multi-widget-per-module model).

```
RECORD WidgetDef:
    template  : String                     -- Tera template for visual display
    condition : String | None              -- Tera condition (truthy = show)
    style     : String | None              -- reserved for semantic styling
    spotlight : SpotlightDef | None        -- event-triggered takeover
```

```
RECORD SpotlightDef:
    event   : String           -- event name that triggers this spotlight
    timeout : Integer = 5      -- seconds before spotlight expires
```

### 3.6 Label Definition

Labels provide context on hover. When the pointer enters a widget's hit area, the label renders near the widget and other widgets fade to low opacity.

```
RECORD LabelDef:
    template : String          -- Tera template for label text
```

Label positioning is automatic: the bar chooses the side with more available space, offset toward the cursor direction. Labels disappear on pointer leave.

### 3.7 Text Mode Definition

Configures how a bar-module behaves in text mode. If omitted, text mode lists children by name with no shortcuts.

```
RECORD TextModeDef:
    shortcuts : Map<String, String>    -- label -> child_id, shown on right side
```

Shortcuts are displayed as right-aligned hints (e.g., `r recursive`) that activate specific children directly.

### 3.8 Render Definition

The render API lets a source script drive dynamic submodule creation. The source outputs pure data (arrays of objects); the config describes how each item becomes a module.

```
RECORD RenderDef:
    items_key : String             -- JSON key containing the array to iterate
    id_field  : String             -- object field for stable child ID
    module    : ModuleDef          -- template module applied per item
    group     : Integer            -- index of the group to place dynamic children in
```

**Rationale**: Scripts output pure data, config describes presentation. This keeps data/presentation separation clean.

**Example**: A workspaces source outputs `{"workspaces": [{"id": 1, "name": "term", "active": true}, ...]}`. The render config iterates `workspaces`, uses `id` as the stable child ID, and stamps out a command-module per workspace with a widget template like `{% if active %}[{{ name }}]{% else %} {{ name }} {% endif %}` and a command like `niri-msg action focus-workspace {{ id }}`.

Dynamic children are merged with static `children` at runtime. If a dynamic child's ID collides with a static child, the static definition takes precedence.

### 3.9 Source Definition

```
ENUM SourceType:
    POLL        -- run command on interval, parse stdout as JSON
    SUBSCRIBE   -- spawn long-running process, parse each stdout line as JSON
    FILE        -- read JSON files on interval
```

```
RECORD SourceDef:
    type     : SourceType
    command  : List<String>    -- command + args (poll and subscribe)
    interval : Integer = 5    -- seconds between polls (poll and file)
    paths    : List<String>   -- file paths to read (file only)
```

### 3.10 Event Definition

```
RECORD EventDef:
    name       : String             -- unique event name within module
    condition  : String             -- Tera template (truthy = fire)
    on_trigger : String | None      -- Tera template for shell command
```

Events fire on rising edge only (false-to-true transition). The condition is re-evaluated on each source update. When an event fires, `on_trigger` (if set) is rendered and executed as a shell command. Spotlights referencing this event name activate within the module's parent bar-module.

---

## 4. State Machine

### 4.1 Navigation State

The bar replaces the old three-state model (Passive/Active/Mode) with a navigation stack and display mode.

```
RECORD NavState:
    stack    : List<String>        -- path from root, e.g. ["power", "profiles"]
    mode     : DisplayMode         -- VISUAL or TEXT
    query    : String              -- text-mode filter string
    selected : usize               -- text-mode selection index
```

```
ENUM DisplayMode:
    VISUAL    -- widgets rendered in groups
    TEXT      -- searchable list of submodules
```

The `stack` determines which bar-module is currently displayed. An empty stack means the root bar-module. Each entry is a child ID, forming a path like `["power", "profiles"]`.

### 4.2 Keyboard Interactivity

Keyboard interactivity depends on stack depth and display mode:

| Condition                    | Interactivity          |
|------------------------------|------------------------|
| Root + VISUAL                | `None` (passive)       |
| Any depth in TEXT            | `OnDemand` (grabbed)   |
| Non-root in VISUAL           | `OnDemand` (grabbed)   |

At root VISUAL, the bar is a passive display — no keyboard capture. Everything else grabs keyboard focus.

### 4.3 Transitions

```
-- Focus entry
IPC toggle-text / hotkey      -> enter TEXT at root (keyboard grabbed)

-- Navigation (both modes)
Escape at root                -> unfocus (return to passive, keyboard released)
Escape at depth > 0           -> pop stack (go up one level)
Click bar-module widget       -> push child onto stack (VISUAL)
Click command-module widget   -> execute command, stay at current level

-- Visual mode
/                             -> switch to TEXT at current level
All other keys                -> forwarded to module-specific handling

-- Text mode
Return on bar-module          -> push onto stack, switch to VISUAL
Return on cmd-module          -> execute command, dismiss to root
Ctrl+Return on bar-module     -> push onto stack, stay in TEXT
Ctrl+Escape at depth > 0     -> pop stack, stay in TEXT

-- Auto-enter
2+ chars typed, single match  -> auto-push (bar, VISUAL) or auto-execute (command)
```

### 4.4 Visual Mode Keys

Reserved keys (always handled by the bar):

| Key       | Action                                    |
|-----------|-------------------------------------------|
| Escape    | Pop stack; unfocus if at root              |
| `/`       | Enter text mode at current level           |

All other keys are owned by the current module. For example, the calendar module uses Left/Right for month navigation and `t` to jump to today. Modules must not bind `/` or Escape.

At root depth with no keyboard focus, no keys are captured.

### 4.5 Text Mode Keys

| Key          | Action                                              |
|--------------|-----------------------------------------------------|
| Escape       | Pop stack; unfocus if at root                       |
| Ctrl+Escape  | Pop stack, stay in text mode; unfocus if at root    |
| Return       | Push selected bar-module (VISUAL) / execute cmd     |
| Ctrl+Return  | Push selected bar-module (TEXT) / execute cmd        |
| Left         | Cycle selection backward (wraps)                    |
| Right        | Cycle selection forward (wraps)                     |
| BackSpace    | Delete last character from query                    |
| Printable    | Append to query, reset selection to 0               |

When the query has 2+ characters and exactly one child matches, auto-enter triggers: push for bar-modules (enters VISUAL), execute + dismiss for command-modules.

### 4.6 Special Module Types

**Calendar**: A bar-module with `type: "calendar"` renders a month grid instead of child widgets. Left/Right navigate months, `t` returns to today.

---

## 5. Source System

### 5.1 Source Manager

The source manager walks the module tree recursively, registering calloop event sources for every module that declares a `source`. Sources are keyed by the module's dot-path ID (e.g., `"power.profiles.saver"`).

All sources write JSON data into a shared `Map<String, ModuleState>` and set a dirty flag.

```
RECORD ModuleState:
    data         : JSON              -- current module data
    prev_data    : JSON              -- previous data (for change detection)
    dirty        : Boolean           -- set when data changes
    event_states : Map<String, Boolean>  -- last evaluated state per event
```

### 5.2 Poll Source

```
FUNCTION register_poll(path, command, interval, handle, dirty, states):
    -- Seed: run command synchronously, parse JSON, store in states
    output = run_command(command)
    states[path].data = parse_json(output)

    -- Timer: re-run on interval
    timer = Timer(interval seconds)
    handle.insert_source(timer, callback):
        output = run_command(command)
        states[path].data = parse_json(output)
        states[path].dirty = true
        dirty.set(true)
        RETURN ToDuration(interval)
```

### 5.3 Subscribe Source

```
FUNCTION register_subscribe(path, command, handle, dirty, states):
    child = spawn(command, stdout=PIPE)
    set_nonblocking(child.stdout)

    -- Register FD with calloop (edge-triggered)
    generic = Generic(child.stdout_fd, READ, Edge)
    handle.insert_source(generic, callback):
        LOOP:
            bytes = read(fd)
            IF bytes == 0 OR EAGAIN:
                BREAK
            append bytes to line_buffer
            FOR EACH complete line in line_buffer:
                states[path].data = parse_json(line)
                states[path].dirty = true
                dirty.set(true)
```

### 5.4 File Source

```
FUNCTION register_file(path, file_paths, interval, handle, dirty, states):
    -- Seed: read all files, merge into object keyed by file stem
    obj = {}
    FOR EACH fp IN file_paths:
        key = file_stem(fp)
        obj[key] = parse_json(read_file(fp))
    states[path].data = obj

    -- Timer: re-read on interval
    timer = Timer(interval seconds)
    handle.insert_source(timer, callback):
        -- same as seed logic
        RETURN ToDuration(interval)
```

### 5.5 Dynamic Submodule Sources

When a module has a `render` definition, its source data drives child creation. After each source update:

1. Extract the array at `render.items_key` from the module's data.
2. For each item, derive a child ID from `render.id_field`.
3. Instantiate the `render.module` template with the item's fields as context.
4. Merge dynamic children into the module's child map (static children take precedence on ID collision).
5. Register sources for any new dynamic children; clean up sources for removed children.

---

## 6. Template Engine

### 6.1 Engine

The template engine wraps Tera. Templates are registered at startup by walking the module tree. Each template receives its module's JSON data as context.

Template name conventions use dot-paths matching the module tree:
- `"{path}.widget"` -- widget template
- `"{path}.widget.__cond"` -- widget condition
- `"{path}.label"` -- label template
- `"{path}.__event.{name}.__cond"` -- event condition
- `"{path}.__event.{name}.__trigger"` -- event trigger command
- `"{path}.render.widget"` -- render template for dynamic children
- `"{path}.render.widget.__cond"` -- render condition for dynamic children

Where `{path}` is the dot-separated module path, e.g. `"power.profiles"`.

### 6.2 Custom Filters

| Filter          | Input        | Args                  | Output                        |
|-----------------|--------------|-----------------------|-------------------------------|
| `icon`          | String       | none                  | PUA character for named icon  |
| `meter`         | Float        | `max`, `width`        | Block meter string            |
| `bar`           | Float        | `max`                 | Single vertical bar character |
| `human_bytes`   | Float        | none                  | Human-readable byte size      |
| `human_duration`| Integer      | none                  | Duration like "4h 12m"        |
| `pad_left`      | String       | `width`               | Right-aligned in field        |
| `pad_right`     | String       | `width`               | Left-aligned in field         |
| `color`         | Any          | `fg`, `bg`            | Passthrough (reserved)        |

### 6.3 Custom Functions

| Function  | Args           | Output                          |
|-----------|----------------|---------------------------------|
| `icon`    | `name: String` | PUA character for named icon    |
| `changed` | `key: String`  | Boolean: field differs from prev|

### 6.4 Icon System

Icons are mapped from names to Unicode Private Use Area (PUA) codepoints via `assets/icons.json`:

```
-- Example entries
{ "clock": "0xE903", "wifi-high": "0xEA58", "battery-full": "0xE87A" }
```

When a template emits a PUA character (via the `icon` filter/function), the renderer checks for a pre-loaded SVG pixmap for that character. If found, the SVG is rendered instead of a font glyph.

SVG resolution: `{icons_dir}/{weight}/{icon_name}.svg` where weight is derived from the icon name suffix (`-fill`, `-bold`, `-thin`, `-light`, `-duotone`) or defaults to `regular`.

### 6.5 Special Template Variables

| Variable     | Type         | Available In         | Description                                 |
|--------------|--------------|----------------------|---------------------------------------------|
| `__output`   | String       | Widget templates     | Name of the Wayland output being rendered   |
| `__prev`     | JSON         | Event conditions     | Previous module data for change detection   |
| `__path`     | List<String> | All templates        | Module's path in the tree (e.g. `["power", "profiles"]`) |
| `__depth`    | Integer      | All templates        | Depth in the module tree (root = 0)         |
| `__index`    | Integer      | Render templates     | Item index within the rendered array        |
| `__parent`   | JSON         | Render templates     | Parent module's full source data            |

---

## 7. Rendering Pipeline

### 7.1 Grid Model

The bar renders into a single-row `Grid` of fixed-width `Cell` values:

```
RECORD Cell:
    ch   : Char     -- character to display
    fg   : Rgba     -- foreground color
    bg   : Rgba     -- background color
    attrs: CellAttrs -- bold, dim (reserved)
```

Cell width and height are derived from the font metrics of the character 'M' at the configured font size.

The grid column count is the maximum number of whole cells that fit in the output width (accounting for scale factor). The layer surface spans the full output width, but the grid is centered horizontally — the left and right margins are each half the remainder after fitting whole cells. This means the grid always contains the maximum usable columns and sits centered on screen.

```
columns = floor(output_width_px / (cell_w * scale))
margin  = (output_width_px - columns * cell_w * scale) / 2
```

### 7.2 Layout

The layout depends on the current navigation state.

**Visual mode** (VISUAL at any depth): Groups are distributed across the grid with equal spacing (space-between). The home button (and breadcrumb path at depth > 0) is prepended to the first group. Within each group, widgets render left-to-right separated by 1 column. Content exceeding a group's `max` truncates with `…`. Hit areas map column ranges to module paths for click handling.

```
-- Root (3 groups, home icon prepended to group 0):
[🏠 launcher workspaces window]    [clock notifications]    [audio network power]

-- At depth (breadcrumb prepended to group 0):
[🏠 > power  profiles suspend]    [info]
```

**Text mode** (TEXT at any depth): Query with cursor on the left, filtered child module names centered with selection highlighting and match character highlighting, shortcut hints on the right.

### 7.3 Breadcrumb

The home button is always injected as the first item of the first group. At root depth, it renders as just a home icon. At deeper depths, it expands into a breadcrumb trail prepended to group 0:

```
-- Root:
[🏠 launcher workspaces window]    [...]    [...]

-- Depth 1:
[🏠 > power  profiles suspend]    [info]

-- Depth 2:
[🏠 > power > profiles  saver balanced performance]
```

Each breadcrumb segment is clickable — clicking it navigates to that level. The home icon always navigates to root. The breadcrumb uses the `name` field of each module in the path, with `>` separators.

### 7.4 Label Rendering

When the pointer hovers over a widget that has a `label` defined:

1. Render the label template with the module's source data.
2. Position the label text adjacent to the widget, on whichever side has more available space, offset toward the cursor direction.
3. Fade all other widgets to low opacity (dim).
4. On pointer leave, restore normal opacity and hide the label.

Labels are inline in the bar row, not floating popups.

### 7.5 Dynamic Submodule Rendering

When a bar-module has a `render` definition, its children include dynamically generated modules. These render identically to static children — each has a widget placed in the group specified by `render.group`. The only difference is lifecycle: dynamic children are created and destroyed as the source data changes.

### 7.6 Render Steps

```
FUNCTION render_grid(grid, pixmap, icons, bg, scale):
    sf = scale as float
    cell_w = base_cell_w * sf
    cell_h = base_cell_h * sf
    pad = base_padding * sf
    font_size = base_font_size * sf

    -- 1. Fill background
    fill pixmap with bg color
    FOR EACH cell IN grid:
        fill cell rectangle with cell.bg

    -- 2. Render icons with contrast outline
    FOR EACH cell IN grid WHERE cell is icon:
        shadow = shadow_color(cell.fg)
        FOR EACH (ox, oy) IN outline_offsets:
            composite_icon(pixmap, icon, x + ox, y + oy, shadow)
        composite_icon(pixmap, icon, x, y, cell.fg)

    -- 3. Render text runs with contrast outline
    FOR EACH contiguous run of same-fg non-icon cells:
        shape text with cosmic-text

        -- Shadow pass: spread glyphs to 8 neighbors
        shadow = shadow_color(run_fg)
        draw text with shadow color, spreading each pixel to outline_offsets

        -- Text pass: normal rendering
        draw text with run_fg color

    -- 4. Copy to Wayland buffer
    convert RGBA to premultiplied ARGB8888 (BGRA byte order)
```

### 7.7 Contrast Outline

Every text glyph and icon receives an automatic contrast halo. The shadow color is computed from the foreground luminance:

```
FUNCTION shadow_color(fg: Rgba) -> Rgba:
    luma = 0.299 * fg.r + 0.587 * fg.g + 0.114 * fg.b
    IF luma > 128:
        RETURN Rgba(0, 0, 0, 180)     -- dark halo for light content
    ELSE:
        RETURN Rgba(255, 255, 255, 180) -- light halo for dark content
```

The outline is rendered at 8 offsets (cardinal + diagonal, 1 physical pixel each) before the foreground pass. Alpha blending ensures the halo composites correctly with the bar background and, through the compositor, with the wallpaper beneath.

### 7.8 Icon Compositing

Icons are tinted: the SVG's alpha channel is used as a mask, and the cell's foreground color fills the shape. This allows a single SVG to render in any color.

### 7.9 Spotlight System

A spotlight temporarily replaces a group of the current bar-module with a single widget. Spotlights are triggered by events and expire after a configurable timeout. Spotlights are scoped to their parent bar-module — they only appear when that module's level is currently displayed.

```
FUNCTION activate_spotlight(group_idx, mod_path, timeout):
    cancel existing spotlight timer for group
    start new timer(timeout seconds):
        ON EXPIRE: remove spotlight, set dirty
    store SpotlightInfo { mod_path, timer_token }
    set dirty
```

---

## 8. IPC Protocol

### 8.1 Transport

Line-based JSON over a Unix domain socket at `$XDG_RUNTIME_DIR/retrobar.sock`. The daemon binds the socket on startup (removing any stale file) and cleans it up on exit.

Connection lifecycle: client connects, sends one JSON line, daemon reads it, dispatches, writes one JSON response line, connection closes.

### 8.2 Request Format

```
RECORD IpcRequest:
    cmd    : String      -- command name (tagged enum discriminant)
    -- additional fields vary by command
```

### 8.3 Commands

| Command      | CLI Usage                          | Request JSON                                            | Effect                                    |
|--------------|------------------------------------|---------------------------------------------------------|-------------------------------------------|
| toggle-text  | `retrobar toggle-text`             | `{"cmd":"toggle-text"}`                                 | Toggle TEXT <-> VISUAL at current level    |
| dismiss      | `retrobar dismiss`                 | `{"cmd":"dismiss"}`                                     | Force root VISUAL from any state          |
| push         | `retrobar push <child>`            | `{"cmd":"push","child":"power"}`                        | Push child onto nav stack                 |
| pop          | `retrobar pop`                     | `{"cmd":"pop"}`                                         | Pop one level from nav stack              |
| navigate     | `retrobar navigate <path...>`      | `{"cmd":"navigate","path":["power","profiles"]}`        | Jump to specific tree path                |
| state        | `retrobar state`                   | `{"cmd":"state"}`                                       | Query current state                       |
| run          | `retrobar run <path>`              | `{"cmd":"run","path":"power.profiles.saver"}`           | Execute a command-module by dot-path      |
| type         | `retrobar type <text>`             | `{"cmd":"type","text":"net"}`                           | Inject text into text-mode query          |
| key          | `retrobar key <keyname>`           | `{"cmd":"key","key":"Return"}`                          | Inject synthetic key event                |

### 8.4 Response Format

```
RECORD IpcResponse:
    ok       : Boolean              -- true on success
    error    : String | None        -- error message on failure
    path     : List<String> | None  -- current nav stack (e.g. ["power", "profiles"])
    mode     : String | None        -- "visual" or "text"
    query    : String | None        -- current query (text mode)
    selected : Integer | None       -- selected index (text mode)
```

### 8.5 Key Names

The `key` command accepts these key names:

| Key Name    | Mapped Keysym     |
|-------------|-------------------|
| `Return`    | `Keysym::Return`  |
| `Enter`     | `Keysym::Return`  |
| `Escape`    | `Keysym::Escape`  |
| `BackSpace` | `Keysym::BackSpace` |
| `Tab`       | `Keysym::Tab`     |
| `Up`        | `Keysym::Up`      |
| `Down`      | `Keysym::Down`    |
| `Left`      | `Keysym::Left`    |
| `Right`     | `Keysym::Right`   |
| `Page_Up`   | `Keysym::Page_Up` |
| `Page_Down` | `Keysym::Page_Down`|
| Single char | Keysym from codepoint, utf8 set |

### 8.6 Client Mode

The `retrobar` binary doubles as the IPC client. When invoked with a known command as the first argument, it connects to the socket, sends the request, prints the response, and exits with code 0 (ok) or 1 (error). A 100ms read timeout on the daemon side prevents misbehaving clients from stalling the event loop.

---

## 9. Definition of Done

### 9.1 Configuration

- [ ] Config loads from `--config` path or `$XDG_CONFIG_HOME/retrobar/config.json`
- [ ] All settings fields have working defaults
- [ ] Hex color strings parse as `#RRGGBB` and `#RRGGBBAA`
- [ ] Unknown JSON keys are silently ignored (forward compatibility)
- [ ] Root bar-module parsed with groups and children
- [ ] Bar-modules and command-modules distinguished by `groups` vs `command`
- [ ] Recursive module tree resolved at startup

### 9.2 Module Tree

- [ ] Module IDs use dot-separated paths (`"power.profiles.saver"`)
- [ ] Tree walk registers all sources recursively
- [ ] Tree walk compiles all templates recursively
- [ ] Dynamic children from render API merge with static children
- [ ] Static children take precedence on ID collision with dynamic children

### 9.3 Navigation

- [ ] Bar starts at root VISUAL with `KeyboardInteractivity::None`
- [ ] Click bar-module widget pushes onto stack
- [ ] Click command-module widget executes command
- [ ] Escape in VISUAL at depth > 0 pops stack
- [ ] Escape in VISUAL at root is no-op
- [ ] Escape in TEXT switches to VISUAL at same level
- [ ] Keyboard interactivity set to `OnDemand` for non-root VISUAL and all TEXT
- [ ] Keyboard interactivity set to `None` for root VISUAL

### 9.4 Visual Mode

- [ ] Groups distributed with space-between layout
- [ ] Group `max` truncates content with `…`
- [ ] Home button prepended to first group at all depths
- [ ] Breadcrumb path prepended at depth > 0 with `>` separators
- [ ] Breadcrumb segments are clickable (navigate to that level)
- [ ] Hit areas map column ranges to module paths

### 9.5 Text Mode

- [ ] Text mode: query left, filtered children center, shortcuts right
- [ ] Fuzzy matching over child module names
- [ ] Highlight matched characters in names
- [ ] Arrow/Tab cycles selection
- [ ] Return on bar-module pushes onto stack
- [ ] Return on command-module executes + dismisses to root VISUAL
- [ ] Auto-enter triggers when query has 2+ chars and single match
- [ ] Shortcut hints rendered from `text_mode.shortcuts`

### 9.6 Labels

- [ ] Hover over widget with label shows inline label text
- [ ] Other widgets fade to low opacity during label display
- [ ] Label positioned on side with more available space
- [ ] Label disappears on pointer leave

### 9.7 Dynamic Submodules

- [ ] Render API extracts array from source data at `items_key`
- [ ] Each item stamped into a module using `render.module` template
- [ ] Dynamic children placed in `render.group`
- [ ] Dynamic children created/destroyed as source data changes
- [ ] Sources registered for new dynamic children, cleaned up for removed ones

### 9.8 Source System

- [ ] Poll source seeds initial data synchronously, then re-runs on interval
- [ ] Subscribe source spawns a long-running child, reads stdout lines as JSON
- [ ] Subscribe source sets stdout non-blocking and uses edge-triggered calloop
- [ ] File source reads and merges multiple JSON files keyed by file stem
- [ ] Source manager kills subscribe child processes on drop
- [ ] Source registration walks module tree recursively

### 9.9 Template Engine

- [ ] Tera templates render with module JSON data as context
- [ ] `icon` filter and function emit PUA characters from icons.json mapping
- [ ] `meter`, `bar`, `human_bytes`, `human_duration` filters produce correct output
- [ ] `changed` function compares current and previous data
- [ ] Widget conditions gate visibility (falsy/empty = hidden)
- [ ] `__output`, `__path`, `__depth` variables available
- [ ] `__index`, `__parent` variables available in render templates

### 9.10 Events and Spotlights

- [ ] Events fire on rising edge (false -> true transition) only
- [ ] Event triggers render and execute shell commands
- [ ] Spotlights scoped to parent bar-module level
- [ ] Spotlight timers expire and restore normal display
- [ ] New spotlight for same group cancels the old timer

### 9.11 IPC

- [ ] Daemon binds socket at `$XDG_RUNTIME_DIR/retrobar.sock`
- [ ] Stale socket file removed before bind
- [ ] Socket file removed on clean shutdown
- [ ] All 9 commands (toggle-text, dismiss, push, pop, navigate, state, run, type, key) dispatch correctly
- [ ] `state` returns current path, mode, and relevant fields
- [ ] `push` with unknown child returns error
- [ ] `push` on command-module returns error
- [ ] `type` outside text mode returns error
- [ ] `run` with unknown path returns error
- [ ] `key` constructs synthetic KeyEvent and reuses existing key handling
- [ ] Client mode: binary detects command arg, connects, sends, prints response, exits
- [ ] 100ms read timeout prevents client stalls

### 9.12 Rendering

- [ ] Grid cell dimensions derived from font 'M' metrics
- [ ] Icons render as tinted SVGs when available, falling back to font glyph
- [ ] Contrast outline renders for both text and icons
- [ ] Shadow color is dark for light foregrounds, light for dark foregrounds
- [ ] Multi-output: one BarInstance per output, independent scale factors
- [ ] Dirty flag prevents redundant redraws

### 9.13 Cross-Feature Parity Matrix

| Scenario                               | Root VISUAL | Depth VISUAL | TEXT   |
|----------------------------------------|-------------|--------------|--------|
| Correct layout renders                 | [ ]         | [ ]          | [ ]    |
| Keyboard interactivity set correctly   | [ ]         | [ ]          | [ ]    |
| IPC `state` returns correct state      | [ ]         | [ ]          | [ ]    |
| IPC `dismiss` returns to root VISUAL   | [ ]         | [ ]          | [ ]    |
| Contrast outline renders on text/icons | [ ]         | [ ]          | [ ]    |

| Source Type | Seeds on startup | Updates on trigger | Sets dirty flag |
|-------------|------------------|--------------------|-----------------|
| Poll        | [ ]              | [ ]                | [ ]             |
| Subscribe   | [ ]              | [ ]                | [ ]             |
| File        | [ ]              | [ ]                | [ ]             |

### 9.14 Integration Smoke Test

```
-- 1. Start daemon with test config
daemon = spawn("retrobar --config test-config.json")
wait_for_socket("$XDG_RUNTIME_DIR/retrobar.sock")

-- 2. Query initial state
resp = ipc("state")
ASSERT resp.ok == true
ASSERT resp.path == []
ASSERT resp.mode == "visual"

-- 3. Push into a module
resp = ipc("push", child="power")
ASSERT resp.ok == true
ASSERT resp.path == ["power"]
ASSERT resp.mode == "visual"

-- 4. Toggle text mode
resp = ipc("toggle-text")
ASSERT resp.ok == true
ASSERT resp.mode == "text"
ASSERT resp.query == ""

-- 5. Type to filter
resp = ipc("type", text="prof")
ASSERT resp.ok == true
ASSERT resp.query == "prof"

-- 6. Toggle back to visual
resp = ipc("toggle-text")
ASSERT resp.mode == "visual"
ASSERT resp.path == ["power"]

-- 7. Pop back to root
resp = ipc("pop")
ASSERT resp.ok == true
ASSERT resp.path == []

-- 8. Navigate directly to nested path
resp = ipc("navigate", path=["power", "profiles"])
ASSERT resp.ok == true
ASSERT resp.path == ["power", "profiles"]

-- 9. Dismiss to root
resp = ipc("dismiss")
ASSERT resp.ok == true
ASSERT resp.path == []
ASSERT resp.mode == "visual"

-- 10. Run a command-module
resp = ipc("run", path="power.profiles.saver")
ASSERT resp.ok == true

-- 11. Error cases
resp = ipc("push", child="nonexistent")
ASSERT resp.ok == false
ASSERT resp.error CONTAINS "unknown child"

resp = ipc("type", text="hello")
ASSERT resp.ok == false
ASSERT resp.error CONTAINS "not in text mode"

-- 12. Inject key
resp = ipc("navigate", path=["power"])
resp = ipc("key", key="Escape")
ASSERT resp.path == []

-- 13. Cleanup
kill(daemon)
ASSERT NOT file_exists("$XDG_RUNTIME_DIR/retrobar.sock")
```

---

## Appendix A: Design Decision Rationale

**Why a character grid instead of pixel layout?** A fixed-width grid eliminates fractional pixel arithmetic, makes column-based alignment trivial, and maps naturally to monospace font rendering. Hit-testing is a simple division. The tradeoff is no proportional fonts or sub-cell positioning, which is acceptable for a status bar.

**Why Tera templates instead of a custom format language?** Tera is a mature, well-documented template engine with conditionals, loops, filters, and functions. It avoids reinventing expression evaluation. The tradeoff is a runtime dependency, but it's pure Rust with no system dependencies.

**Why external scripts instead of built-in modules?** Built-in modules create a dependency maze (D-Bus bindings, PulseAudio clients, NetworkManager APIs). External scripts can be written in any language, tested independently, and replaced without recompiling. The bar stays small and the module ecosystem stays open.

**Why cosmic-text instead of fontconfig + freetype?** cosmic-text handles font loading, shaping (including ligatures and complex scripts), and rasterization in a single Rust crate. It integrates with SwashCache for glyph caching. The alternative is stitching together multiple C library bindings.

**Why calloop instead of tokio?** The bar is fundamentally single-threaded and I/O-bound (waiting for Wayland events, timer ticks, pipe data). calloop is the event loop used by smithay-client-toolkit itself, so it integrates natively with Wayland sources. tokio would add unnecessary complexity and a large dependency for async runtime machinery that provides no benefit here.

**Why a contrast outline instead of enforcing a minimum background opacity?** A transparent bar that floats over the wallpaper is an aesthetic choice users should be free to make. The outline ensures readability at any opacity, including 0%, without constraining the visual design. The luminance-adaptive shadow (dark for light text, light for dark text) handles both light and dark wallpapers.

**Why a recursive tree instead of flat modules?** Flat modules force all interaction into a single level — every module needs its own bespoke mode template with inline navigation. A recursive tree lets modules compose naturally: power contains profiles, workspaces contain per-workspace controls, audio contains per-sink settings. The same navigation primitives (push, pop, text search) work at every level. The bar becomes a filesystem-like hierarchy navigated with a consistent UX, rather than a collection of unrelated modal views.

**Why a render API with item arrays + config templates?** Scripts should output pure data (an array of workspace objects, a list of bluetooth devices) without knowing how the bar will display them. The render config describes how each data item becomes a clickable submodule — widget template, command template, placement section. This keeps scripts reusable across different bar configurations and avoids embedding presentation logic in shell/Python code. The alternative (scripts generating module definitions directly) would couple data sources to bar internals.

**Why inline labels instead of popover tooltips?** Popovers require a second layer surface, z-ordering logic, and compositor-specific positioning. Inline labels reuse the existing grid — the label text renders in the same row, adjacent widgets dim, and nothing leaves the bar's surface. This is simpler to implement, more consistent with the grid aesthetic, and avoids compositor compatibility issues. The tradeoff is limited label length (constrained by bar width), which is acceptable since labels are meant to be brief.

**Why space-between groups instead of left/center/right?** Three named sections are a special case that doesn't generalize. Space-between over N groups gives the same symmetric result for 3 groups but also works for 2 or 5. The only non-grid math is one division (`gap = remaining / (n-1)`), and within each group layout is still pure column arithmetic. Adding `max` per group prevents overflow without introducing flex inside groups.

---

## Appendix B: Default Module Tree

The reference layout for a typical desktop. Modules prefixed with `!` have a widget condition that hides them by default — they appear only when an event makes them relevant (e.g., media playing, bluetooth connected, high CPU temperature).

### Top-Level Groups

```
[🏠 launcher workspaces window]    [!system clock !notifications]    [!mounts !bluetooth !media audio brightness network power]
```

### Full Tree

```
root (bar)
├─ group 0: [launcher, workspaces, window]
│  ├─ launcher (bar, render: dynamic from desktop entries)
│  │  └─ [firefox], [kitty], ... (cmd: gtk-launch <app>)
│  ├─ workspaces (bar, render: dynamic per workspace)
│  │  └─ [1], [2], ... (cmd: focus-workspace <id>)
│  └─ window (cmd: focus current window)
│
├─ group 1: [system, clock, notifications]
│  ├─ !system (bar, condition: cpu/temp threshold)
│  │  ├─ cpu (cmd)
│  │  ├─ memory (cmd)
│  │  └─ temp (cmd)
│  ├─ clock (bar, type: calendar)
│  │  └─ weather (bar)
│  │     └─ forecast days...
│  └─ !notifications (bar, condition: unread count > 0, render: dynamic)
│     └─ [notif-1], ... (cmd: dismiss)
│
└─ group 2: [mounts, bluetooth, media, audio, brightness, network, power]
   ├─ !mounts (bar, condition: removable mounted, render: dynamic)
   │  └─ [sda1], ... (cmd: unmount)
   ├─ !bluetooth (bar, condition: device connected, render: dynamic)
   │  └─ [WH-1000XM4], ... (cmd: disconnect)
   ├─ !media (bar, condition: player active)
   │  ├─ prev (cmd: playerctl previous)
   │  ├─ play (cmd: playerctl play-pause)
   │  └─ next (cmd: playerctl next)
   ├─ audio (bar)
   │  ├─ mute (cmd: wpctl set-mute toggle)
   │  └─ sinks (bar, render: dynamic per sink)
   │     └─ [WH-1000XM4], ... (cmd: wpctl set-default)
   ├─ brightness (bar)
   │  ├─ up (cmd: brightnessctl set +5%)
   │  └─ down (cmd: brightnessctl set 5%-)
   ├─ network (bar, render: dynamic per connection)
   │  └─ [eduroam], ... (cmd: nmcli connect)
   └─ power (bar)
      ├─ profiles (bar)
      │  ├─ saver (cmd: powerprofilesctl set power-saver)
      │  ├─ balanced (cmd: powerprofilesctl set balanced)
      │  └─ performance (cmd: powerprofilesctl set performance)
      └─ suspend (cmd: systemctl suspend)
```

### Design Notes

- **Launcher first**: acts as a home/start button on the leftmost position.
- **Conditional modules (`!`)**: keep the root bar clean — mounts, bluetooth, media, system, and notifications only appear when relevant. Their events control widget visibility via conditions.
- **Weather under clock**: time-related information groups naturally. Drilling into clock shows calendar + weather forecast.
- **Dynamic collections**: workspaces, desktop entries, bluetooth devices, sinks, mounts, and notifications all use the render API — their source scripts output arrays, the config stamps each item into a module.
- **Deepest nesting is 3 levels**: `root > power > profiles > saver`. Most modules are 1-2 levels deep, keeping breadcrumbs short.
