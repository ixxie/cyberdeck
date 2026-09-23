---
status: todo
title: btusb autosuspend is disabled only at runtime
---

## Problem

While chasing the bluetooth audio loops in July, USB autosuspend for the
bluetooth adapter (`btusb.enable_autosuspend`) was turned off by hand at
runtime. It was never made persistent in the operator's dotfiles, so every
reboot re-enables it.

It only matters if idle-time bluetooth disconnect cycles come back; the
failures fixed since (the native backend, pairing agent, watchdog, façade)
may have made it moot. The fix, if needed, is host config in `dotfiles`,
not cyberdeck code: `boot.kernelParams = [ "btusb.enable_autosuspend=0" ];`
(or the equivalent `extraModprobeConfig`), applied by the operator with
`yo gen switch`.

Close this either by adding the parameter, or by deciding after a stretch
of normal use that the disconnect cycles do not recur without it.

## Thread
