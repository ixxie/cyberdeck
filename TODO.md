# Retrobar TODO

## Legend
- [ ] pending
- [x] done
- [-] cut / deferred

---

## Phase 0: Rename & Cleanup
- [x] Rename package from `retroshell` to `retrobar` (Cargo.toml, flake.nix, module.nix, all Rust references)
- [x] Update config path from `retroshell/` to `retrobar/`
- [x] Clean up unused config fields (`panel`, stale `events` stub)

## Phase 1: Config Schema Overhaul
- [x] Expand `ModuleDef` to match plan: add `mode` (template/pages/filter/actions), `events` list
- [x] Add `WidgetDef.spotlight` (event ref + timeout)
- [x] Add `WidgetDef.style` as template string (replaces static fg/bg)
- [x] Add `Settings.activate_key`, `Settings.mode_keys` map, `Settings.icon_weight`
- [x] Add `Settings.background.blur` flag
- [x] Add `SourceDef::File` variant (paths + interval)
- [x] Add `ModeDef` struct: template/pages (mutually exclusive), actions map, optional filter
- [x] Add `FilterDef` struct: source key, field, template, on_select
- [x] Add `EventDef` struct: name, condition template, optional on_trigger template
- [x] Add `SpotlightDef` struct: event name, timeout
- [x] Write test-config.json that exercises the new schema

## Phase 2: State Controller
- [x] Define `BarState` enum: `Passive`, `Active { query, selected }`, `Mode { module, page, filter_query, filter_selected }`
- [x] Implement state transitions: passive->active, active->mode, mode->passive, passive->mode (via mode key / click)
- [x] Toggle `keyboard_interactivity` on state change (none for passive, on_demand for active/mode)
- [x] Keyboard event handling via SCTK seat/keyboard
- [x] Route keypresses based on current state

## Phase 3: Active State (Fuzzy Menu)
- [x] Render active UI: query field + centered module names + match count
- [x] Fuzzy matching over module names (simple substring or scoring)
- [x] Highlight matched characters in module names
- [x] Arrow/Tab cycles selection
- [x] Enter -> enter selected module's mode
- [x] Auto-enter mode on single match with >=2 chars typed
- [x] Escape -> passive

## Phase 4: Mode Rendering
- [x] Render mode UI: module name + template output + action hints
- [x] Compile and evaluate mode templates against module JSON state
- [x] Key -> action dispatch: match keypress against mode's `actions` map, spawn shell command
- [x] Pagination support: multiple pages, up/down navigation, page indicator (1/N)

## Phase 5: Mode Filters
- [x] Filter UX: query input field, fuzzy match over entries from JSON state
- [x] Render matching entries with selection highlight
- [x] Arrow keys cycle selection
- [x] Enter executes `on_select` template against selected entry
- [x] Reuse filter logic between active menu and mode filters

## Phase 6: Event System
- [x] Track previous JSON state per module
- [x] After each state update, evaluate all event conditions (Tera templates)
- [x] Detect false->true edge transitions
- [x] Execute `on_trigger` commands (Tera-templated) on fire
- [x] Implement `changed` Tera function (compare current vs previous state value)

## Phase 7: Spotlight
- [x] Widget spotlight declaration in config
- [x] On event fire, if a widget references that event via spotlight, replace section content
- [x] Calloop timer for spotlight timeout
- [x] Restore normal section content after timeout
- [x] Reset timer if same event fires again during active spotlight

## Phase 8: Tera Filters & Functions
- [x] `meter(max, width)` -> block meter string (█ filled, · empty)
- [x] `bar(max)` -> single vertical bar char (▁▂▃▄▅▆▇█)
- [x] `human_bytes` -> human-readable byte size (K/M/G/T)
- [x] `human_duration` -> "4h 12m" format (input in seconds)
- [x] `pad_left(width)` / `pad_right(width)` (character-aware)
- [-] `format_time(fmt)` -> deferred (needs time library dep)
- [x] `color(fg, bg)` -> passthrough stub (inline styles deferred to Phase 14)
- [x] `changed` -> implemented in Phase 6 as `changed(key="field")` function
- [x] `icon(name)` function -> Phosphor codepoint lookup (added alongside existing filter)
- [-] `truncate(length)` -> use Tera built-in: `{{ val | truncate(length=N, end="") }}`

## Phase 9: Pointer Interaction
- [x] Handle pointer enter/leave/click via SCTK seat
- [x] Hit-test clicks against widget cell ranges
- [x] Click on widget -> enter that module's mode

## Phase 10: File Source Type
- [x] Implement `SourceDef::File` variant
- [x] Read file paths, build JSON from basenames
- [x] Poll-based fallback with configurable interval
- [-] (Optional) inotify watcher via calloop — deferred

## Phase 11: Calendar Mode (Built-in)
- [x] Built-in `calendar` mode type for time module
- [x] Render month grid inline in bar row
- [x] Left/right arrow navigation between months
- [x] `t` key jumps to today
- [x] Highlight current day

## Phase 12: Nix Module Overhaul
- [x] Rename service from `retroshell` to `retrobar`
- [x] Update module.nix to match new config schema
- [x] Add per-module `deps` collection and PATH wrapping
- [-] Structured module type checking in Nix — deferred (attrs works for now)
- [x] Update settings.nix with new schema defaults
- [x] Ensure compiled JSON matches what the Rust binary expects

## Phase 13: Module Scripts
- [x] Update existing scripts (time, network, workspaces) for new JSON contract
- [x] Write power-stats script
- [x] Write system-stats script
- [x] Write audio-monitor script (audio-stats.sh)
- [x] Write bluetooth-stats script
- [x] Write display-stats script
- [x] Write notif-monitor script
- [x] Write storage-stats script
- [x] Write media-monitor script
- [x] Write weather-fetch script
- [x] Write desktop-entries script (launcher)
- [x] Write niri-window script (window title via niri events)

## Phase 14: Polish & Edge Cases
- [x] Graceful process cleanup on exit (kill source children via Drop + SIGINT/SIGTERM)
- [x] Handle source command crashes (poll logs errors and retries, subscribe read errors logged)
- [x] Handle malformed JSON from sources (already handled — invalid JSON is logged and skipped)
- [x] Handle narrow bar widths (grid truncates at column bounds naturally)
- [x] Style system: `ok`, `warn`, `alert`, `muted`, `accent`, `dim`, `info` semantic styles (StylePalette)
- [-] Configurable style palette — deferred (defaults work, config wiring later)

## Phase 15: Project Restructure
- [x] Restructure to per-module layout (`mods/<name>/scripts/`, `mods/<name>/nix/`)
- [x] Split monolithic `settings.nix` into per-module nix definitions
- [x] Create `mods/default.nix` assembler
- [x] Update `module.nix` to import from `mods/`

## Phase 16: New Modules
- [ ] Add keyboard layout module (layout switching, compositor events)
- [ ] Decide on tray/SNI support (in scope or explicitly out)
- [ ] Add idle/lock module (screen lock status, idle inhibitor toggle)

## Phase 17: Deferred Items
- [ ] Configurable style palette (from Phase 14)
- [ ] inotify file watcher for `SourceDef::File` (from Phase 10)
- [ ] `format_time(fmt)` Tera filter (from Phase 8)
- [ ] Structured module type checking in Nix (from Phase 12)
