---
status: accepted
title: Modules are TOML manifests embedded in the binary
---

Recorded 2026-09-23 from the history; decided in `ff5ab7f` (2026-03-26).

## Context

At `init` a module was a Nix file (`mods/<id>/<id>.nix`) holding its whole
definition — source, templates, commands — so it existed only for NixOS
users and the CLI had to be taught each module by hand. Meanwhile the
Retrobar spec's premise, that modules are external scripts, had already
given way to native Rust modules for anything that needed D-Bus or a
stream.

## Decision

Each module is a portable manifest, `mods/<id>.mod.toml` — name, icon,
`type`, `[source]`, badges, widget, hooks, actions, key hints, params. The
source is `poll`, `subscribe` or `file` for script-backed modules and
`native` for Rust ones, so both kinds share one format. `build.rs` embeds
every manifest in the binary and generates the CLI's module subcommands
from their action names. Nix keeps only packaging concerns in
`nix/mods/<id>.nix`: `enable`, `deps`, companion `services`, `env`.
User config is a sparse override deep-merged over the builtin manifest; an
id that is not a builtin is a complete user-defined module.

## Alternatives

- **Keep modules in Nix**: excludes every non-NixOS user and keeps the CLI
  out of sync.
- **Read manifests from disk at runtime**: lets users drop in files, but
  makes the binary depend on an install layout and loses the compile-time
  CLI generation.
- **Modules as pure Rust**: no per-user restyling without a rebuild.

## Consequences

One binary carries its modules; the CLI, the bar and the NixOS module agree
on them by construction. A native module still has to be registered in
several hand-written match statements
([the issue](corpus:issue/26-09-23-native-modules-need-five-edits)), and the
CLI knows only builtins
([the issue](corpus:issue/26-09-23-cli-cannot-reach-user-modules)).
