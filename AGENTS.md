# AGENTS.md

Guidance for coding agents (Claude Code, Codex and others) working on Drift. Keep this file current: any change to architecture, commands, layout or conventions updates it in the same commit.

## What Drift is

A Linux first desktop browser on the Servo engine (crates.io `servo`, pinned in Cargo.toml). The browser UI is HTML/CSS/JS rendered by Servo in its own webview, the Rust core owns windows, tabs and layout. There is one engine only, no fallback.

## Current state

Scaffold only. `src/main.rs` is a placeholder, Servo is not a dependency yet. Next step is the spike from the roadmap in README.md.

## Planned layout

```
src/
  main.rs       entry, CLI args
  app.rs        winit event loop, owns Servo and the window, routes input
  tabs.rs       Tab model and TabManager
  layout.rs     rect math: sidebar, content card, split ratio (pure, unit tested)
  bridge.rs     JSON messages between UI and core
  protocol.rs   browser:// scheme, serves embedded ui/ assets and bridge calls
  session.rs    session save/restore (~/.local/share/drift/session.json)
  keys.rs       global shortcuts, handled before input reaches webviews
ui/             sidebar UI, plain HTML/CSS/JS, no framework, embedded at build time
packaging/      desktop file, icon, release scripts
docker/         build environment image (published to GHCR)
```

UI to core: `fetch("browser://bridge/<cmd>")`. Core to UI: `evaluate_javascript("drift.update(...)")` on the UI webview.

## Commands

```sh
cargo build                 # debug build
cargo run --release         # run the browser
cargo fmt --all             # format
cargo clippy --all-targets -- -D warnings
cargo test
packaging/linux/package.sh <version>   # after cargo build --release, writes dist/
```

Without local deps: `podman run --rm -it -v "$PWD:/work:Z" ghcr.io/xrookiefight/drift-build-env <cmd>`.

## Conventions

- Rust 2024 edition, rustfmt defaults with max_width 100, clippy clean with `-D warnings`.
- Keep `layout.rs` and `session.rs` free of Servo and winit types so they stay unit testable.
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
