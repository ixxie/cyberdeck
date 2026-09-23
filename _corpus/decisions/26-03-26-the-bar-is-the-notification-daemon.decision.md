---
status: accepted
title: The bar is the notification daemon
---

Recorded 2026-09-23 from the history; decided in `78b41fc` (2026-03-26),
extended through 2026-04-02 (`12ad73e`, `66593d7`, `c9973e1`, `5087fc2`).

## Context

The bar has to show notifications — count, source, a way to dismiss — and
the "all you need is Niri + cyberdeck" goal rules out a separate daemon
(mako, dunst) whose state the bar would have to scrape.

## Decision

cyberdeck implements `org.freedesktop.Notifications` itself (zbus, in
`src/notifications.rs`) and claims the name on the session bus. A
notification becomes a toast and a per-app icon pill on the root bar;
notifications group by app, apps can be muted (no toasts), and a deep view
lists apps, drills into their notifications and dismisses or clears them.
App icons come from the notification's hints and are drawn as circular
grayscale images.

## Alternatives

- **Scrape an existing daemon** (`makoctl list`): no control over display,
  and two places notifications appear.
- **No notifications in the bar**: contradicts the one-shell goal.

## Consequences

Notifications are first-class in the bar's navigation. No other
notification daemon can run alongside, and a development build contends
with the running bar for the bus name (see
[running cyberdeck](corpus:guide/running)).
