---
name: ui-change
description: Change the browser chrome (sidebar, tabs, split view visuals, themes) in ui/ and the bridge that feeds it. Use for any visual or interaction change in the Drift interface.
---

# Change the Drift UI

- The UI lives in `ui/` as plain HTML, CSS and JS with no framework and no build step. It is embedded into the binary and served as `drift://ui/...` by `src/protocol.rs`.
- Theme values are CSS custom properties at the top of the main stylesheet. Add tokens there instead of hardcoding colors. Both `prefers-color-scheme` modes must look right.
- Servo renders this UI, so stick to CSS that Servo supports. When in doubt, test it in the app, not in Firefox or Chrome.
- Data flow: the core pushes state with `drift.update(state)` (see `src/bridge.rs` for the message shapes). The UI sends commands with `fetch("drift://ui/bridge?" + encodeURIComponent(JSON.stringify(cmd)))`. When adding a command, add it on both sides and keep the JSON shape documented in `bridge.rs`.
- Layout of the content webviews is computed in `src/layout.rs`: SIDEBAR_WIDTH, TOOLBAR_HEIGHT, CARD_MARGIN and SPLIT_GAP. The same numbers live in ui/style.css (--sidebar-width, --toolbar-height, --card-margin) and ui/app.js (SPLIT_GAP). Change all of them together or the pages will not line up with the drawn card.
- Never make the UI wait for page content.
- Verify by running the app in light and dark mode. Servo's CSS support is not Firefox's: no backdrop-filter, and animations can leave their last frame behind when a class is removed, so toggle visibility with display.
- Screenshots: `target/local/drift --profile <dir> --screenshot docs/assets/<name>.png <url>` captures the window once pages settle. Retake the README showcase images when the change is visible there and keep the file names.
