---
status: todo
title: The audio backend and watchdog start even when unused
---

## Problem

`main` starts `pipewire::start()`, `pipewire::start_policy()` and
`watchdog::spawn()` unconditionally, before it knows which modules are
enabled. A user without the audio module still gets a PipeWire client
thread, the mic-demand policy (which can switch bluetooth profiles), and a
watchdog that queries BlueZ every three seconds and may reconnect devices.
On a system without PipeWire the monitor thread has nothing to talk to.

Start the backend only when a module that needs it (`audio`, and
`bluetooth` for the watchdog) is enabled; the policy in particular should
never run for a user who did not opt into cyberdeck's bluetooth
switching.

## Thread
