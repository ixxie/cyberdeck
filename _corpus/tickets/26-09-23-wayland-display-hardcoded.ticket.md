---
status: todo
title: WAYLAND_DISPLAY is hardcoded to wayland-1
---

## Problem

The user service sets `WAYLAND_DISPLAY = "wayland-1"` in three places:
`nix/module.nix` (the bar), `nix/mods/wallpaper.nix` (the awww daemon) and
the unit that `install.sh` writes. It happens to be Niri's usual socket on
contingent. On a second session, a nested compositor, or a compositor that
names its socket differently, the bar connects to the wrong display or
none.

Graphical-session services should inherit the variable from the session:
Niri imports its environment into the systemd user manager when it starts,
so the fix is to drop the hardcoded value and keep
`After=graphical-session.target`.

## Thread
