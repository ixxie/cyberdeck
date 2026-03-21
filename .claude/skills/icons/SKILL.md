---
name: icons
description: Manage the retrobar icon set — add, list, or verify Phosphor icons
user_invocable: true
---

# Icon Management Skill

Retrobar uses [Phosphor Icons](https://phosphoricons.com/) rendered as SVGs. The icon system has two parts:

1. **`assets/icons.json`** — Maps icon names to PUA (Private Use Area) codepoints. These codepoints are arbitrary unique identifiers used as placeholder characters in text rendering.

2. **Phosphor SVG assets** — The actual SVG files, sourced from the `phosphor-icons/core` flake input (pinned at rev `2b75f3ad12b420c9504ef05df8d2564a28f8500e`). SVGs are loaded by name from `{icons_dir}/{weight}/{name}-{weight}.svg`.

## How icons work

- Templates use `{{ "icon-name" | icon }}` or `icon(name="icon-name")` to insert an icon character
- The template engine looks up the name in `icons.json` to get a PUA codepoint
- The renderer maps that codepoint back to a name and renders the corresponding SVG
- If a name isn't in `icons.json`, it renders as `[icon-name]` (literal text fallback)

## Adding icons

When the user asks to add an icon or you encounter a `[icon-name]` rendering issue:

1. **Verify the icon exists in Phosphor** by fetching:
   `https://raw.githubusercontent.com/phosphor-icons/core/2b75f3ad12b420c9504ef05df8d2564a28f8500e/assets/duotone/{name}-duotone.svg`

2. **Pick a unique PUA codepoint** — scan `assets/icons.json` for the highest existing codepoint and use the next one, or pick any unused value in the `0xE000–0xF8FF` range. The exact value doesn't matter as long as it's unique within the file.

3. **Add the entry** to `assets/icons.json`:
   ```json
   "icon-name": "0xNNNN"
   ```

4. **Verify** with `cargo check` — the JSON is embedded at compile time via `include_str!`.

## Listing icons

All available icons are in `assets/icons.json`. To find what's currently registered:
```
cat assets/icons.json
```

## Finding icon names

Browse https://phosphoricons.com/ to find icon names. The name in `icons.json` should match the Phosphor name without the weight suffix (e.g., `sign-out` not `sign-out-duotone`).

## Where icons are referenced

- **Nix module configs** (`mods/*/name.nix`) — `icon` field, `indicator.template`, `key-hints[].icon`
- **Rust deep modules** (`src/mods/*.rs`) — hardcoded icon names in render functions
- **Template engine** (`src/template.rs`) — `render_icon()` method
