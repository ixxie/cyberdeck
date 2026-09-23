---
status: todo
title: The release path and remotes are stale
---

## Problem

The non-NixOS path has not been touched since 2026-03-28:

- `.github/workflows/release.yml` installs wayland, xkbcommon, fontconfig
  and freetype headers but not `libpipewire-0.3-dev` (and clang for
  bindgen), which the build has needed since `ef2a178`; it also runs
  `cargo clippy -- -D warnings`, and the build currently has warnings. The
  workflow would fail.
- `install.sh` downloads the latest GitHub release of `ixxie/cyberdeck`,
  and the README tells users to add `github:ixxie/cyberdeck` as a flake
  input. GitHub is a mirror of Codeberg.
- Codeberg `origin/main` is at `a512e1d` (2026-05-24); the sixteen commits
  since exist locally and on the `bacillus` remote only.

Decide the canonical remote (see
[the playground move](corpus:ticket/26-09-23-move-to-the-playground-space)),
push to it, then either fix the workflow's dependencies or drop the
binary-release path until someone needs it.

## Thread
