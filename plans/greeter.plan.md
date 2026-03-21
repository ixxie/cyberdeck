# Retrobar Lockscreen & Greeter

## Context

Add a lockscreen and greeter to retrobar, reusing its existing rendering engine, keyboard handling, and Wayland infrastructure. The lockscreen prevents desktop session access; the greeter handles login via greetd. Both share the same full-screen UI.

smithay-client-toolkit 0.19 already includes `ext-session-lock-v1` support, so the protocol layer is ready. The rendering pipeline (cosmic-text, tiny-skia, Phosphor icons) scales directly from bar-height to full-screen.

## Architecture

### Two modes, one binary

```
retrobar                → bar mode (default, current behavior)
retrobar lock           → lockscreen mode (ext-session-lock-v1 + PAM)
retrobar greet          → greeter mode (greetd IPC)
```

Shared code: renderer, font system, layout engine, keyboard handling, color system, icon set.

### Phase 1: Lockscreen

**New files:**
- `src/lock.rs` — `LockApp` struct, session lock protocol handling, PAM auth, UI rendering

**Modified files:**
- `src/main.rs` — dispatch `lock` subcommand
- `Cargo.toml` — add `pam-client` dependency

**Protocol flow:**
1. Connect to Wayland, bind `ext_session_lock_manager_v1`
2. Call `lock()` → compositor sends `locked` event
3. Create lock surfaces per output (full-screen, receives all input)
4. Render password prompt UI using existing `Renderer`
5. Handle keyboard input → accumulate password characters
6. On Enter: authenticate via PAM
7. On success: call `unlock_and_destroy()`, exit
8. On failure: show error, clear password, retry

**UI layout (rendered via existing Renderer + Layout):**
- Center: clock (large) + date
- Below: password input field (masked with `●` characters)
- Below: status text ("incorrect password" / "locked" / etc.)
- Background: solid color or wallpaper (reuse wallpaper mod's current image)

**PAM integration:**
- Use `pam-client` crate
- Authenticate against the `login` service (or configurable)
- Run PAM in the main thread (lockscreen is single-purpose, no async needed)
- PAM config: `/etc/pam.d/retrobar` (provided by NixOS module)

**Security considerations:**
- Never dismiss lock surface without successful PAM auth
- Clear password from memory after auth attempt (`zeroize` crate)
- If lock surface creation fails, keep trying (don't silently unlock)
- Handle compositor disconnect → process exits, session stays locked (protocol guarantee)

**IPC integration:**
- `retrobar lock` sent from CLI triggers the running bar to enter lock mode
- Or: lockscreen runs as a separate process (simpler, more secure — crash isolation)
- Recommendation: **separate process** — the bar and lockscreen don't need to share state

**NixOS module additions:**
- PAM config for retrobar (`security.pam.services.retrobar`)
- Systemd service for lock-on-idle (via `swayidle` or `systemd-logind` idle action)
- `loginctl lock-session` integration (listens for lock signal)

### Phase 2: Greeter

**New files:**
- `src/greet.rs` — `GreetApp` struct, greetd IPC, user/session selection

**Modified files:**
- `src/main.rs` — dispatch `greet` subcommand
- `Cargo.toml` — add `greetd_ipc` dependency

**greetd IPC flow:**
1. Connect to greetd socket (`$GREETD_SOCK`)
2. Render username prompt
3. Send `create_session(username)` → receive auth challenge
4. Render password prompt
5. Send `post_auth_message_response(password)` → receive success/error
6. On success: `start_session(cmd, env)` to launch the user's desktop

**UI:** Same as lockscreen but with username field above password field, and optional session selector.

**NixOS module:**
- greetd config pointing to `retrobar greet` as the greeter command
- Fallback greeter config (tuigreet) for safety during development

### Shared infrastructure

**What's reused from the bar:**
| Component | How |
|-----------|-----|
| `Renderer` | Same text/icon rendering, just on a larger surface |
| `Layout` + `RenderedWidget` | Same flex layout for positioning UI elements |
| `Rgba` + color system | Same color handling |
| `IconSet` | Same Phosphor icons for UI elements (lock icon, user icon) |
| Keyboard handling | Same keysym/modifier parsing |
| Config loading | Lockscreen config could be a section in retrobar config |

**What's new:**
| Component | Details |
|-----------|---------|
| `ext-session-lock-v1` | Full-screen lock surfaces (smithay-client-toolkit has this) |
| PAM auth | `pam-client` crate, ~30 lines |
| greetd IPC | `greetd_ipc` crate, ~50 lines |
| Password input widget | Masked text field with cursor |
| Full-screen layout | Centered vertical layout instead of horizontal bar |

### New dependencies

```toml
pam-client = "0.5"       # PAM authentication
greetd_ipc = "0.10"      # greetd protocol (phase 2)
zeroize = "1"             # secure password clearing
```

## File structure

```
src/
  main.rs          # dispatch: bar / lock / greet
  bar.rs           # existing bar (unchanged)
  lock.rs          # lockscreen app (new)
  greet.rs         # greeter app (new, phase 2)
  render.rs        # shared renderer (unchanged)
  layout.rs        # shared layout (unchanged)
  config.rs        # add lockscreen config section
  ...
```

## Development safety

- **Test in nested compositor**: `niri -- retrobar lock` inside existing session
- **Failsafe timer in debug builds**: auto-unlock after 30s if `RETROBAR_DEBUG_LOCK=1`
- **greetd fallback**: always configure `tuigreet` as fallback greeter
- **TTY escape**: Ctrl+Alt+F2 always works regardless

## Implementation order

1. Scaffold `src/lock.rs` with LockApp struct + session lock protocol
2. Render a static full-screen surface (solid color + centered text)
3. Add keyboard input → password accumulation
4. Add PAM authentication
5. Add clock/date display
6. Add NixOS module (PAM config, idle lock service)
7. Polish UI (wallpaper background, animations)
8. Phase 2: greeter (greetd IPC, username input)

## Verification

1. `cargo check` compiles with new deps
2. Test in nested compositor: `niri -- retrobar lock`
3. Verify lock surface covers entire screen
4. Type password → verify PAM auth works
5. Wrong password → verify error display, retry
6. Correct password → verify unlock + process exit
7. Kill lockscreen process → verify session stays locked (protocol guarantee)
