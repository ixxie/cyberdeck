---
title: Overview
---

# cyberdeck

cyberdeck is a keyboard-first desktop shell for the [Niri](https://github.com/YaLTeR/niri)
Wayland compositor: one bar per output, drawn on a layer-shell surface, that
doubles as launcher, notification daemon, audio and bluetooth control centre,
and a CLI that drives all of it. It is a single Rust binary (`cyberdeck`,
aliased `deck`) and a NixOS module.

It is the operator's daily shell on **contingent**, and a UX prototype for a
future desktop distro. It is in **maintenance mode**: it gets fixes when
daily use turns up a bug, and nothing else. The README's warning still holds
— "experimental, vibe coded software, works on my machine."

## What it does

- **The bar.** A root row of *badges* (one or more small icon/text elements
  per module) plus a centre *location* pill (workspace number and window
  title). Toasts appear transiently; a module can force a badge visible
  through a hook.
- **Navigation.** `cyberdeck launcher` (bound to `Mod+Space`) takes the
  keyboard. From there you type to search apps and modules, or enter a module
  to see its *widget*, its *deep view* (for interactive modules) and its key
  hints. `Esc` walks back up.
- **Parity.** Every module action is reachable three ways — a key in the bar,
  `cyberdeck <module> <action>` on the command line, and therefore a
  compositor keybinding. The volume, brightness and media keys on contingent
  are `cyberdeck audio up`, `cyberdeck brightness down`, and so on.
- **Modules.** 21 builtin modules: airplane, audio, bluetooth, brightness,
  calendar, clipboard, keyboard, media, mounts, network, notifications,
  profiles, recording, screenshot, session, storage, system, wallpaper,
  weather, window, workspaces. The launcher is built in and always on.

How these pieces fit is in [the architecture](corpus:article/architecture).

## Build and run

```sh
nix build            # the package, at ./result/bin/cyberdeck
nix develop          # cargo + rust-analyzer (fenix stable), pipewire, wayland headers
cargo check
cargo test           # config parsing, template compilation, rfkill states
```

The full procedure, including running a second bar against a test config
without disturbing the live one, is in [running cyberdeck](corpus:guide/running).

## How it is wired into the operator's system

cyberdeck is consumed by the operator's NixOS config (the `dotfiles` repo)
as a flake input pointing at this checkout:

```nix
cyberdeck.url = "git+file:///home/ixxie/repos/lab/cyberdeck";
```

A git+file input builds the *committed* tree, so a change only reaches the
laptop once it is committed, the input is re-locked
(`nix flake update cyberdeck`) and the system is switched (`yo gen switch`).
`dotfiles/modules/cyberdeck.nix` imports `nixosModules.default` and sets
`services.cyberdeck`: bottom position, the base16 background colour, 0.8
scale on `eDP-1` and `DP-2`, and the modules enabled — calendar, workspaces,
window, network, session, profiles, audio, bluetooth, airplane, system,
brightness, notifications, weather (Oraison), recording, screenshot, storage,
wallpaper.

The NixOS module (`nix/module.nix`):

- merges `mods.<id>` over each module's Nix defaults (`nix/mods/<id>.nix`:
  `enable`, `deps`, extra `services`, `env`), keeps the enabled ones, and
  writes the rest as a sparse JSON config;
- wraps the binary with the enabled modules' runtime dependencies on `PATH`
  and adds the Vulkan/GL driver paths;
- installs a systemd **user** service, `cyberdeck.service`, wanted by
  `graphical-session.target`, whose `preStart` links the JSON config into
  `~/.config/cyberdeck/config.json`;
- restarts that user service for every logged-in user from an activation
  script, so a switch reloads the bar.

Two other pieces of dotfiles config exist because of cyberdeck:
`modules/media.nix` turns off WirePlumber's
`bluetooth.autoswitch-to-headset-profile` (cyberdeck owns that
choreography, see
[the decision](corpus:decision/26-07-12-cyberdeck-owns-bt-profile-switching)),
and `modules/niri.nix` binds the launcher, the snip screenshot and the media
keys to `cyberdeck` commands.

## Status

- **Maintenance mode.** The last feature landed 2026-08-13 (the airplane
  module). The operator's ruling is that cyberdeck stays a prototype until
  the host arc lands; after that it becomes the basis of the desk distro.
- **Where the code lives.** `origin` (Codeberg, `ixxie/cyberdeck`) stops at
  2026-05-24; the sixteen commits since are local and on the `bacillus`
  remote only. The README still points installers at the GitHub mirror.
- **Consumer-ready it is not.** It hardcodes Niri, assumes `wayland-1`,
  starts its PipeWire backend whether or not audio is enabled, and one
  module still shells out to `swaymsg`. The gaps are filed as issues and
  gathered in [consumer readiness](corpus:issue/26-09-23-consumer-readiness).
- **Next move.** Relocate to the *playground* space in the new home layout —
  [the issue](corpus:issue/26-09-23-move-to-the-playground-space).

The dated story is in [the history](corpus:article/history).
