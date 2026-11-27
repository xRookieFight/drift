# AGENTS.md

Guidance for coding agents (Claude Code, Codex and others) working on Drift. Keep this file current: any change to architecture, commands, layout or conventions updates it in the same commit.

## What Drift is

A Linux first desktop browser on the Servo engine (crates.io `servo`, pinned in Cargo.toml). The browser UI is HTML/CSS/JS rendered by Servo in its own webview, the Rust core owns windows, tabs and layout. There is one engine only, no fallback.

## Current state

v0.1 in progress. The shell runs: sidebar UI, tabs, split view, dialogs, session restore. See the roadmap in README.md for what is next.

## Layout

```
src/
  main.rs       entry, CLI args (--no-restore, --profile DIR, URL)
  app.rs        winit event loop, owns Servo, the window and all webviews, routes input,
                composites the frame, implements WebViewDelegate (through DelegateProxy)
  tabs.rs       Tabs, workspaces, essentials, focus and split state (pure, unit tested)
  layout.rs     rect math: sidebar, content card, split panes (pure, unit tested)
  bridge.rs     JSON messages between UI and core (Command, UiState)
  protocol.rs   drift:// scheme: embedded ui/ assets and the bridge endpoint
  dialogs.rs    alert/confirm/prompt, <select> and context menu queue shown by the UI
  session.rs    session save/restore (~/.local/share/drift/session.json)
  omnibox.rs    address field input to URL or search
  favicon.rs    favicon pixels to PNG data URLs
  keys.rs       browser shortcuts, checked before input reaches webviews
  keyutils.rs   winit to Servo keyboard event mapping (adapted from servoshell)
  theme.rs      light/dark from the XDG portal (winit reports nothing on Wayland)
  compositor.rs mixes floating UI boxes (panel, dialogs, menus) over the pages on the CPU
  history.rs    visit history, frecency suggestions and inline site completion
  picker.rs     <input type=file> through the XDG portal file chooser (rfd), on a worker thread
  content.css   user stylesheet injected into every page (focus ring fix)
  updater.rs    GitHub Releases check, verified download and atomic swap
ui/             sidebar UI, plain HTML/CSS/JS, no framework, embedded at build time
packaging/      desktop file, icon, release scripts
scripts/        cargo-in-container.sh
docker/         build environment image (published to GHCR)
```

### How a frame is drawn

Every webview (the UI and each visible tab) renders into its own OffscreenRenderingContext. `Running::paint` blits the UI full window first, then each pane at its layout rect. The UI draws the backdrop, the card shadow, the split divider and the focus ring around where the panes land, using the rects the core sends in UiState.

### Floating UI

The UI webview is drawn *under* the pages, so anything that must cover a page (the address panel, page dialogs, select popups, context menus) is reported by the UI with `set_overlays` as a list of boxes (rect, radius, shadow, scrim). Each frame `Running::paint_overlays` reads that region back from the window and from the UI framebuffer and `compositor::composite` mixes them: rounded corners and the shadow show the live page, the scrim dims it. Input inside a reported box goes to the UI. New floating UI must report its box in `reportFloats()` in ui/app.js, nothing else is needed on the Rust side.

### Workspaces and essentials

Every tab has a workspace; only the current workspace's tabs are in view (`Tabs::in_view_ids`). Pinned tabs are essentials: shown as tiles in every workspace and kept in one block at the front of the list. Sidebar positions sent by the UI (drag and drop) are positions in what the sidebar shows, never in the full list. Session files are version 2; version 1 files still load into the default workspace.

### Compact mode

Ctrl+Alt+C or the top bar button hides sidebar and top bar and gives the card the whole window. `Running::check_compact_edges` calls `drift.reveal('sidebar' | 'topbar')` when the pointer touches the left or top edge, and `drift.reveal(null)` when it moves back onto a page. The revealed chrome is a floating box like any other.

### Window chrome

The window has no server side decorations. The top bar carries minimize, maximize and close, empty parts of the top bar and the sidebar head send `window_drag`, and `Running::resize_direction` turns the outer 5 px into resize handles.

### Input routing

`hit_test` picks the pane under the pointer, everything else goes to the UI webview. A mouse press captures the target until release, which is what makes divider dragging work. Keyboard goes to the UI or the focused tab depending on the last click, Ctrl+L and friends move it. While a page dialog is open all input goes to the UI.

### Bridge

UI to core: `fetch("drift://ui/bridge?" + encodeURIComponent(JSON.stringify(cmd)))`. The sidebar may send any command. A tab may only send the commands in `Command::allowed_from_pages` (history page actions), and `Running::accepts` checks again that the tab really shows drift://ui/history.html. History never has a URL of its own, the core pushes it into the history page with `driftHistory.show()`, so web pages cannot read it. Core to UI: `drift.update(state)` evaluated in the UI webview, at most once per event loop turn.

### Updates

`updater.rs` checks https://api.github.com/repos/xRookieFight/drift/releases at most once a day (state in ~/.local/share/drift/update.json), on a worker thread. An install downloads the tarball (or the AppImage when `$APPIMAGE` is set), verifies it against SHA256SUMS from the same release, writes it next to the target and renames it over. Builds running from a cargo target dir never update themselves. Release asset names are a contract with `packaging/linux/package.sh`: `drift-<version>-x86_64-linux.tar.gz`, `Drift-<version>-x86_64.AppImage`, `SHA256SUMS`. Change both sides together.

### Known gaps

Colour pickers are not implemented, Servo gets a cancel. Permissions (camera, notifications, geolocation) are denied. Servo 0.8 has no download API, so there is no downloads UI.

## Commands

```sh
cargo build --profile local # optimized, no LTO, quick relinks: use this while working
cargo run --profile local   # run the browser (debug builds of Servo are too slow to use)
cargo build --release       # full LTO, what releases ship
cargo fmt --all             # format
cargo clippy --all-targets -- -D warnings
cargo test --profile local  # shares the local build instead of a 15 GB debug Servo
packaging/linux/package.sh <version>   # after cargo build --release, writes dist/
packaging/linux/install.sh [binary]    # per-user install to ~/.local, --uninstall to remove
```

Without the -devel packages: `scripts/cargo-in-container.sh build --profile local` builds in the GHCR image, the binary in target/local runs on a Fedora host that has the runtime libs (gstreamer, mesa).

`--screenshot PATH` (left out of --help on purpose) waits for the visible pages to load, saves the window as PNG and quits. Use it for docs/assets and smoke tests, it works on every compositor. Two env hooks only act in screenshot mode: `DRIFT_DEBUG_CLICKS="x,y;x,y"` clicks window pixels once the page settles, `DRIFT_DEBUG_UI_JS='...'` runs a snippet in the UI webview (for example to open the panel and type into it).

## Conventions

- Rust 2024 edition, rustfmt defaults with max_width 100, clippy clean with `-D warnings`.
- Keep `layout.rs`, `tabs.rs`, `session.rs` and `omnibox.rs` free of Servo and winit types so they stay unit testable.
- The UI must never block on page content. No synchronous waits on content webviews from the UI path.
- UI code: no framework, no build step. CSS custom properties for theme tokens.
- Comments are short and explain why, not what.
- Docs and this file are updated when behavior changes.

## Commits and releases

- Commit titles: one line, imperative sentence case, no conventional prefix, no trailing period, no body, no trailers.
- Releases: update CHANGELOG.md, bump Cargo.toml version, tag `vX.Y.Z`, push the tag. `release.yml` builds and publishes. See the `release` skill.

## CI

- `ci.yml`: fmt on the host, clippy and tests inside `ghcr.io/xrookiefight/drift-build-env`.
- `build-env.yml`: rebuilds the image when `docker/build-env.Dockerfile` changes, and monthly.
- `audit.yml`: cargo-deny for advisories, licenses and sources.
- `release.yml`: tarball, AppImage and SHA256SUMS on `v*` tags.

## Skills

Task guides live in `.claude/skills/` (Claude Code) and `.agents/skills/` (Codex). Both copies must stay identical: build-and-run, bump-servo, release, ui-change.
