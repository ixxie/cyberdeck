---
title: Running cyberdeck
---

## Goal

Build cyberdeck, run a development build against the live session, get a
change onto the operator's laptop, and debug the running bar.

## Build

```sh
cd ~/repos/lab/cyberdeck
nix develop                 # or let direnv load it (.envrc: use flake)
cargo check
cargo test
nix build                   # release package at ./result/bin/cyberdeck
```

The devshell has the fenix stable toolchain, rust-analyzer, cargo-watch and
the native inputs (wayland, libxkbcommon, fontconfig, freetype, Vulkan, GL,
pipewire). Outside Nix, `cargo build --release` needs the same libraries'
development headers.

## Run a development build

The daemon is a singleton in practice: it binds
`$XDG_RUNTIME_DIR/cyberdeck.sock` (removing whatever socket was there) and
claims `org.freedesktop.Notifications`. Stop the service before running your
own build, or the two fight over both.

```sh
systemctl --user stop cyberdeck
RUST_LOG=cyberdeck=debug cargo run -- --config ~/.config/cyberdeck/config.json
# Ctrl-C, then
systemctl --user start cyberdeck
```

`~/.config/cyberdeck/config.json` is the config the NixOS module generated,
so the development build sees the same modules and settings as the live one.
With no `--config` the daemon looks for `config.toml`, then `config.json`,
and falls back to four modules (calendar, workspaces, window,
notifications).

## Drive and inspect it

```sh
cyberdeck launcher                 # take the keyboard, search apps and modules
cyberdeck <module> <action> [args] # e.g. cyberdeck audio up, cyberdeck airplane toggle
cyberdeck <module> open            # navigate the bar to a module
cyberdeck state                    # the navigation state, as JSON
cyberdeck dismiss
cyberdeck theme pills              # classic | floating | pills | transparent, until restart
cyberdeck pw status                # the PipeWire mirror: nodes, bluetooth devices, profiles
cyberdeck pw watch <name>          # run the mic-demand policy in the foreground, narrated
journalctl --user -u cyberdeck -f  # the service's log
```

`cyberdeck pw …` runs its own PipeWire connection, so it works without the
bar. `pw watch` starts a second policy actor; stop the bar first if you want
it to be the only one.

## Ship a change to contingent

The operator's `dotfiles` flake takes cyberdeck as
`git+file:///home/ixxie/repos/lab/cyberdeck`, which builds the committed
tree only.

1. Commit the change in this repo (a new file must at least be `git add`ed
   for Nix to see it).
2. In `dotfiles`: `nix flake update cyberdeck`.
3. The operator runs `yo gen switch`. Agents never rebuild or switch the
   laptop.

The module's activation script restarts `cyberdeck.service` for every
logged-in user on switch, so the new bar is live straight away.

## Other distros

`install.sh` downloads the latest GitHub release, installs the binary to
`~/.local/bin`, Phosphor icons to `~/.local/share/cyberdeck/icons`, and
`cyberdeck.service` as a user unit. Then:

```sh
deck init                              # writes a commented ~/.config/cyberdeck/config.toml
systemctl --user enable --now cyberdeck
```

This path has not been touched since 2026-03-28, before the PipeWire
dependency arrived; see
[the issue](corpus:ticket/26-09-23-release-path-is-stale).
