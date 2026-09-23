---
status: open
title: The keyboard module calls swaymsg
---

## Problem

`mods/keyboard.mod.toml` reads layouts with `swaymsg -t get_inputs | jq …`
and `src/mods/keyboard.rs` switches with
`swaymsg input type:keyboard xkb_switch_layout N`. cyberdeck only supports
Niri, where neither works, so the module is dead code on the one supported
compositor. It is not enabled on contingent, which is probably why nobody
noticed; the README still lists `swaymsg` as its dependency.

Niri exposes the same through `niri msg --json keyboard-layouts` and
`niri msg action switch-layout <next|prev|N>`; porting is a small change.
Alternatively drop the module.

## Thread
