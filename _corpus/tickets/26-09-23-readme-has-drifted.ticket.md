---
status: todo
title: The README has drifted from the code
---

## Problem

The README's content was last updated on 2026-05-17 (later edits only
touched the mirror badge). Since then, and in places before:

- the module table lists `outputs` and `inputs` (merged into `audio` in
  May) and lacks `audio` and `airplane`; it gives `wireplumber` as the audio dependency (now libpipewire, natively)
  and `swww` for wallpapers (now awww);
- the settings example uses `layout = "pills"`; the key is `theme`, and the
  default is `classic`, not `pills`;
- the example's `[settings.theme]` table would not even parse, since
  `theme` is a string; its `color`/`opacity`/`radius`/`padding` and
  `[settings.theme.track]`/`[settings.theme.pill]` do not match
  `RuntimeSettings`, which has top-level `color`, `radius`, `material`,
  `bar` and `pill`;
- the source-layout section omits `pipewire.rs`, `watchdog.rs`,
  `notifications.rs`, the GPU renderer and `actions.rs`;
- `flake.nix`'s description and `plans/greeter.plan.md` still say
  "Retrobar", and `test-config.json` is in the Retrobar-era format.

The corpus now carries the architecture; the README should shrink to what
it is, how to install it, and a pointer to `_corpus/`.

## Thread
