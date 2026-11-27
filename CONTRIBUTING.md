# Contributing to Drift

Thanks for wanting to help. This file covers how to get set up and what a good change looks like.

## Getting set up

Follow "Build from source" in the [README](README.md). The first Servo build is slow, later builds are incremental. Installing `sccache` and setting `RUSTC_WRAPPER=sccache` helps a lot if you switch branches often.

## Before you open a pull request

Run these locally, CI runs the same ones:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

If you touched the UI (anything under `ui/`), run the app and check both light and dark mode. Attach a screenshot to the pull request.

## What makes a good change

- One purpose per pull request. Unrelated cleanups go in their own.
- Match the existing structure and naming. [AGENTS.md](AGENTS.md) describes the architecture.
- Keep the UI responsive. Nothing in the shell should wait on page content.
- Update docs and AGENTS.md when behavior, commands or architecture change.
- Add tests for logic that can be tested without a window (layout math, session files, URL parsing).

## Commit messages

A single line in imperative sentence case, no prefix, no trailing period. For example: `Add divider drag to split view`.

## Site compatibility reports

Most broken sites are Servo bugs, not Drift bugs. Use the site compatibility issue template and include whether the site also breaks in servoshell. If it does, it belongs upstream at [servo/servo](https://github.com/servo/servo/issues).

## License

By contributing you agree that your work is licensed under the [MPL-2.0](LICENSE).
