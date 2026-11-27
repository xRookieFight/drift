# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-27

### Added

- Servo 0.8 based browser shell with one window and any number of tabs
- Vertical tab sidebar with drag to reorder, middle click to close and Ctrl+click to split
- Top bar with back, forward, reload, the address field and the split toggle
- Split view with two tabs side by side, a draggable divider and a focus ring
- Address field that accepts URLs, bare domains and searches (DuckDuckGo)
- Page dialogs: alert, confirm, prompt, select popups and the context menu
- Light and dark themes that follow the desktop, read from the XDG portal on Wayland
- Session restore and a persistent profile in ~/.local/share/drift
- Self updates from GitHub Releases, checked daily and verified against SHA256SUMS
- Per-user install script (packaging/linux/install.sh), shipped in the release tarball
- --theme and --no-update-check options
- Keyboard shortcuts: Ctrl+T, Ctrl+W, Ctrl+L, Ctrl+R, Ctrl+Tab, Ctrl+Shift+S, Alt+Left/Right, zoom, F11
- Project scaffold, documentation, CI, release pipeline and build environment image

[Unreleased]: https://github.com/xRookieFight/drift/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/xRookieFight/drift/releases/tag/v0.1.0
