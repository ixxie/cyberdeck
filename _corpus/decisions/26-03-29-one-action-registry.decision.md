---
status: accepted
title: One action registry, executed wherever it can run
---

Recorded 2026-09-23 from the history; decided in `ac6c7c7` and `2bdc4a6`
(2026-03-29).

## Context

"Isomorphic" — GUI, CLI and keybinding parity — is a stated principle. By
late March there were three disconnected ways to act on a module:
`[commands]` tables, shell strings in `[[key-hints]]`, and hardcoded native
dispatch in `cli.rs`. They drifted apart, and the CLI crashed when the bar
was not running.

## Decision

Each manifest has one `[[actions]]` table: `name`, `run` (a shell command,
or `"native"`), `label`, optional `key` and `icon`. A key press in the bar,
`cyberdeck <module> <action>` and an IPC `action` request all resolve
through it. Native actions run in whichever process received them:
`actions::exec_native` executes them in the CLI process when it can and
sends the bar a fire-and-forget `event` to show a toast; only actions that
need the bar's in-memory state return `BarOnly` and travel over the
socket.

## Alternatives

- **Everything through the bar over IPC**: simpler dispatch, but every
  keybinding breaks when the bar is down or restarting.
- **Keep per-path dispatch**: the drift that prompted this.

## Consequences

Compositor keybindings are just `cyberdeck <module> <action>`, and they
work with no bar running (volume, brightness, rfkill, power profiles). The
native-action match in `src/actions.rs` is one more place a new native
module has to be wired in.
