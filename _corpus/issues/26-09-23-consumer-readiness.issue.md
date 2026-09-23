---
status: open
title: What "consumer-ready" would take
---

## Problem

cyberdeck works on one laptop for one user. Shipping it to anyone else — or
basing the desk distro on it — needs more than fixes; it needs a few
structural changes. This issue gathers the gaps found while writing the
corpus. Most are filed separately:

- **Compositor.** Niri only, by design —
  [Niri lock-in](corpus:issue/26-09-23-niri-lock-in); and the keyboard
  module does not even work on Niri —
  [swaymsg](corpus:issue/26-09-23-keyboard-module-calls-swaymsg).
- **Session assumptions.** `WAYLAND_DISPLAY=wayland-1` is hardcoded —
  [the issue](corpus:issue/26-09-23-wayland-display-hardcoded).
- **Hard dependencies.** The PipeWire backend and the bluetooth watchdog
  start whether or not audio or bluetooth are enabled —
  [the issue](corpus:issue/26-09-23-audio-backend-always-starts).
- **Host policy it cannot enforce.** Deterministic bluetooth switching
  assumes WirePlumber's autoswitch is off; outside the operator's dotfiles
  nothing turns it off, and the two will fight
  ([the decision](corpus:decision/26-07-12-cyberdeck-owns-bt-profile-switching)).
- **Exclusive services.** The bar is the notification daemon; installing it
  beside mako or dunst fails in a way the user has to diagnose.
- **Extensibility.** A native module means edits in five places
  ([the issue](corpus:issue/26-09-23-native-modules-need-five-edits)), and
  user-defined modules cannot be driven from the CLI
  ([the issue](corpus:issue/26-09-23-cli-cannot-reach-user-modules)).
  User templates get no compile check, so a Jinja-ism is a silently
  missing badge.
- **Distribution.** The release workflow and installer predate the
  PipeWire dependency and point at a mirror sixteen commits behind —
  [the issue](corpus:issue/26-09-23-release-path-is-stale).
- **Documentation.** The README describes modules and settings that no
  longer exist — [the issue](corpus:issue/26-09-23-readme-has-drifted).
- **Defaults.** The NixOS module runs with `RUST_LOG=cyberdeck=debug`;
  there is no settings UI, no first-run flow beyond `deck init`, and no
  screen lock or greeter
  ([the issue](corpus:issue/26-09-23-lockscreen-and-greeter)).
- **Quality.** 23 unit tests, all on config parsing, templates and rfkill
  states; nothing exercises the bar, navigation or the PipeWire backend
  outside the operator's hardware. The README's own warning — "vibe coded
  … works on my machine" — is accurate.

None of this is scheduled: cyberdeck stays a prototype until the host arc
lands. This is the checklist for when it becomes the desk distro.

## Thread
