---
status: accepted
title: Draw with vello on wgpu, present through SHM
---

Recorded 2026-09-23 from the history; decided in `bbb2183` (2026-04-02).

## Context

The bar was rasterised on the CPU with tiny-skia. Pills with rounded
corners, fades and per-monitor scaling were getting more elaborate, and the
Retrobar spec's fixed character grid had already been replaced by a flex
layout.

## Decision

`gpu_render.rs` builds a vello scene (track, pills, text runs, tinted
icons) and `gpu.rs` renders it to a wgpu texture, reads the pixels back and
copies them into a Wayland SHM buffer. cosmic-text still shapes and
measures text; tiny-skia survives for icon and app-icon pixmaps.

## Alternatives

- **Stay on tiny-skia**: simpler, no GPU dependency, and adequate for a
  bar-sized surface.
- **Present a wgpu surface directly**: skips the readback, but the commit
  message records SHM as the reliable path for the layer surface.

## Consequences

Anti-aliased vector drawing at any scale. The binary now needs Vulkan or GL
at runtime — the NixOS module adds `/run/opengl-driver/lib` and the flake
patches the dlopen'd libraries onto the rpath (`15190fd`, after the binary
failed outside the devshell) — and the GPU round trip plus a readback per
frame is more machinery than a status bar strictly needs.
