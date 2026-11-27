---
name: bump-servo
description: Update the pinned servo crate to a newer release and fix the API breaks it causes. Use when a new Servo release is out or a needed engine fix landed upstream.
---

# Bump Servo

Servo's embedding API still changes between releases, so a bump is a deliberate change, never done by Dependabot.

1. Check releases: `gh api repos/servo/servo/releases --jq '.[0:3][] | .tag_name'` and the crates.io version of `servo`.
2. Read the release notes for embedding API changes. The reference embedder is `ports/servoshell` in the Servo repo at the same tag, compare how it uses `ServoBuilder`, `WebView`, `WebViewDelegate` and the rendering context with `src/app.rs`.
3. Update the version in Cargo.toml, then `cargo update -p servo`.
4. Sync native deps: diff `python/servo/platform/linux_packages/dnf/dnf_base.txt` at the new tag against `docker/build-env.Dockerfile` and the README Fedora list. Update both if it changed.
5. Fix compile errors, keep the changes local to the Servo facing modules (`app.rs`, `protocol.rs`, delegate code). `layout.rs` and `session.rs` must not need changes.
6. Verify: fmt, clippy, test, then run the app and load the smoke sites (wikipedia.org, github.com, duckduckgo.com, servo.org).
7. Re-check the advisory ignores in deny.toml, they are all Servo transitive. Drop the ones the bump fixed.
8. Update AGENTS.md if any architecture note changed, add a CHANGELOG entry, commit as `Update Servo to X.Y.Z`.
