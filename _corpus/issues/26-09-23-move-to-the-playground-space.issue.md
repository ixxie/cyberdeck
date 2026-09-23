---
status: open
title: Move cyberdeck to the playground space
---

## Problem

The new home layout groups projects by function. cyberdeck is a prototype,
so it belongs in `~/projects/playground/` (prototypes; agents free; no
backup) rather than in the workshop group (`lab/`, heading for
`~/projects/workshop/`), where it sits today as an adjacent, off-path
sibling.

Moving it means:

- relocating the checkout, and dropping it from the workshop's `just init`
  member list;
- changing the `dotfiles` flake input
  (`git+file:///home/ixxie/repos/lab/cyberdeck`) to the new path,
  re-locking, and a `yo gen switch` by the operator — until then the laptop
  keeps building from the old path;
- deciding whether playground is pushed to the forge, and where cyberdeck's
  canonical remote is (see
  [the stale release path](corpus:issue/26-09-23-release-path-is-stale)).

Waits for the playground group to exist.

## Thread
