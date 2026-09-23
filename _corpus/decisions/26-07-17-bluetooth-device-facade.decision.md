---
status: accepted
title: Bluetooth audio devices sit behind a façade
---

Recorded 2026-09-23 from the history; decided in `2ac9faa` (2026-07-17),
hardened in `0d8ad95` (2026-07-30).

## Context

A profile flip (A2DP ↔ HFP) destroys the device's nodes and creates new
ones. WirePlumber re-links app streams, but they stay parked in
`suspended` and never resume, so music stopped on every transition. Desktop
operating systems have the same problem; phone operating systems avoid it
by hiding devices behind an audio-server abstraction.

## Decision

Take the phone architecture. Each bluetooth headset gets a persistent
virtual sink and source (`deck_sink.<mac>`, `deck_mic.<mac>`), run as a
`pipewire -c` loopback child process like the denoise chain. Apps and the
defaults only ever see the façade; profile flips re-plumb hidden backend
streams underneath. Selecting a bluetooth mic no longer flips the profile —
hi-fi holds until something captures. Parked streams are kicked after any
switch.

After the first deployment (`0d8ad95`) no backend carries a static target:
`module-loopback` self-destructs at init when its configured target does not
exist, which a spawn racing a flip triggered. Both backends are pinned at
runtime on every graph wake, liveness is re-verified instead of trusted,
and the policy adopts an existing façade after a restart.

## Alternatives

- **Resume parked streams only**: fixes the stall, not the gap or the
  default hopping between nodes.
- **Accept the interruption**: what every desktop OS does; the operator
  did not.

## Consequences

A 45-second tone played uninterrupted across a demand-driven engage and an
idle restore. The UI dropped the a2dp/hfp tags; a row shows `[call]` while
the headset link is live. The cost is a child process per headset and the
subtlest code in the repository — `pipewire.rs` is 1,581 lines.
