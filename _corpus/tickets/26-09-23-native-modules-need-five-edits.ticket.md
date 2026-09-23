---
status: todo
title: A native module needs edits in five places
---

## Problem

Adding a native module (see [the guide](corpus:guide/adding-a-module))
touches `mods/<id>.mod.toml`, `nix/mods/<id>.nix`, `nix/mods/default.nix`,
`src/mods/<id>.rs`, up to three match statements in `src/mods/mod.rs`
(`poll_fn`, `subscribe_fn`, `create_interactive`) and one in
`src/actions.rs` (`exec_native`). Forgetting one fails quietly: an unknown
source kind is a log line, a missing action arm is "unknown native action"
at run time.

A registry would make a module one unit: a `NativeModule` value per module
(kind, poll, optional subscribe, optional interactive constructor, native
actions), collected in one list, with the dispatch tables derived from it.
A test could then assert every manifest with `type = "native"` has a
registered kind and every `run = "native"` action a handler — the same
move that `builtin_templates_compile` made for templates.

## Thread
