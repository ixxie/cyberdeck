---
status: todo
title: Niri lock-in
---

## Problem

[Niri is the only compositor](corpus:decision/26-03-21-niri-only), and the
coupling is spread through the code rather than behind a seam:

- `src/mods/workspaces.rs`, `src/mods/window.rs`: `niri msg --json` polls
  and the event stream;
- `src/nav.rs`: app launch (`spawn-sh`, with a direct-spawn fallback),
  workspace rename, overview;
- `src/actions.rs`: workspace rename and focus;
- `src/bar.rs`: `check_niri_changes` drives the location pill and the
  `defocus-on-niri-events` setting;
- `mods/workspaces.mod.toml`, `mods/session.mod.toml` (logout is
  `niri msg action quit`), `mods/screenshot.mod.toml` (focused output).

For a desk distro that is fine if the distro is Niri. For anything wider,
the first step is a compositor trait (workspaces, focused window, outputs,
spawn, quit) with Niri as its one implementation, so a second backend or
the portable `ext-workspace` / foreign-toplevel protocols can be added
without touching the modules.

## Thread
