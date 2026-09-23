---
status: accepted
title: Audio talks to PipeWire natively
---

Recorded 2026-09-23 from the history; decided in `ef2a178` (2026-07-12).

## Context

The audio module enumerated devices with `pw-dump` and diffed `wpctl
status` output every 200 ms. It missed changes (device add/remove needed a
separate fix, `04423cb`), cost a subprocess per tick, and could not observe
the transient states — profiles, node errors, streams — that bluetooth
debugging needed.

## Decision

`src/pipewire.rs` uses libpipewire (pipewire-rs 0.8) on a dedicated thread:
registry, node, device and metadata listeners mirror the graph into a shared
`PwState`. Reads are a lock and a clone; a generation counter bumps on every
graph event, and the audio module's subscription waits on it instead of
polling. Volume, mute, default and profile changes travel over a command
channel into the loop and are applied as SPA pods. `cyberdeck pw` exposes
the backend standalone for debugging.

## Alternatives

- **Keep shelling out** to `wpctl`/`pw-dump`: no events, no streams, and a
  process per poll.
- **PulseAudio compatibility API** (libpulse): hides bluetooth profiles and
  PipeWire-specific state.
- **WirePlumber's Lua API**: moves logic into the session manager, outside
  the bar's control and tests.

## Consequences

Audio updates are event-driven and the backend sees everything it needs
for [owning bluetooth profile switching](corpus:decision/26-07-12-cyberdeck-owns-bt-profile-switching).
The binary links libpipewire, and the monitor thread starts with the daemon
whether or not the audio module is enabled
([the issue](corpus:issue/26-09-23-audio-backend-always-starts)). The
mod.toml's shell actions (`cyberdeck audio up`) still call `wpctl`.
