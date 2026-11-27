---
name: release
description: Cut a Drift release (changelog, version bump, tag, verify the published artifacts). Use when asked to release or tag a version.
---

# Release Drift

1. Make sure main is green in CI.
2. In CHANGELOG.md, rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD` and add a fresh empty Unreleased section above it. The release body is extracted from this section by `packaging/release-notes.sh`, check it with `packaging/release-notes.sh X.Y.Z`.
3. Set `version = "X.Y.Z"` in Cargo.toml and run `cargo build` so Cargo.lock updates.
4. Commit as `Release X.Y.Z`, push, then `git tag vX.Y.Z && git push origin vX.Y.Z`.
5. Watch `release.yml`: `gh run watch`. Versions below 1.0 are marked as prerelease automatically.
6. Verify the release page has the tarball, the AppImage and SHA256SUMS, download the AppImage and launch it once.
7. If a build failed after the tag was pushed, fix on main and rerun with workflow_dispatch and the same tag rather than moving the tag.
