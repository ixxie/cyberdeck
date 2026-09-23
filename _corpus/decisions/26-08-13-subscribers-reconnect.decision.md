---
status: accepted
title: Subscriptions reconnect in the harness
---

Recorded 2026-09-23 from the history; decided in `4c0ee1d` (2026-08-13).

## Context

A subscribe function runs for the process lifetime; when it returned — its
service unreachable or gone — the thread simply ended and the module froze
on its last value. A NixOS switch restarted the bar at 22:01:08 and
NetworkManager at 22:01:18; the network module's single attempt hit
`ServiceUnknown` and it showed "disconnected" for eleven minutes, until a
manual restart. Any bluetoothd, PipeWire or NetworkManager restart could
strand any subscribe module the same way.

## Decision

Retry in the harness, not per module: when a subscription returns, wait
five seconds, re-seed by poll, and subscribe again. Re-seeding also
refreshes the data as soon as the service is back. Only a closed channel —
the bar itself is gone — ends the thread.

## Alternatives

- **Retry inside each module**: ten subscribe functions, ten chances
  to get it wrong.
- **Order the services** (systemd `After=` on NetworkManager and friends):
  covers boot and switch ordering, not a service restarting later.

## Consequences

Service restarts heal within seconds. A subscription that fails instantly
now retries every five seconds forever, logging a warning each time.
