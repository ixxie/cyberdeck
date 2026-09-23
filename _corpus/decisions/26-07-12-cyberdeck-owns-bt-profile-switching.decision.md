---
status: accepted
title: cyberdeck owns bluetooth profile switching
---

Recorded 2026-09-23 from the history; decided in `ef2a178` (2026-07-12),
refined in `5b4f122` (2026-07-17).

## Context

Bluetooth earbuds on contingent fell into connect/disconnect loops whenever
the microphone was used. The radio path (SCO, mSBC) was fine. The fault was
WirePlumber's on-demand autoswitch to the headset profile racing flaky HFP
setup — RFCOMM resets mid-open, transports acquired while dying — and
fighting explicit switches; sometimes the Handsfree service was never
established at connect at all.

## Decision

cyberdeck performs the profile choreography itself, deterministically, and
reports the stage that failed. Selecting a bluetooth mic: ensure the
headset profile exists (issue a BlueZ `Connect()` if it is missing), switch
to it, wait for the source node to appear, then move the default. Switching
away restores A2DP. Every failure becomes a toast, never a silent no-op.

From 2026-07-17 the switch follows demand: selecting a mic records a
*preference*, and the policy thread engages the headset profile only when a
real capture stream appears (filter chains, monitor taps and streams aimed
elsewhere do not count), restoring A2DP ten seconds after the last one
ends. A manual choice holds the policy off until the next demand edge.

The host side is part of the decision: the operator's config sets WirePlumber's
`bluetooth.autoswitch-to-headset-profile = false`.

## Alternatives

- **Tune WirePlumber's autoswitch**: the race is inside it, and it cannot
  report which stage failed.
- **Always-on headset profile**: telephone-quality playback all the time.

## Consequences

Mic use is reliable and playback returns to hi-fi on its own. cyberdeck now
depends on a host setting it cannot enforce outside the NixOS module's
reach — a consumer install would have both actors switching profiles.
