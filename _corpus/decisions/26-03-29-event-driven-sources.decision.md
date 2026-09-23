---
status: accepted
title: Latency-sensitive modules subscribe instead of polling
---

Recorded 2026-09-23 from the history; decided in `2bdc4a6` (2026-03-29).

## Context

Workspaces, the window title, media and network changed on screen a poll
interval late, and fast polling of external commands costs a process per
tick.

## Decision

A native module may provide a `subscribe` function next to its `poll`.
The harness in `src/mods/mod.rs` seeds the module's state with one poll and
then runs the subscription, which pushes JSON on every change: Niri's event
stream for workspaces and window, `playerctl --follow` for media,
NetworkManager and BlueZ signals over D-Bus for network and bluetooth,
`/dev/rfkill` for airplane, the PipeWire mirror's generation counter for
audio. Modules without a subscription keep polling on their `interval`,
and can be nudged to re-poll right after an action.

## Alternatives

- **Faster polling everywhere**: latency still bounded by the interval, and
  CPU cost grows with it.
- **Async runtime (tokio) for all sources**: the Retrobar spec already
  chose calloop to match smithay; threads plus a calloop channel keep that.

## Consequences

The bar updates instantly on workspace switches and media changes. A
subscription is a long-lived connection that can die with its service —
the gap closed by
[subscribers reconnect](corpus:decision/26-08-13-subscribers-reconnect).
