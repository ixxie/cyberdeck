---
status: accepted
title: Presentation is Tera templates over JSON
---

Recorded 2026-09-23 from the history; in force since `82aaba2 init`
(2026-03-21). The compile guard was added in `6e40845` (2026-08-13).

## Context

Every module produces JSON; the bar needs text and icons. The formatting
has to be editable per module and per user without recompiling, and has to
express conditions (show this badge only when muted), loops (one hexagon
per workspace) and small formatting helpers (meters, byte sizes).

## Decision

Badges, badge conditions and highlights, widgets, labels, and hook
conditions and actions are all **Tera** templates, rendered against the
module's JSON. The engine adds cyberdeck's own filters (`icon`, `meter`,
`bar`, `sparkline`, `human_bytes`, `human_duration`, `pad_left`,
`pad_right`, `color`, `dim`) and a `changed(key=…)` function for hooks. A
condition is true unless it renders empty, `false` or `0`.

Because a template that fails to compile is only logged and its badge
silently never appears, `modlib::tests::builtin_templates_compile` compiles
every builtin template in the test suite.

## Alternatives

- **A custom format language** (waybar-style `{placeholders}`): no
  conditionals or loops without reinventing an expression evaluator. The
  original Retrobar spec rejected it for that reason.
- **Jinja via minijinja**: closer to what people (and models) already
  write, but Tera was chosen first and nothing forced a migration.
- **Presentation in Rust**: loses per-user customisation.

## Consequences

Modules can be restyled from config, and config-only modules need no Rust.
The cost is Tera's divergence from Jinja: `selectattr`, `| list` and
friends look right and do not compile. Two builtin badges — bluetooth
"device connected" and storage "disk nearly full" — were written that way
and never rendered for months, until 2026-08-13. The builtins are now
guarded; user-written templates are not.
