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
  tabs.rs       Tab list, focus and split state (pure, unit tested)
  layout.rs     rect math: sidebar, content card, split panes (pure, unit tested)
  bridge.rs     JSON messages between UI and core (Command, UiState)
  protocol.rs   drift:// scheme: embedded ui/ assets and the bridge endpoint
  dialogs.rs    alert/confirm/prompt, <select> and context menu queue shown by the UI
  session.rs    session save/restore (~/.local/share/drift/session.json)
  omnibox.rs    address field input to URL or search
  favicon.rs    favicon pixels to PNG data URLs
  keys.rs       browser shortcuts, checked before input reaches webviews
  keyutils.rs   winit to Servo keyboard event mapping (adapted from servoshell)
ui/             sidebar UI, plain HTML/CSS/JS, no framework, embedded at build time
packaging/      desktop file, icon, release scripts
scripts/        cargo-in-container.sh
docker/         build environment image (published to GHCR)
```

### How a frame is drawn

Every webview (the UI and each visible tab) renders into its own OffscreenRenderingContext. `Running::paint` blits the UI full window first, then each pane at its layout rect. The UI draws the backdrop, the card shadow, the split divider and the focus ring around where the panes land, using the rects the core sends in UiState.

### Input routing

`hit_test` picks the pane under the pointer, everything else goes to the UI webview. A mouse press captures the target until release, which is what makes divider dragging work. Keyboard goes to the UI or the focused tab depending on the last click, Ctrl+L and friends move it. While a page dialog is open all input goes to the UI.

### Bridge

UI to core: `fetch("drift://ui/bridge?" + encodeURIComponent(JSON.stringify(cmd)))`. The handler only accepts requests whose target webview is the UI webview, so pages cannot drive the browser. Core to UI: `drift.update(state)` evaluated in the UI webview, at most once per event loop turn.

### Known gaps

File and colour pickers are not implemented yet, Servo gets a cancel. Permissions (camera, notifications, geolocation) are denied.

## Commands

```sh
cargo build --profile local # optimized, no LTO, quick relinks: use this while working
cargo run --profile local   # run the browser (debug builds of Servo are too slow to use)
cargo build --release       # full LTO, what releases ship
cargo fmt --all             # format
cargo clippy --all-targets -- -D warnings
cargo test --profile local  # shares the local build instead of a 15 GB debug Servo
packaging/linux/package.sh <version>   # after cargo build --release, writes dist/
```

Without the -devel packages: `scripts/cargo-in-container.sh build --profile local` builds in the GHCR image, the binary in target/local runs on a Fedora host that has the runtime libs (gstreamer, mesa).

`--screenshot PATH` (left out of --help on purpose) waits for the focused page to load, saves the window as PNG and quits. Use it for docs/assets and smoke tests, it works on every compositor.

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
