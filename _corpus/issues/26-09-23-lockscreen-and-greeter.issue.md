---
status: open
title: Lockscreen and greeter
---

## Problem

From the README's todo list. "All you need to work is Niri + cyberdeck"
still needs a screen locker and a login greeter from elsewhere.

`plans/greeter.plan.md` (written in the Retrobar days) sketches both in the
same binary: `cyberdeck lock` on `ext-session-lock-v1` (already in
smithay-client-toolkit 0.19) with PAM, and `cyberdeck greet` speaking
greetd's IPC, sharing the renderer, fonts, icons and keyboard handling.
The plan predates the GPU renderer and the module system, so it needs a
refresh before it is used.

A screen locker is security-critical code; for a vibe-coded prototype this
is the feature to be most careful with.

## Thread
