---
name: ui-change
description: Change the browser chrome (sidebar, tabs, split view visuals, themes) in ui/ and the bridge that feeds it. Use for any visual or interaction change in the Drift interface.
---

# Change the Drift UI

- The UI lives in `ui/` as plain HTML, CSS and JS with no framework and no build step. It is embedded into the binary and served as `browser://ui/...` by `src/protocol.rs`.
- Theme values are CSS custom properties at the top of the main stylesheet. Add tokens there instead of hardcoding colors. Both `prefers-color-scheme` modes must look right.
- Servo renders this UI, so stick to CSS that Servo supports. When in doubt, test it in the app, not in Firefox or Chrome.
- Data flow: the core pushes state with `drift.update(state)` (see `src/bridge.rs` for the message shapes). The UI sends commands with `fetch("browser://bridge/<cmd>")`. When adding a command, add it on both sides and keep the JSON shape documented in `bridge.rs`.
- Layout of the content webviews (card margin, split ratio) is computed in `src/layout.rs`. If the UI changes the sidebar width or card margin, change the constants there too so the content rect lines up with the drawn card.
- Never make the UI wait for page content.
- Verify by running the app in light and dark mode. When the change is visible in the README showcase, retake the screenshots with `grim` into `docs/assets/` and keep the same file names.
