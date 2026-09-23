---
title: History
---

# History

46 commits from 2026-03-21 to 2026-08-13, all by the operator, 43 of them
co-authored by a Claude model. About 29,600 lines added and 8,400 removed.
The work came in two bursts — building the shell in March and April,
hardening bluetooth audio in July — with long gaps in between and after.

## Cadence

| month | commits | what happened |
|---|---|---|
| 2026-03 | 8 | the shell is built; the spec is dropped |
| 2026-04 | 18 | polish burst, 15 of them on 04-02; GPU rendering |
| 2026-05 | 4 | audio module replaces inputs/outputs; fenix toolchain |
| 2026-06 | 0 | |
| 2026-07 | 14 | bluetooth audio hardening; native PipeWire backend |
| 2026-08 | 2 | airplane module; subscribe reconnect |
| 2026-09 | 0 | ruled a prototype; this corpus |

The busiest days: 2026-04-02 (15 commits), 2026-07-10 (8), 2026-03-26 (4).

## Before the first commit: Retrobar

The project began on 2026-03-21 as **Retrobar**, "a minimal, recursively
composable Wayland status bar that renders to a single-row character grid".
An orphan `WIP` commit from that morning survives on the local branch
`foo`: a `SPEC.md` (1,037 lines), `PLAN.md` and `TODO.md`. The spec's
rationale appendix argued for a fixed character grid instead of pixel layout,
for **external scripts instead of built-in modules** ("built-in modules
create a dependency maze"), for Tera templates, for calloop over tokio, and
for a recursive module tree navigated like a filesystem.

## Foundation — 2026-03-21 to 2026-03-29

`82aaba2 init` (2026-03-21) landed 10,783 lines at once: the spec, plan and
todo, a README already calling the project **cyberdeck**, a NixOS module,
and — against the spec's own argument — 21 native Rust modules under
`src/mods/`. Tera, calloop and the IPC socket survive from the spec to this
day; the character grid and the script-only modules did not.

The next five days were 32 small commits, later squashed into four on
2026-03-26 (the originals are on the local branch `backup-pre-rebase`):

- `ff5ab7f` modules move out of Nix into portable `mods/*.mod.toml`
  manifests embedded by `build.rs`, and the clap CLI is generated from them
  — [module manifests](corpus:decision/26-03-26-module-manifests);
- `78b41fc` cyberdeck becomes the D-Bus notification daemon, with circular
  grayscale app icons, and gets a first PipeWire helper (`pw-dump` based) —
  [the bar is the notification daemon](corpus:decision/26-03-26-the-bar-is-the-notification-daemon);
- `38527c5` the bar's zones settle: launcher left, navigation centre,
  toasts right; interactive modules get the `KeyResult` enum;
- `6456908` a unified style system (track and pill padding).

On 2026-03-28 `9ee2a55` overhauled rendering and config (kebab-case serde,
per-monitor scale) and **deleted SPEC.md, PLAN.md and TODO.md**, adding
the GitHub release workflow, `install.sh` and the systemd unit instead. The
flake description and `plans/greeter.plan.md` still say Retrobar.

2026-03-29 finished the model: `ac6c7c7` replaced three separate action
paths with one `[[actions]]` table
([one action registry](corpus:decision/26-03-29-one-action-registry)), and
`2bdc4a6` let the CLI run native actions without the bar and moved Niri,
media and network onto event subscriptions
([event-driven sources](corpus:decision/26-03-29-event-driven-sources)).

## Polish — 2026-04-01 to 2026-04-04

Eighteen commits, fifteen on 2026-04-02: the centre *location* pill
(workspace number and window title, iterated five times in one afternoon),
per-app notification icons with grouping, mute, hover previews and a deep
view, workspace actions, and pagination fixes. `bbb2183` moved drawing
from tiny-skia to vello on wgpu
([GPU rendering](corpus:decision/26-04-02-gpu-rendering-with-vello)).
Two unlabelled `wip` commits bracket the burst; the second (`b4d78d2`,
2026-04-04) grew the config schema by 565 lines and the network module by
858.

## Consolidation — 2026-05-17 to 2026-05-24

After six quiet weeks, `3ba6a80` merged the separate `inputs` and `outputs`
modules into one `audio` module with speaker and microphone tabs, added
`profiles`, and grew bluetooth by 445 lines. Then a Codeberg→GitHub mirror
badge, and `a512e1d` moved the Rust toolchain to fenix and added direnv.
That commit is the last one on `origin` (Codeberg).

## Bluetooth audio — 2026-07-10 to 2026-07-30

Seven more quiet weeks, then fourteen commits driven by daily use of
bluetooth earbuds on contingent, whose headset profile kept dropping into
connect/disconnect loops.

- 2026-07-10: discovery scans that ended instantly (BlueZ stops discovery
  when the requesting client leaves the bus, `b1caff9`); the binary dying
  outside the devshell because dlopen'd libraries were missing from the
  rpath (`15190fd`); the audio list not refreshing on device add/remove
  (`04423cb`); bluetooth profiles and node errors surfaced in the UI
  (`addd5d9`); the **watchdog** that reconnects wedged bluetooth audio
  (`1e46847`); bonding and trusting devices on connect (`66f31f9`); and
  freezing the screen during region screenshots (`d02b819`), then making
  it unfreeze when the capture ends (`a5b54c2`).
- 2026-07-11: bonds evaporated on disconnect until a pairing agent was
  registered (`dfcdb6d`).
- 2026-07-12: `ef2a178` replaced `pw-dump`/`wpctl` polling with a native
  libpipewire mirror and took profile switching away from WirePlumber —
  [native backend](corpus:decision/26-07-12-native-pipewire-backend),
  [deterministic switching](corpus:decision/26-07-12-cyberdeck-owns-bt-profile-switching).
- 2026-07-17: the headset profile starts following actual microphone use
  (`5b4f122`), and the **device façade** keeps playback alive across
  profile flips (`2ac9faa`) —
  [the façade](corpus:decision/26-07-17-bluetooth-device-facade).
- 2026-07-30: `0d8ad95`, fixes from the façade's first real deployment:
  a spawn racing a profile flip left half a façade, and a stale headset
  profile survived restarts.

## Maintenance — 2026-08-13 onward

Two commits on one day, both from incidents:

- `6e40845`: a stray airplane-mode key press on 2026-08-09 soft-blocked
  bluetooth; systemd-rfkill persisted the block across reboots and BlueZ
  never clears one, so bluetooth was dead for four days while every UI said
  "off". The new **airplane** module shows and clears rfkill blocks
  ([rfkill without a helper](corpus:decision/26-08-13-rfkill-without-a-helper)).
  Testing it exposed two badges — bluetooth "device connected" and storage
  "disk nearly full" — written in Jinja syntax that Tera cannot compile,
  which had therefore **never rendered**. A test now compiles every
  builtin template ([Tera templates](corpus:decision/26-03-21-tera-templates)).
- `4c0ee1d`: a NixOS switch restarted the bar ten seconds before
  NetworkManager; the network module's one subscription attempt failed and
  it showed "disconnected" for eleven minutes. Subscriptions now reconnect
  ([subscribers reconnect](corpus:decision/26-08-13-subscribers-reconnect)).

Nothing has been committed since. On 2026-09-23 the operator ruled that
cyberdeck stays a prototype until the host arc lands, and this corpus was
written.

## Loose ends in the repository

- Sixteen commits (2026-07-10 onward) are not on `origin`; they exist
  locally and on the `bacillus` remote.
- Local branches `foo` (the orphan Retrobar WIP), `backup-pre-rebase` (the
  32 pre-squash March commits) and `next` (at `88de0f0`, also on origin)
  are history, not work in progress.
- `test-config.json` is in the Retrobar-era config format (`groups`,
  `toggle-key`, `background`), which the current loader does not
  understand.
