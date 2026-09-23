---
status: accepted
title: Niri is the only compositor
---

Recorded 2026-09-23 from the history; in force since `82aaba2 init`
(2026-03-21).

## Context

A shell needs to know about workspaces, the focused window, outputs, and how
to launch apps. Wayland has no single protocol for all of that: each
compositor exposes its own IPC, and the portable protocols
(`ext-workspace`, `wlr-foreign-toplevel-management`) are partial and
unevenly supported. The operator runs Niri and nothing else.

## Decision

cyberdeck talks to Niri directly and to no other compositor. The workspaces
and window modules subscribe to `niri msg --json event-stream`; apps launch
through `niri msg action spawn-sh`; workspace rename, focus and overview,
full-output screenshots and logout are `niri msg action …` calls; the bar
defocuses on Niri events (`defocus-on-niri-events`). The README lists Niri
as a prerequisite.

## Alternatives

- **A compositor abstraction** (Niri, Sway, Hyprland backends behind one
  trait): triples the surface to test for a user base of one.
- **Portable protocols only**: `ext-workspace-v1` and foreign-toplevel would
  cover workspaces and titles but not launching, overview or renaming, and
  Niri's support was incomplete.

## Consequences

The Niri integration is deep and cheap: event-driven workspace and title
updates with no polling. It also makes cyberdeck unusable anywhere else,
which is the first thing a wider audience would hit — see
[compositor lock-in](corpus:issue/26-09-23-niri-lock-in). The keyboard
module never followed: it still calls `swaymsg`, so it does not work on the
one compositor cyberdeck supports
([the issue](corpus:issue/26-09-23-keyboard-module-calls-swaymsg)).
