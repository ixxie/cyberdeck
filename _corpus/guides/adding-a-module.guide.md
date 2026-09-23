---
title: Adding a module
---

## Goal

Add a module to cyberdeck — either one that lives only in your config, or a
builtin that ships in the binary, with or without Rust behind it. How the
pieces fit is in [the architecture](corpus:article/architecture).

## Pick the kind

| you need | kind | touches |
|---|---|---|
| data from a shell command, badges, shell actions, just for you | **config-only** | your config |
| the same, shipped to everyone | **manifest-only builtin** | `mods/`, `nix/mods/` |
| data or actions that need Rust (D-Bus, a device file, a live stream) | **native builtin** | `mods/`, `nix/mods/`, `src/mods/`, `src/actions.rs` |
| a deep view with its own key handling | **interactive** | the native set, plus `create_interactive` |

## Config-only

Any `bar.modules.<id>` whose id is not a builtin is parsed as a complete
module definition. With the NixOS module:

```nix
services.cyberdeck.mods.uptime = {
  enable = true;
  name = "Uptime";
  icon = "clock";
  source = { type = "poll"; command = [ "sh" "-c" "printf '{\"s\":%d}' $(cut -d. -f1 /proc/uptime)" ]; interval = 60; };
  badges.default.template = "{{ s | human_duration }}";
};
```

Or in `~/.config/cyberdeck/config.toml` under `[bar.modules.uptime]`. The
CLI only knows builtins, so `cyberdeck uptime …` will not work for it —
see [the issue](corpus:issue/26-09-23-cli-cannot-reach-user-modules).

## Manifest-only builtin

The clipboard, mounts and screenshot modules are this kind: shell commands
and templates, no Rust.

1. Write `mods/<id>.mod.toml`: `name`, `description`, `icon`, a `[source]`
   (`poll` with a `command` that prints one JSON object, or `subscribe` with
   a command that prints one JSON object per line), `[badges.<name>]`,
   optionally `[widget]`, `[[hooks]]`, `[[actions]]` and `[[key-hints]]`.
   Copy `mods/clipboard.mod.toml` as a starting point; `type = "actions"`
   gives it the generic action palette as its deep view.
2. Write `nix/mods/<id>.nix` returning `{ enable = false; deps = [ … ]; }`
   with the packages your commands call; add `services`, `systemServices`
   or `env` if the module needs a companion daemon (see
   `nix/mods/wallpaper.nix`).
3. Add `<id> = importMod "<id>";` to `nix/mods/default.nix`.

`build.rs` picks the manifest up on the next build: it is embedded in the
binary and gets a CLI subcommand with one sub-subcommand per action. The
flake's source filter already includes `*.mod.toml`, but a new file must be
`git add`ed before `nix build` sees it.

## Native builtin

On top of the manifest-only steps (with `[source] type = "native"`,
`kind = "<id>"`, and actions that should run in Rust marked
`run = "native"`):

4. Write `src/mods/<id>.rs` with
   `pub fn poll(params: &serde_json::Map<String, Value>) -> Value` returning
   the module's JSON. For event-driven data also write
   `pub fn subscribe(params, sender: Sender<(String, Value)>, id: String)`,
   which blocks for the life of the process and sends `(id, json)` on every
   change. Return from it when the service goes away; the harness re-polls
   and re-subscribes after five seconds.
5. In `src/mods/mod.rs`: declare the module (`pub mod <id>;`) and add an arm
   to the `poll_fn` match in `register`, and to the `subscribe_fn` match if
   you wrote one.
6. In `src/actions.rs`: add an arm to `exec_native` for each `native`
   action. Return `ActionResult::Ok { toast }` when the CLI process can do
   the work itself, `BarOnly` when it needs the bar's state (the request is
   then forwarded over the socket).
7. Put `// corpus:article/architecture` at the top of `src/mods/<id>.rs`.

## Interactive

8. Implement `InteractiveModule` for a `<Name>Deep` struct in
   `src/mods/<id>.rs`: `render_center`, `key_hints`, `handle_key`, `reset`,
   and, as needed, `activate`, `cursor`, `tabs`/`active_tab` and
   `exec_action` (the path for `BarOnly` actions).
9. Set `type = "<id>"` in the manifest and add an arm to
   `create_interactive` in `src/mods/mod.rs`.

## Check it

```sh
nix develop -c cargo test   # compiles every builtin template
nix develop -c cargo run -- --config /tmp/deck-test.json
cyberdeck <id> --help
```

Templates are **Tera, not Jinja**. `selectattr`, `| list` and other
Jinja-only filters do not compile, and a template that does not compile
only logs an error — the badge never appears. Use
`filter(attribute="x", value=true) | length`, or a
`{% for %}{% if %}true{% endif %}{% endfor %}` loop for comparisons.
`builtin_templates_compile` catches this for builtins; config-only modules
have no such guard, so watch the log (`RUST_LOG=cyberdeck=info`) for
`bad badge template`.
