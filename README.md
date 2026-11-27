<p align="center">
  <img src="docs/assets/logo.svg" width="96" alt="Drift logo">
</p>

<h1 align="center">Drift</h1>

<p align="center">
  A calm, Zen-inspired web browser built on <a href="https://servo.org">Servo</a>.
</p>

<p align="center">
  <a href="https://github.com/xRookieFight/drift/actions/workflows/ci.yml"><img src="https://github.com/xRookieFight/drift/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/xRookieFight/drift/releases"><img src="https://img.shields.io/github/v/release/xRookieFight/drift?include_prereleases&sort=semver" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MPL--2.0-blue" alt="License: MPL-2.0"></a>
</p>

---

Drift is a browser that tries to get out of your way. Tabs live in a quiet sidebar, pages sit on a soft rounded card, and two tabs can share the window in split view. The whole interface is plain HTML and CSS drawn by Servo itself, so theming it is just writing CSS.

> [!WARNING]
> Drift is in early development and nothing is usable yet. Servo is still maturing too, so some sites will render incorrectly or not at all. Please report those with the site compatibility template.

## Showcase

Screenshots from the real app land here with v0.1.0. No mockups.

## Features

Planned for v0.1.0:

- Vertical tabs in a sidebar, with the address bar and navigation built in
- Split view with two tabs side by side and a draggable divider
- Light and dark themes that follow the system
- Session restore
- Keyboard first: <kbd>Ctrl</kbd>+<kbd>T</kbd>, <kbd>Ctrl</kbd>+<kbd>W</kbd>, <kbd>Ctrl</kbd>+<kbd>L</kbd>, <kbd>Ctrl</kbd>+<kbd>Tab</kbd>, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd>

See the [roadmap](#roadmap) for what comes after.

## Install

Prebuilt Linux builds (tarball and AppImage) are published on the [releases page](https://github.com/xRookieFight/drift/releases) once v0.1.0 ships.

## Build from source

Drift targets Linux first. You need a recent stable Rust toolchain and the native libraries Servo depends on.

On Fedora:

```sh
sudo dnf install gcc-c++ clang clang-libs llvm cmake libtool gperf python3-devel \
  libX11-devel libXi-devel libXrandr-devel libXcursor-devel libXmu-devel \
  libxkbcommon-devel wayland-devel mesa-libGL-devel mesa-libEGL-devel vulkan-loader \
  freetype-devel fontconfig-devel harfbuzz-devel expat-devel glib2-devel dbus-devel \
  libunwind-devel ncurses-devel libjpeg-turbo-devel \
  gstreamer1-devel gstreamer1-plugins-base-devel gstreamer1-plugins-bad-free-devel
cargo run --release
```

The first build compiles all of Servo, so expect it to take a while and use around 20 GB of disk.

If you would rather not install anything, use the build environment image:

```sh
podman run --rm -it -v "$PWD:/work:Z" ghcr.io/xrookiefight/drift-build-env cargo build --release
```

## Roadmap

| Version | Focus |
|---------|-------|
| 0.1 | Sidebar tabs, split view, theming, session restore |
| 0.2 | Workspaces |
| 0.3 | Compact mode |
| 0.4 | Essentials and pinned sites |
| 0.5 | History and downloads UI |
| later | Flatpak |

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first, and please follow the [Code of Conduct](CODE_OF_CONDUCT.md). Security issues go through [SECURITY.md](SECURITY.md), not public issues.

## Acknowledgements

Drift stands on [Servo](https://github.com/servo/servo), and its look is inspired by [Zen Browser](https://zen-browser.app). Drift is an independent project and is not affiliated with either.

## License

[Mozilla Public License 2.0](LICENSE)
