---
name: build-and-run
description: Build, run and debug Drift locally or in the build env container. Use when setting up a machine, when a build fails on missing native libraries, or before running the browser to check a change.
---

# Build and run Drift

1. Native deps (Fedora): the package list is in `docker/build-env.Dockerfile`. Install the same packages with dnf. On other distros, look up Servo's list in `python/servo/platform/linux_packages/` of the Servo version pinned in Cargo.toml.
2. Build: `cargo build` (debug) or `cargo build --release`. The first build compiles Servo and needs about 20 GB and a long wait. Do not cancel it because it looks stuck on `mozjs_sys` or `servo-script`, those crates take the longest.
3. Run: `cargo run --release -- [url]`. Use `RUST_LOG=drift=debug` for shell logs and `RUST_LOG=debug` to include Servo.
4. No local deps? Run anything through the image:
   `podman run --rm -it -v "$PWD:/work:Z" ghcr.io/xrookiefight/drift-build-env cargo build`
   A GUI cannot run from the container without forwarding Wayland, so use it for build, clippy and tests.
5. Before calling a change done: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, then actually launch the app if the change touches anything visible.

Common failures:
- `libclang` not found: install `clang-libs`, or set `LIBCLANG_PATH`.
- Link errors about EGL/GL: `mesa-libEGL-devel` and `mesa-libGL-devel`.
- Out of disk: `cargo clean`, then build with `CARGO_PROFILE_DEV_DEBUG=0`.
