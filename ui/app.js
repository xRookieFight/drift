// Drift sidebar. The core owns all state and pushes it here through drift.update(), we only
// render it and send commands back. See src/bridge.rs for both message shapes.
"use strict";

const SPLIT_GAP = 8; // layout.rs SPLIT_GAP

const ICONS = {
  back: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M10 3.5 5.5 8l4.5 4.5"/></svg>',
  forward: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3.5 10.5 8 6 12.5"/></svg>',
  reload: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M13 8a5 5 0 1 1-1.5-3.6"/><path d="M13 2.8v2.6h-2.6"/></svg>',
  split: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><rect x="2" y="3" width="12" height="10" rx="2"/><path d="M8 3v10"/></svg>',
  plus: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M8 3.5v9M3.5 8h9"/></svg>',
  close: '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="m4.5 4.5 7 7M11.5 4.5l-7 7"/></svg>',
};

const $ = (id) => document.getElementById(id);
const el = {
  back: $("back"),
  forward: $("forward"),
  reload: $("reload"),
  split: $("split"),
  form: $("address-form"),
  address: $("address"),
  tabs: $("tabs"),
  newTab: $("new-tab"),
  card: $("card"),
  ring: $("focus-ring"),
  divider: $("divider"),
  overlay: $("overlay"),
  dialog: $("dialog"),
};

el.back.innerHTML = ICONS.back;
el.forward.innerHTML = ICONS.forward;
el.reload.innerHTML = ICONS.reload;
el.split.innerHTML = ICONS.split;
el.newTab.querySelector(".glyph").innerHTML = ICONS.plus;

function send(cmd) {
  // fire and forget, the core answers by pushing new state
  fetch("drift://ui/bridge?" + encodeURIComponent(JSON.stringify(cmd))).catch(() => {});
}

let state = null;
let editingAddress = false;
let shownDialog = 0;

function place(node, r, pad = 0) {
  node.style.left = r.x - pad + "px";
  node.style.top = r.y - pad + "px";
  node.style.width = r.w + pad * 2 + "px";
  node.style.height = r.h + pad * 2 + "px";
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
      li.innerHTML = '<span class="favicon"></span><span class="title"></span><button class="close" title="Close tab (Ctrl+W)"></button>';
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
        fav.style.backgroundSize = "cover";
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

function prettyUrl(url) {
  if (!url || url.startsWith("drift://")) return "";
  try {
    const u = new URL(url);
    return u.host + (u.pathname === "/" ? "" : u.pathname);
  } catch (_) {
    return url;
  }
}

function render() {
  document.body.classList.toggle("split", !!state.split);
  document.body.classList.toggle("loading", state.loading);
  document.body.classList.toggle("immersive", state.immersive);

  el.back.disabled = !state.can_go_back;
  el.forward.disabled = !state.can_go_forward;
  el.split.classList.toggle("on", !!state.split);
  if (!editingAddress) el.address.value = state.url;

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
  renderDialog();
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

// ---- toolbar and address field ----------------------------------------------------------------

el.back.addEventListener("click", () => send({ cmd: "back" }));
el.forward.addEventListener("click", () => send({ cmd: "forward" }));
el.reload.addEventListener("click", () => send({ cmd: "reload" }));
el.split.addEventListener("click", () => send({ cmd: "toggle_split" }));
el.newTab.addEventListener("click", () => send({ cmd: "new_tab" }));

el.address.addEventListener("focus", () => {
  editingAddress = true;
  el.address.select();
});
el.address.addEventListener("blur", () => {
  editingAddress = false;
  if (state) el.address.value = state.url;
});
el.address.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    el.address.value = state ? state.url : "";
    el.address.blur();
    send({ cmd: "focus_content" });
  }
});
el.form.addEventListener("submit", (e) => {
  e.preventDefault();
  const input = el.address.value.trim();
  if (!input) return;
  editingAddress = false;
  el.address.blur();
  send({ cmd: "navigate", input });
});

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
  el.dialog.style.left = el.dialog.style.top = "";
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

  el.dialog.onkeydown = (e) => {
    if (e.key === "Enter") ok.click();
    if (e.key === "Escape") answer(d, false);
  };
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
  document.onkeydown = (e) => {
    if (e.key === "Escape" && shownDialog === d.id) answer(d, false);
  };
}

// ---- entry points for the core --------------------------------------------------------------------

window.drift = {
  update(next) {
    state = next;
    render();
  },
  focusAddress() {
    el.address.focus();
    el.address.select();
  },
};

send({ cmd: "ready" });
