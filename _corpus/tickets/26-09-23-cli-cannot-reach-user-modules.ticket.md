---
status: todo
title: The CLI cannot reach user-defined modules
---

## Problem

The config accepts complete user-defined modules (any id that is not a
builtin), but `cli.rs` builds its subcommands from `MOD_META`, generated at
compile time from `mods/*.mod.toml`, and `run_module_cmd` looks actions up
in the builtins only. `cyberdeck <user-module> <action>` is therefore
"unknown module", and a user module's actions cannot be bound to
compositor keys — breaking the GUI/CLI/keybinding parity the project
promises.

The CLI could load the same config the daemon does (or ask the daemon over
IPC for its module table) and fall back to it for ids it does not know.

## Thread
