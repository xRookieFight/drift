// Drift browser chrome. The core owns all state and pushes it here through drift.update(), we
// render it and send commands back. See src/bridge.rs for both message shapes.
"use strict";

const SPLIT_GAP = 8; // layout.rs SPLIT_GAP

const svg = (body, extra = "") =>
  `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" ${extra}>${body}</svg>`;

const ICONS = {
  back: svg('<path d="M10 3.5 5.5 8l4.5 4.5"/>'),
  forward: svg('<path d="M6 3.5 10.5 8 6 12.5"/>'),
  reload: svg('<path d="M13 8a5 5 0 1 1-1.5-3.6"/><path d="M13 2.8v2.6h-2.6"/>'),
  split: svg('<rect x="2" y="3" width="12" height="10" rx="2"/><path d="M8 3v10"/>', 'stroke-width="1.5"'),
  plus: svg('<path d="M8 3.5v9M3.5 8h9"/>'),
  close: svg('<path d="m4.5 4.5 7 7M11.5 4.5l-7 7"/>', 'stroke-width="1.7"'),
  minimize: svg('<path d="M4 8.5h8"/>'),
  maximize: svg('<rect x="4" y="4" width="8" height="8" rx="1.5"/>', 'stroke-width="1.5"'),
  search: svg('<circle cx="7" cy="7" r="4.25"/><path d="m10.2 10.2 3.3 3.3"/>'),
  globe: svg('<circle cx="8" cy="8" r="5.75"/><path d="M2.5 8h11M8 2.25c1.6 1.7 2.4 3.6 2.4 5.75S9.6 12.05 8 13.75C6.4 12.05 5.6 10.15 5.6 8S6.4 3.95 8 2.25Z"/>', 'stroke-width="1.3"'),
  clock: svg('<circle cx="8" cy="8" r="5.75"/><path d="M8 5v3.2l2 1.3"/>', 'stroke-width="1.4"'),
  lock: svg('<rect x="3.5" y="7" width="9" height="6.5" rx="1.5"/><path d="M5.5 7V5.2a2.5 2.5 0 0 1 5 0V7"/>', 'stroke-width="1.4"'),
};

const $ = (id) => document.getElementById(id);
const el = {
  back: $("back"),
  forward: $("forward"),
  reload: $("reload"),
  split: $("split"),
  urlbox: $("urlbox"),
  urlText: $("url-text"),
  tabs: $("tabs"),
  newTab: $("new-tab"),
  card: $("card"),
  ring: $("focus-ring"),
  divider: $("divider"),
  panel: $("panel"),
  panelInput: $("panel-input"),
  suggestions: $("suggestions"),
  overlay: $("overlay"),
  dialog: $("dialog"),
  update: $("update"),
};

el.back.innerHTML = ICONS.back;
el.forward.innerHTML = ICONS.forward;
el.reload.querySelector(".when-idle").innerHTML = ICONS.reload;
el.split.innerHTML = ICONS.split;
el.newTab.querySelector(".glyph").innerHTML = ICONS.plus;
$("win-min").innerHTML = ICONS.minimize;
$("win-max").innerHTML = ICONS.maximize;
$("win-close").innerHTML = ICONS.close;
el.panel.querySelector(".glyph").innerHTML = ICONS.search;
el.urlbox.querySelector(".lock").innerHTML = ICONS.lock;

function send(cmd) {
  // fire and forget, the core answers by pushing new state
  fetch("drift://ui/bridge?" + encodeURIComponent(JSON.stringify(cmd))).catch(() => {});
}

let state = null;
let shownDialog = 0;

function place(node, r, pad = 0) {
  node.style.left = r.x - pad + "px";
  node.style.top = r.y - pad + "px";
  node.style.width = r.w + pad * 2 + "px";
  node.style.height = r.h + pad * 2 + "px";
}

function prettyUrl(url) {
  if (!url || url.startsWith("drift://")) return "";
  try {
    const u = new URL(url);
    return u.host + (u.pathname === "/" ? "" : u.pathname);
  } catch (_) {
    return url;
  }
}

// ---- floating boxes ------------------------------------------------------------------------
//
// The panel, dialogs and popups live in this webview, which the core draws *under* the pages.
// We tell it where they are, it mixes those boxes over the pages and routes their input here.

let lastFloats = "";

function floatOf(node, radius, shadow, scrim = 0) {
  const r = node.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height, radius, shadow, scrim };
}

function reportFloats() {
  const floats = [];
  if (!el.panel.hidden) floats.push(floatOf(el.panel, 16, 36));
  if (!el.overlay.hidden) {
    const popup = el.overlay.classList.contains("popup");
    floats.push(popup ? floatOf(el.dialog, 10, 18) : floatOf(el.dialog, 14, 40, 0.35));
  }
  const json = JSON.stringify(floats);
  if (json !== lastFloats) {
    lastFloats = json;
    send({ cmd: "set_overlays", floats });
  }
}

// ---- rendering --------------------------------------------------------------------------------

function renderUrl() {
  const url = state.url;
  el.urlText.textContent = "";
  el.urlText.classList.toggle("empty", !url);
  el.urlbox.querySelector(".lock").style.display = url.startsWith("https://") ? "" : "none";
  if (!url) {
    el.urlText.textContent = "Search or enter address";
    return;
  }
  if (url.startsWith("drift://")) {
    // our own pages get a name, not a made up address
    const focused = state.tabs.find((t) => t.id === state.focused);
    el.urlText.textContent = (focused && focused.title) || "Drift";
    return;
  }
  try {
    const u = new URL(url);
    const host = document.createElement("span");
    host.className = "host";
    host.textContent = u.host.replace(/^www\./, "");
    const rest = document.createElement("span");
    rest.className = "rest";
    rest.textContent = (u.pathname === "/" ? "" : u.pathname) + u.search;
    el.urlText.append(host, rest);
  } catch (_) {
    el.urlText.textContent = url;
  }
}

function renderTabs() {
  const split = state.split;
  const inSplit = (id) => split && (split.left === id || split.right === id);
  const existing = new Map();
  for (const li of el.tabs.children) existing.set(Number(li.dataset.id), li);

  const order = [];
  for (const tab of state.tabs) {
    let li = existing.get(tab.id);
    if (!li) {
      li = document.createElement("li");
      li.className = "tab";
      li.dataset.id = tab.id;
      li.innerHTML =
        '<span class="icon-slot"><span class="favicon"></span><span class="spinner"></span></span>' +
        '<span class="title"></span><button class="close" title="Close tab (Ctrl+W)"></button>';
      li.querySelector(".close").innerHTML = ICONS.close;
      wireTab(li);
    }
    existing.delete(tab.id);
    li.classList.toggle("selected", tab.id === state.focused);
    li.classList.toggle("split-member", !!inSplit(tab.id));
    li.classList.toggle("loading", tab.loading);
    li.querySelector(".title").textContent = tab.title || prettyUrl(tab.url) || "New Tab";
    li.title = tab.title ? tab.title + "\n" + tab.url : tab.url;
    const fav = li.querySelector(".favicon");
    if (tab.favicon) {
      if (fav.dataset.src !== tab.favicon) {
        fav.dataset.src = tab.favicon;
        fav.style.backgroundImage = `url("${tab.favicon}")`;
        fav.classList.add("img");
      }
    } else if (fav.dataset.src) {
      delete fav.dataset.src;
      fav.style.backgroundImage = "";
      fav.classList.remove("img");
    }
    order.push(li);
  }
  for (const li of existing.values()) li.remove();
  order.forEach((li, i) => {
    if (el.tabs.children[i] !== li) el.tabs.insertBefore(li, el.tabs.children[i] || null);
  });
}

function render() {
  document.body.classList.toggle("split", !!state.split);
  document.body.classList.toggle("loading", state.loading);
  document.body.classList.toggle("immersive", state.immersive);

  el.back.disabled = !state.can_go_back;
  el.forward.disabled = !state.can_go_forward;
  el.split.classList.toggle("on", !!state.split);
  renderUrl();

  place(el.card, state.card);
  if (state.split && state.panes.length === 2) {
    const [left, right] = state.panes;
    const focused = state.split.focused === "left" ? left : right;
    place(el.ring, focused, 3);
    place(el.divider, {
      x: left.x + left.w,
      y: left.y,
      w: right.x - (left.x + left.w),
      h: left.h,
    });
  }

  renderTabs();
  renderUpdate();
  renderDialog();
  if (!el.panel.hidden) placePanel();
  reportFloats();
}

// ---- tabs: click, middle click, ctrl+click to split, drag to reorder ----------------------

function wireTab(li) {
  li.addEventListener("mousedown", (e) => {
    const id = Number(li.dataset.id);
    if (e.button === 1) {
      e.preventDefault();
      send({ cmd: "close", id });
      return;
    }
    if (e.button !== 0 || e.target.closest(".close")) return;
    if (e.ctrlKey) {
      send({ cmd: "split_with", id });
      return;
    }
    startTabDrag(li, e);
  });
  li.querySelector(".close").addEventListener("click", (e) => {
    e.stopPropagation();
    send({ cmd: "close", id: Number(li.dataset.id) });
  });
}

function startTabDrag(li, down) {
  const id = Number(li.dataset.id);
  let dragging = false;
  const move = (e) => {
    if (!dragging && Math.abs(e.clientY - down.clientY) > 5) {
      dragging = true;
      li.classList.add("dragging");
    }
  };
  const up = (e) => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    li.classList.remove("dragging");
    if (!dragging) {
      send({ cmd: "activate", id });
      return;
    }
    const items = [...el.tabs.children];
    let to = items.length - 1;
    for (let i = 0; i < items.length; i++) {
      const r = items[i].getBoundingClientRect();
      if (e.clientY < r.top + r.height / 2) {
        to = i;
        break;
      }
    }
    const from = items.indexOf(li);
    if (to > from) to -= 1;
    if (to !== from) send({ cmd: "move_tab", id, to: Math.max(0, to) });
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

// ---- split divider ---------------------------------------------------------------------------

el.divider.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || !state || !state.split) return;
  e.preventDefault();
  el.divider.classList.add("active");
  const card = state.card;
  let pending = null;
  const move = (ev) => {
    const usable = Math.max(1, card.w - SPLIT_GAP);
    const ratio = (ev.clientX - card.x - SPLIT_GAP / 2) / usable;
    // one resize per frame is plenty, every mousemove would re-layout the pages for nothing
    if (pending === null) {
      requestAnimationFrame(() => {
        send({ cmd: "set_ratio", ratio: pending });
        pending = null;
      });
    }
    pending = ratio;
  };
  const up = () => {
    el.divider.classList.remove("active");
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
});

// ---- toolbar and window controls -------------------------------------------------------------

el.back.addEventListener("click", () => send({ cmd: "back" }));
el.forward.addEventListener("click", () => send({ cmd: "forward" }));
el.reload.addEventListener("click", () => send({ cmd: "reload" }));
el.split.addEventListener("click", () => send({ cmd: "toggle_split" }));
el.newTab.addEventListener("click", () => openPanel(true));
el.urlbox.addEventListener("mousedown", (e) => {
  if (e.button === 0) openPanel(false);
});
$("win-min").addEventListener("click", () => send({ cmd: "window_minimize" }));
$("win-max").addEventListener("click", () => send({ cmd: "window_toggle_maximize" }));
$("win-close").addEventListener("click", () => send({ cmd: "window_close" }));

// empty parts of the top bar and the sidebar head move the window, like a title bar
for (const area of document.querySelectorAll(".drag")) {
  area.addEventListener("mousedown", (e) => {
    if (e.button === 0 && e.target === area) send({ cmd: "window_drag" });
  });
  area.addEventListener("dblclick", (e) => {
    if (e.target === area) send({ cmd: "window_toggle_maximize" });
  });
}

// ---- the floating address panel -----------------------------------------------------------------
//
// Opens from the address box, Ctrl+L and Ctrl+T. In new tab mode nothing exists until Enter,
// so a Ctrl+T that is changed mind about leaves no empty tab behind.

const panel = {
  newTab: false,
  seq: 0,
  typed: "",
  items: [],
  active: 0,
  completing: false,
};

function placePanel() {
  const card = state ? state.card : { x: 0, y: 0, w: window.innerWidth, h: window.innerHeight };
  const w = Math.min(680, card.w - 48);
  el.panel.style.width = w + "px";
  el.panel.style.left = card.x + (card.w - w) / 2 + "px";
  el.panel.style.top = card.y + Math.max(24, card.h * 0.12) + "px";
}

function openPanel(newTab) {
  panel.newTab = newTab;
  panel.completing = false;
  el.panel.hidden = false;
  placePanel();
  el.panelInput.value = newTab || !state ? "" : state.url;
  panel.typed = el.panelInput.value;
  el.panelInput.focus();
  // select() on an empty field paints the placeholder as selected in Servo
  if (el.panelInput.value) el.panelInput.select();
  requestSuggestions();
  reportFloats();
}

function closePanel(giveFocusBack) {
  if (el.panel.hidden) return;
  el.panel.hidden = true;
  el.suggestions.innerHTML = "";
  el.panelInput.blur();
  reportFloats();
  if (giveFocusBack) send({ cmd: "focus_content" });
}

function requestSuggestions() {
  panel.seq += 1;
  send({ cmd: "suggest", query: panel.typed, seq: panel.seq });
}

function kindIcon(kind) {
  return kind === "search" ? ICONS.search : kind === "history" ? ICONS.clock : ICONS.globe;
}

function renderSuggestions() {
  el.suggestions.innerHTML = "";
  panel.items.forEach((item, i) => {
    const li = document.createElement("li");
    li.className = "suggestion" + (i === panel.active ? " active" : "");
    const kind = document.createElement("span");
    kind.className = "kind";
    kind.innerHTML = kindIcon(item.kind);
    const label = document.createElement("span");
    label.className = "label";
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = item.title || prettyUrl(item.url) || item.url;
    label.append(title);
    if (item.kind !== "search" && item.title) {
      const url = document.createElement("span");
      url.className = "url";
      url.textContent = prettyUrl(item.url);
      label.append(url);
    }
    const hint = document.createElement("span");
    hint.className = "hint";
    hint.textContent = panel.newTab ? "Open in new tab ↵" : "Go ↵";
    li.append(kind, label, hint);
    li.addEventListener("mousemove", () => {
      if (panel.active !== i) {
        panel.active = i;
        renderSuggestions();
      }
    });
    li.addEventListener("mousedown", (e) => {
      e.preventDefault();
      go(item.url);
    });
    el.suggestions.append(li);
  });
  reportFloats();
}

function go(input) {
  if (!input.trim()) return;
  send({ cmd: panel.newTab ? "open_in_new_tab" : "navigate", input });
  closePanel(false);
}

el.panelInput.addEventListener("input", (e) => {
  panel.typed = el.panelInput.value;
  // only complete while typing forwards, completing on backspace would fight the user
  panel.completing = (e.inputType || "").startsWith("insert");
  panel.active = 0;
  requestSuggestions();
});

el.panelInput.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    closePanel(true);
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if (!panel.items.length) return;
    const step = e.key === "ArrowDown" ? 1 : -1;
    panel.active = (panel.active + step + panel.items.length) % panel.items.length;
    renderSuggestions();
  } else if (e.key === "Tab") {
    e.preventDefault();
    const end = el.panelInput.value.length;
    el.panelInput.setSelectionRange(end, end);
    panel.typed = el.panelInput.value;
    requestSuggestions();
  } else if (e.key === "Enter") {
    e.preventDefault();
    const pick = panel.items[panel.active];
    // the first row mirrors what was typed (or its completion), the rest are real picks
    go(panel.active > 0 && pick ? pick.url : el.panelInput.value);
  }
});

// focus moving to a page (a click outside) closes the panel
el.panelInput.addEventListener("blur", () => {
  setTimeout(() => {
    if (document.activeElement !== el.panelInput) closePanel(false);
  }, 0);
});

function applySuggestions(resp) {
  if (resp.seq !== panel.seq || el.panel.hidden) return;
  panel.items = resp.items || [];
  const typed = panel.typed;
  if (panel.completing && resp.complete && resp.complete.toLowerCase().startsWith(typed.toLowerCase())) {
    // inline completion: the rest of the site name, selected so typing just replaces it
    el.panelInput.value = typed + resp.complete.slice(typed.length);
    el.panelInput.setSelectionRange(typed.length, el.panelInput.value.length);
    // the top row says what Enter does, and Enter now goes to the completed site
    const site = "https://" + resp.complete;
    panel.items = [{ url: site, title: "", kind: "url" }].concat(
      panel.items.filter((i) => i.kind !== "search" && i.url !== site),
    );
  }
  panel.active = 0;
  renderSuggestions();
}

// ---- update notice ----------------------------------------------------------------------------

function renderUpdate() {
  const u = state.update;
  el.update.hidden = !u;
  if (!u) return;
  const text = el.update.querySelector(".update-text");
  const go = el.update.querySelector(".update-go");
  const later = el.update.querySelector(".update-later");
  const lines = {
    available: [`Drift ${u.version} is available`, "Downloads in the background"],
    downloading: [`Downloading Drift ${u.version}`, "You can keep browsing"],
    ready: [`Drift ${u.version} is installed`, "Restart to start using it"],
    failed: ["Update failed", u.error || "Try again later"],
  }[u.status] || ["", ""];
  text.textContent = lines[0];
  const small = document.createElement("small");
  small.textContent = lines[1];
  text.append(small);
  go.textContent = { available: "Update", downloading: "Updating", ready: "Restart", failed: "Retry" }[u.status];
  go.disabled = u.status === "downloading";
  later.hidden = u.status === "downloading";
}

el.update.querySelector(".update-go").addEventListener("click", () => {
  const u = state && state.update;
  if (!u) return;
  send({ cmd: u.status === "ready" ? "restart" : "install_update" });
});
el.update.querySelector(".update-later").addEventListener("click", () => send({ cmd: "dismiss_update" }));

// ---- dialogs and popups ------------------------------------------------------------------------

function renderDialog() {
  const d = state.dialog;
  if (!d) {
    if (shownDialog) {
      el.overlay.hidden = true;
      el.dialog.innerHTML = "";
      shownDialog = 0;
    }
    return;
  }
  if (d.id === shownDialog) return;
  shownDialog = d.id;
  el.dialog.innerHTML = "";
  el.dialog.style.left = el.dialog.style.top = el.dialog.style.minWidth = "";
  const popup = d.kind === "select" || d.kind === "menu";
  el.overlay.classList.toggle("popup", popup);
  el.overlay.hidden = false;
  if (popup) buildPopup(d);
  else buildModal(d);
}

function answer(d, ok, value) {
  send({ cmd: "dialog_result", id: d.id, ok, value: value === undefined ? null : value });
}

function buildModal(d) {
  const origin = document.createElement("div");
  origin.className = "origin";
  origin.textContent = `${d.origin} says`;
  const message = document.createElement("div");
  message.className = "message";
  message.textContent = d.message;
  el.dialog.append(origin, message);

  let input = null;
  if (d.kind === "prompt") {
    input = document.createElement("input");
    input.type = "text";
    input.value = d.value;
    el.dialog.append(input);
  }

  const buttons = document.createElement("div");
  buttons.className = "buttons";
  const ok = document.createElement("button");
  ok.className = "primary";
  ok.textContent = "OK";
  ok.onclick = () => answer(d, true, input ? input.value : undefined);
  if (d.kind !== "alert") {
    const cancel = document.createElement("button");
    cancel.textContent = "Cancel";
    cancel.onclick = () => answer(d, false);
    buttons.append(cancel);
  }
  buttons.append(ok);
  el.dialog.append(buttons);

  // centre over the page card, a page dialog belongs to the page
  const card = state.card;
  const r = el.dialog.getBoundingClientRect();
  el.dialog.style.left = card.x + (card.w - r.width) / 2 + "px";
  el.dialog.style.top = card.y + Math.max(40, card.h * 0.2) + "px";

  el.dialog.onkeydown = (e) => {
    if (e.key === "Enter") ok.click();
    if (e.key === "Escape") answer(d, false);
  };
  el.overlay.onmousedown = null;
  (input || ok).focus();
}

function buildPopup(d) {
  let group = null;
  const picked = new Set(d.options.filter((o) => o.selected).map((o) => o.index));
  for (const o of d.options) {
    if (d.kind === "menu" && !o.label) {
      const sep = document.createElement("div");
      sep.className = "separator";
      el.dialog.append(sep);
      continue;
    }
    if (o.group && o.group !== group) {
      group = o.group;
      const g = document.createElement("div");
      g.className = "group";
      g.textContent = group;
      el.dialog.append(g);
    }
    const row = document.createElement("div");
    row.className = "option" + (o.disabled ? " disabled" : "") + (o.selected ? " selected" : "");
    row.textContent = o.label;
    if (!o.disabled) {
      row.onmouseup = () => {
        if (d.kind === "menu") {
          answer(d, true, o.index);
        } else if (d.multiple) {
          picked.has(o.index) ? picked.delete(o.index) : picked.add(o.index);
          row.classList.toggle("selected");
        } else {
          answer(d, true, [o.index]);
        }
      };
    }
    el.dialog.append(row);
  }

  if (d.anchor) {
    const a = d.anchor;
    const below = d.kind === "select" ? a.y + a.h + 2 : a.y;
    el.dialog.style.left = Math.min(a.x, window.innerWidth - 180) + "px";
    el.dialog.style.top = Math.min(below, window.innerHeight - 80) + "px";
    if (d.kind === "select") el.dialog.style.minWidth = Math.max(160, a.w) + "px";
  }

  // clicking outside closes the popup (and submits for multi selects)
  el.overlay.onmousedown = (e) => {
    if (e.target === el.overlay) {
      if (d.kind === "select" && d.multiple) answer(d, true, [...picked]);
      else answer(d, false);
    }
  };
}

document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  const d = state && state.dialog;
  if (d && d.id === shownDialog) answer(d, false);
});

// ---- entry points for the core --------------------------------------------------------------------

window.drift = {
  update(next) {
    state = next;
    render();
  },
  openPanel(newTab) {
    openPanel(!!newTab);
  },
  suggestions(resp) {
    applySuggestions(resp);
  },
  // the core calls this when a click lands on a page while something floats
  dismissFloating() {
    closePanel(false);
  },
};

send({ cmd: "ready" });
