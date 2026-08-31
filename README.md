# herdr-mission-control

Mission Control (exposé) for [herdr](https://github.com/herdrdev/herdr): on a shortcut, a full-screen popup shows every pane of the current workspace as tiles (content preview + agent status), grouped by tab. Selecting a tile switches focus to that pane.

Styled ANSI previews, adaptive and responsive grid.

## Install

```bash
herdr plugin install vjeantet/herdr-mission-control
```

Requires herdr 0.8.0+. The manifest's build step downloads the prebuilt binary published for this
version and your platform from this repository's GitHub releases, and verifies its SHA-256. Prebuilt
targets are macOS (Apple silicon and Intel) and Linux (x86_64, aarch64, armv7 - statically linked
against musl, so no glibc version to match). Anywhere else, or when a release is missing, it builds
from source instead and a [Rust toolchain](https://rustup.rs) is required.

For development, herdr runs your checkout directly. Rebuild after every change:

```bash
cargo build --release
herdr plugin link .
```

## Bind a key

If you don't use the [command palette](https://github.com/vjeantet/herdr-palette), add a shortcut in the herdr config:

```toml
[[keys.command]]
key = "prefix+e"
type = "plugin_action"
command = "vjeantet.mission-control.open"
description = "mission control"
```

Manual test without the shortcut:

```bash
herdr plugin pane open --plugin vjeantet.mission-control --entrypoint mission-control
```

## Keyboard

- arrows / `hjkl`: navigate between tiles
- `Enter`: switch to the selected pane
- `1`-`9`: direct jump
- `Backspace`: close the selected pane (with confirmation; Mission Control stays open for chained closes, and exits when nothing is left to show)
- `Esc` / `q`: quit

## Implementation notes

- herdr v1 plugin: manifest + subprocess. The binary talks directly to the API socket (`HERDR_SOCKET_PATH`, newline-delimited JSON, one request per connection): `session.snapshot`, `pane.read`, `pane.zoom`, `pane.close`. No CLI spawning: required for the live refresh.
- Live tile refresh at 4 Hz (previews, agent statuses, zoom states). The section/tile structure stays frozen from open time: the selection and the spatial layout do not move under the cursor.
- Focus by id does not exist in the API; we use `pane zoom <id> --on|--off` with the mode matching the tab's current zoom state: a no-op for the zoom, but `handle_pane_zoom` focuses the pane before checking the mode.
- Previews: `pane.read` source `visible`, format `ansi`, no line limit. The `recent*` sources window the tail of the rendered grid (blank rows below the cursor included), which returns all-blank text for a pane whose content sits at the top of a tall screen; `visible` reads the full viewport from row 0. Trailing blank lines are trimmed at render time before the tail window is applied.
- ANSI rendering: home-grown SGR parser (`src/ansi.rs`), sufficient because herdr regenerates the ANSI from its cell grid (pure SGR, no cursor sequences). Zero parsing dependencies.
- Grid: shrink to fit everything, 60×15 floor, then scrolling (persistent offset, adjusted only when the selection would leave the viewport). An intermediate "header-only degraded tiles" mode was tried and removed: it crushed everything while leaving the screen half empty.

## License

MIT - see [LICENSE](LICENSE).

