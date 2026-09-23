---
status: accepted
title: Airplane mode writes /dev/rfkill directly
---

Recorded 2026-09-23 from the history; decided in `6e40845` (2026-08-13).

## Context

On 2026-08-09 a stray airplane-mode key press soft-blocked bluetooth.
systemd-rfkill persisted the block across reboots; NetworkManager clears a
wifi block when wireless is re-enabled, but BlueZ treats a kill switch as
deliberate and never clears one, so `bluetoothctl power on` failed with
`Failed (0x03)` for four days while every UI reported bluetooth as "off".

## Decision

A new `airplane` module reads `/sys/class/rfkill`, subscribes to
`/dev/rfkill` (a blocking read is an event stream) and toggles radios by
writing the kernel's 8-byte `rfkill_event` struct to it. The device node
carries a uaccess ACL for the logged-in user, so no privileged helper,
polkit rule or `rfkill` binary is needed. The root bar badges a full block
and, with a different icon, a partial one (some radios down, some up — the
state that hides). The bluetooth module reports "blocked" rather than "off"
and clears its own block before powering on.

## Alternatives

- **Shell out to `rfkill`**: one more runtime dependency for an 8-byte write.
- **A privileged helper or polkit action**: unnecessary given uaccess.

## Consequences

The failure mode that cost four days is now a visible badge and one key
press. The module depends on the uaccess ACL, which logind grants on a
normal seat but a nonstandard session setup might not.
