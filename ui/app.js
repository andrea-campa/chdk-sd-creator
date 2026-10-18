const { invoke, Channel } = window.__TAURI__.core;

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => [...document.querySelectorAll(sel)];
const el = (tag, props = {}, ...kids) => {
  const n = Object.assign(document.createElement(tag), props);
  n.append(...kids.filter((k) => k != null));
  return n;
};

const ORDER = ["camera", "method", "card", "firmware", "install"];
const state = {
  step: "camera",
  cameras: [],
  camera: null,
  cards: [],
  card: null,
  channel: "stable",
  builds: [],
  firmware: null,
  method: "fi2",
  full: true,
  busy: false,
  installed: false,
};

/* ---------- helpers ---------- */

function fmtBytes(n) {
  if (!n) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1000)), u.length - 1);
  return `${(n / 1000 ** i).toFixed(i >= 3 ? 1 : 0)} ${u[i]}`;
}
const fwLabel = (fw) => `${fw[0]}.${fw.slice(1, 3)}${fw[3].toUpperCase()}`; // 100c -> 1.00C

let toastTimer;
function toast(msg, error = false) {
  const t = $("#toast");
  t.textContent = msg;
  t.className = "toast" + (error ? " error" : "");
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), error ? 7000 : 3500);
}

function confirmDialog(title, text, okLabel) {
  const d = $("#confirm");
  $("#confirm-title").textContent = title;
  $("#confirm-text").textContent = text;
  $("#confirm-ok").textContent = okLabel;
  d.returnValue = "";
  d.showModal();
  return new Promise((res) => d.addEventListener("close", () => res(d.returnValue === "ok"), { once: true }));
}

/* The filesystem the card needs for the chosen start method. */
const needsFat16 = () => state.method === "autoboot" && !state.camera?.fat32_autoboot;
const cardReady = (card) => !!card && !card.locked && !(needsFat16() && card.fat === "FAT32");

function reachable(step) {
  switch (step) {
    case "camera": return true;
    case "method": return !!state.camera;
    case "card": return !!state.camera;
    case "firmware": return !!state.camera && cardReady(state.card);
    case "install": return !!state.camera && !!state.card && !!state.firmware;
  }
}

/* ---------- navigation ---------- */

function go(step) {
  if (state.busy || !reachable(step)) return;
  state.step = step;
  $$("[data-panel]").forEach((p) => (p.hidden = p.dataset.panel !== step));
  if (step === "method") renderMethod();
  if (step === "card") loadCards();
  if (step === "firmware") loadBuilds();
  if (step === "install") renderInstall();
  renderRail();
}

function renderRail() {
  const idx = ORDER.indexOf(state.step);
  $$("#steps li").forEach((li) => {
    const s = li.dataset.step;
    li.classList.toggle("active", s === state.step);
    li.classList.toggle("complete", ORDER.indexOf(s) < idx);
    li.classList.toggle("reachable", reachable(s) && s !== state.step);
  });
  const cam = state.camera;
  $("#sum-camera").textContent = cam ? cam.name : "Not chosen";
  $("#sum-card").textContent = state.card ? `${state.card.name} · ${state.card.fat}` : "Not chosen";
  $("#sum-firmware").textContent = state.firmware ? `${fwLabel(state.firmware)} · ${state.channel}` : "Not chosen";
  $("#sum-install").textContent = state.installed ? "Installed" : " ";
  $("#camera-next").disabled = !state.camera;
  $("#sum-method").textContent = state.method === "autoboot" ? "Autoboot" : "Firmware Update";
  $("#card-next").disabled = !cardReady(state.card);
  $("#fw-next").disabled = !state.firmware;
}

/* ---------- 1. camera ---------- */

const norm = (s) => s.toLowerCase().replace(/[\s\-_.\/]/g, "");
let cameraIndex = []; // [{cam, hay, names}]
let series = "All";
let visibleCameras = [];

function buildCameraIndex() {
  cameraIndex = state.cameras.map((cam) => {
    const names = [cam.name, ...cam.aliases].map(norm);
    return { cam, names, hay: [...names, norm(cam.id), norm(cam.series || "")].join("|") };
  });
  const counts = {};
  for (const c of state.cameras) counts[c.series || "Other"] = (counts[c.series || "Other"] || 0) + 1;
  const order = ["IXUS / ELPH", "PowerShot A", "PowerShot SX", "PowerShot G", "PowerShot S", "PowerShot D", "EOS", "Other"];
  const chips = ["All", ...order.filter((s) => counts[s])];
  $("#series-chips").replaceChildren(...chips.map((s) => {
    const b = el("button", { className: "chip", role: "radio" }, s);
    b.dataset.series = s;
    b.onclick = () => { series = s; renderCameras(); };
    return b;
  }));
  $("#camera-search").placeholder = `Search ${state.cameras.length} models, e.g. SX130, SD790, ELPH 160, G12`;
}

function filterCameras() {
  const raw = $("#camera-search").value.trim();
  const tokens = raw.split(/\s+/).map(norm).filter(Boolean);
  const whole = norm(raw);
  const hits = cameraIndex.filter((e) =>
    (series === "All" || (e.cam.series || "Other") === series) &&
    tokens.every((t) => e.hay.includes(t)));
  // Best first: a name starting with the query, then one containing it, then the rest.
  const rank = (e) => !whole ? 0
    : e.names.some((n) => n.startsWith(whole) || n.replace(/^(powershot|ixus|digitalelph|eos)/, "").startsWith(whole)) ? 0
    : e.names.some((n) => n.includes(whole)) ? 1 : 2;
  return hits
    .map((e) => [rank(e), e.cam])
    .sort((a, b) => a[0] - b[0] || a[1].name.localeCompare(b[1].name, undefined, { numeric: true }))
    .map(([, cam]) => cam);
}

function selectCamera(cam) {
  if (state.camera?.id !== cam.id) {
    state.camera = cam;
    state.firmware = null;
    state.builds = [];
    try { localStorage.setItem("camera", cam.id); } catch {}
  }
  renderCameras();
  renderRail();
}

function renderCameras() {
  $$("#series-chips .chip").forEach((b) => {
    b.classList.toggle("on", b.dataset.series === series);
    b.setAttribute("aria-checked", String(b.dataset.series === series));
  });
  visibleCameras = filterCameras();
  const list = $("#camera-list");
  list.replaceChildren();
  for (const cam of visibleCameras) {
    const selected = state.camera?.id === cam.id;
    const tags = [cam.processor, cam.os].filter(Boolean).map((t) => el("span", { className: "tag" }, t));
    if (cam.fi2) tags.push(el("span", { className: "tag ok", title: "Can load CHDK from Menu → Firmware Update" }, "Firmware Update"));
    if (!cam.stable) tags.push(el("span", { className: "tag warn" }, "Dev builds only"));
    const row = el("button", { className: "cam-row" + (selected ? " selected" : ""), role: "option" },
      el("div", { className: "cam-main" },
        el("div", { className: "title" }, `Canon ${cam.name}`),
        cam.aliases.length ? el("div", { className: "sub" }, cam.aliases.join(" · ")) : null),
      el("div", { className: "cam-meta" },
        el("div", { className: "tags" }, ...tags),
        el("div", { className: "sub" }, `firmware ${cam.firmwares.map(fwLabel).join(", ")}`)));
    row.setAttribute("aria-selected", String(selected));
    row.onclick = () => selectCamera(cam);
    row.ondblclick = () => { selectCamera(cam); go("method"); };
    list.append(row);
  }
  const n = visibleCameras.length;
  $("#camera-count").textContent = n === state.cameras.length ? `${n} supported models` : `${n} of ${state.cameras.length} models`;
  if (!n) list.append(el("div", { className: "empty" }, "No camera matches. Try the model number alone, like “130” or “SD790”."));
}

function scrollSelectedCamera() {
  $("#camera-list .cam-row.selected")?.scrollIntoView({ block: "nearest" });
}

/* Arrow keys move through the results, Enter continues. */
function cameraKeys(e) {
  if (!["ArrowDown", "ArrowUp", "Enter"].includes(e.key)) return;
  if (e.key === "Enter") {
    if (visibleCameras.length === 1) selectCamera(visibleCameras[0]);
    if (state.camera) go("method");
    return;
  }
  e.preventDefault();
  if (!visibleCameras.length) return;
  const i = visibleCameras.findIndex((c) => c.id === state.camera?.id);
  const next = e.key === "ArrowDown" ? Math.min(i + 1, visibleCameras.length - 1) : Math.max(i - 1, 0);
  selectCamera(visibleCameras[i < 0 ? 0 : next]);
  scrollSelectedCamera();
}

/* ---------- 2. card ---------- */

const cardSvg = () => {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 34 42");
  s.classList.add("card-icon");
  s.innerHTML = `<path d="M2 1h22l9 9v30a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1z" fill="none" stroke="currentColor" stroke-width="1.6"/>
    <path d="M7 4v7M12 4v7M17 4v7M22 4v7" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>`;
  return s;
};

let cardsLoading = false;
async function loadCards({ quiet = false } = {}) {
  if (cardsLoading) return;
  cardsLoading = true;
  if (!quiet) $("#card-status").textContent = "Looking for cards…";
  let cards;
  try {
    cards = await invoke("cards");
    $("#card-status").textContent = "";
  } catch (e) {
    cards = [];
    $("#card-status").textContent = String(e);
  } finally {
    cardsLoading = false;
  }
  // Background polls only re-render when something actually changed.
  if (quiet && JSON.stringify(cards) === JSON.stringify(state.cards)) return;
  state.cards = cards;
  if (state.card) state.card = state.cards.find((c) => c.device === state.card.device) ?? null;
  if (!state.card && state.cards.length === 1) state.card = state.cards[0];
  renderCards();
  renderRail();
}

// Pick up cards being inserted, removed or unlocked while the SD card step is open.
setInterval(() => {
  if (state.step === "card" && !state.busy && !document.querySelector("dialog[open]")) {
    loadCards({ quiet: true });
  }
}, 3000);

function renderCards() {
  const list = $("#card-list");
  list.replaceChildren();
  if (!state.cards.length) {
    list.append(el("div", { className: "empty" },
      "No SD card found. Insert one (FAT or FAT32 formatted). The list updates by itself."));
  }
  for (const card of state.cards) {
    const used = card.size ? Math.round(((card.size - card.free) / card.size) * 100) : 0;
    const tags = [
      el("span", { className: "tag" }, card.fat),
      el("span", { className: "tag" }, card.protocol || "Removable"),
      el("span", { className: "tag" }, `/dev/${card.device}`),
    ];
    if (card.locked) tags.push(el("span", { className: "tag warn" }, "Locked"));
    if (card.has_chdk) tags.push(el("span", { className: "tag ok" }, "CHDK already on card"));
    const data = dataPartition(card);
    for (const other of card.other_partitions) {
      const role = other === data ? "photos & CHDK" : "not used by the camera";
      tags.push(el("span", { className: "tag" }, `+ “${other.name}” partition (${role})`));
    }
    if (!card.locked && !cardReady(card)) tags.push(el("span", { className: "tag warn" }, "Needs FAT16 for autoboot"));
    const max = state.camera?.max_card_gb;
    if (max && card.disk_size > max * 1.08e9) {
      tags.push(el("span", { className: "tag warn" }, `Larger than the ${max} GB this camera supports`));
    }
    const meter = el("div", { className: "meter" }, el("div", { style: `width:${used}%` }));
    const row = el("button", { className: "item card-row" + (state.card?.device === card.device ? " selected" : "") },
      cardSvg(),
      el("div", { className: "grow" },
        el("div", { className: "title" }, card.name || "Untitled"),
        el("div", { className: "sub" }, `${fmtBytes(card.free)} free of ${fmtBytes(card.size)}`),
        meter,
        el("div", { className: "tags" }, ...tags)));
    row.onclick = () => { state.card = card; renderCards(); renderRail(); };
    row.ondblclick = () => go("firmware");
    list.append(row);
  }
  renderFormatBox();
}

/* On multi-partition cameras CHDK switches to the first FAT32 partition after booting. */
function dataPartition(card) {
  if (!state.camera?.multipart || !card || card.fat === "FAT32") return null;
  return card.other_partitions.find((p) => p.fat === "FAT32") ?? null;
}

function renderFormatBox() {
  const card = state.card, cam = state.camera;
  const box = $("#format-box"), warn = $("#card-warn");
  const ready = cardReady(card);
  warn.hidden = !card || ready;
  if (card && card.locked) {
    warn.textContent = `“${card.name}” is locked, so nothing can be written to it. Slide the lock switch on the side of the card up (towards the contacts), reinsert it and press Refresh.`;
  } else if (card && !ready) {
    warn.textContent = card.formattable
      ? `The ${cam.name} can only autoboot from FAT16, and “${card.name}” is FAT32. Format it below, or go back and choose the Firmware Update method, which works with this card as it is.`
      : `The ${cam.name} can only autoboot from FAT16, and “${card.name}” is FAT32. This app won't format this disk; use another card or the Firmware Update method.`;
  }
  box.hidden = !card || !card.formattable || card.locked;
  if (!card || !cam) return;
  if (!ready && !card.locked) box.open = true;
  if (needsFat16()) {
    $("#format-summary").textContent = ready ? "Reformat as FAT16 (optional)…" : "Format as FAT16 for autoboot…";
    $("#format-desc").textContent = card.disk_size > 2.15e9
      ? (cam.multipart
          ? `The card gets a 2 GB FAT16 partition the camera boots from, and a FAT32 partition with the rest of the space, where CHDK and your photos live.`
          : `The card gets a 2 GB FAT16 partition, which the camera uses for CHDK and your photos. The rest of the card becomes a second partition that only your computer sees.`)
      : "The whole card becomes one FAT16 partition the camera can autoboot from.";
  } else {
    $("#format-summary").textContent = "Format as a clean FAT32 card (optional)…";
    $("#format-desc").textContent = "Not required: CHDK files are simply added to the card. Use this only if you want to start from an empty card.";
  }
}

function renderMethod() {
  const cam = state.camera;
  $("#method-fi2").hidden = !cam.fi2;
  if (!cam.fi2) state.method = "autoboot";
  $(`input[name=method][value=${state.method}]`).checked = true;
  $("#method-fs-hint").textContent = needsFat16()
    ? `Autoboot on the ${cam.name} needs a FAT16 card (2 GB partition at most). A FAT32 card has to be erased and reformatted in the next step.`
    : "Works with the card as it is: CHDK files are added without erasing anything.";
}

async function formatCard() {
  const card = state.card;
  if (!card) return;
  const layout = needsFat16() ? "fat16" : "fat32";
  const ok = await confirmDialog(
    `Erase “${card.name}”?`,
    `Everything on /dev/${card.whole_disk} (${fmtBytes(card.disk_size)}) will be deleted, including the ${fmtBytes(card.size - card.free)} of files on “${card.name}” now, and the card formatted as ${layout.toUpperCase()}. This cannot be undone.`,
    "Erase card");
  if (!ok) return;
  setBusy(true);
  $("#card-status").textContent = "Formatting…";
  try {
    await invoke("format_card", { device: card.device, layout });
    toast("Card formatted.");
    state.card = null;
    await new Promise((r) => setTimeout(r, 800));
  } catch (e) {
    toast(String(e), true);
  } finally {
    setBusy(false);
    $("#format-box").open = false;
    await loadCards();
    // The formatted card usually comes back with the same name.
    state.card = state.cards.find((c) => c.whole_disk === card.whole_disk) ?? null;
    renderCards();
    renderRail();
  }
}

/* ---------- 3. firmware ---------- */

let buildsReq = 0;
async function loadBuilds() {
  const cam = state.camera;
  const steps = cam.fw_check?.length ? cam.fw_check : [
    "The version in the camera’s normal menu or in photo info (e.g. “1.0.0.0”) leaves out the letter CHDK needs. Use these steps to see the full version.",
    `Press the button below to put an empty ${cam.ver_req} file on the card, then eject it.`,
    "Put the card in the camera and switch it on in Playback mode (▶).",
    "Hold FUNC./SET, then press DISP. (On some models: hold MENU, then press DISP.)",
    "A line like “Firmware Ver GM1.00C” appears. 1.00C means firmware 100c.",
  ];
  $("#fw-steps").replaceChildren(...steps.map((t) => el("li", {}, t)));
  if (!cam.stable && state.channel === "stable") state.channel = "trunk";
  $("#verreq-name").textContent = cam.ver_req;
  $$("#channel button").forEach((b) => b.classList.toggle("on", b.dataset.channel === state.channel));
  renderFirmwares(null);
  $("#fw-status").textContent = "Checking which builds are available…";
  const req = ++buildsReq;
  try {
    const builds = await invoke("builds", { cameraId: cam.id, channel: state.channel });
    if (req !== buildsReq) return;
    state.builds = builds;
    const b = builds[0];
    $("#fw-status").textContent = b ? `CHDK ${b.version}, revision ${b.revision}` : "";
  } catch (e) {
    if (req !== buildsReq) return;
    state.builds = [];
    $("#fw-status").textContent = `Could not reach the build server: ${e}`;
  }
  renderFirmwares(state.builds);
}

function renderFirmwares(builds) {
  const list = $("#fw-list");
  list.replaceChildren();
  for (const fw of state.camera.firmwares) {
    const available = builds === null || builds.some((b) => b.firmware === fw);
    const tile = el("button", {
      className: "item" + (state.firmware === fw ? " selected" : "") + (available ? "" : " unavailable"),
      disabled: !available,
    },
      el("div", { className: "title" }, fwLabel(fw)),
      el("div", { className: "sub" }, available ? `firmware ${fw}` : "no build"));
    tile.onclick = () => { state.firmware = fw; renderFirmwares(builds); renderRail(); };
    tile.ondblclick = () => go("install");
    list.append(tile);
  }
  if (builds && state.firmware && !builds.some((b) => b.firmware === state.firmware)) {
    state.firmware = null;
    renderRail();
  }
}

async function writeVerReq() {
  try {
    const path = await invoke("write_ver_req", { device: state.card.device, cameraId: state.camera.id });
    $("#verreq-status").textContent = `Created ${path}`;
  } catch (e) {
    toast(String(e), true);
  }
}

/* ---------- 4. install ---------- */

function renderInstall() {
  const cam = state.camera, card = state.card;
  const method = state.method === "autoboot" ? "autoboot (lock the card)" : "Firmware Update menu";
  $("#install-summary").textContent =
    `Canon ${cam.name}, firmware ${fwLabel(state.firmware)}, ${state.channel === "stable" ? "stable" : "development"} build → “${card.name}”, started via ${method}.`;
  $$("#package button").forEach((b) => b.classList.toggle("on", (b.dataset.full === "1") === state.full));
  $("#install-btn").disabled = !cardReady(card) || state.busy;
}

function setBusy(b) {
  state.busy = b;
  $$("button").forEach((x) => { if (!x.closest("dialog")) x.disabled = b; });
  if (!b) { renderRail(); if (state.step === "install") renderInstall(); }
}

function log(text, cls) {
  const li = el("li", { className: cls || "" }, text);
  $("#log").append(li);
  return li;
}

async function runInstall() {
  const cam = state.camera;
  // Autoboot can't finish without Full Disk Access: ask before copying anything.
  if (state.method === "autoboot" && !(await ensureFullDiskAccess())) return;
  setDoneMode(false);
  $("#progress").hidden = false;
  $("#fda-help").hidden = true;
  $("#log").replaceChildren();
  $("#bar-fill").style.width = "0%";
  $("#log").classList.add("running");
  setBusy(true);

  let dlLine = null, exLine = null;
  const ch = new Channel();
  ch.onmessage = (m) => {
    if (m.kind === "step") { dlLine = exLine = null; log(m.text); }
    if (m.kind === "download") {
      const pct = m.total ? m.done / m.total : 0;
      $("#bar-fill").style.width = `${Math.round(pct * 50)}%`;
      dlLine ??= log("");
      dlLine.textContent = `Downloaded ${fmtBytes(m.done)}${m.total ? ` of ${fmtBytes(m.total)}` : ""}`;
    }
    if (m.kind === "extract") {
      $("#bar-fill").style.width = `${50 + Math.round((m.done / m.total) * 45)}%`;
      exLine ??= log("");
      exLine.textContent = `Copied ${m.done} of ${m.total} files`;
    }
  };

  try {
    const res = await invoke("install", {
      req: {
        cameraId: cam.id,
        firmware: state.firmware,
        channel: state.channel,
        full: state.full,
        device: state.card.device,
        method: state.method,
      },
      onProgress: ch,
    });
    $("#bar-fill").style.width = "100%";
    state.installed = true;
    showDone(res);
  } catch (e) {
    log(String(e), "error");
    $("#fda-help").hidden = !String(e).includes("Full Disk Access");
  } finally {
    $("#log").classList.remove("running");
    setBusy(false);
  }
}

const ICONS = {
  eject: '<path d="M12 5l7 8H5z"/><path d="M5 18h14"/>',
  lock: '<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>',
  power: '<path d="M12 3v9"/><path d="M6.6 7.4a7.5 7.5 0 1 0 10.8 0"/>',
  play: '<path d="M8 5l11 7-11 7z"/>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  alt: '<rect x="3" y="6" width="18" height="12" rx="3"/><path d="M7.5 15l2-6 2 6M8.2 13h2.6M14 9v6h3"/>',
};

function howtoItem(icon, title, detail) {
  const ico = el("span", { className: "ico" });
  ico.innerHTML = `<svg viewBox="0 0 24 24">${ICONS[icon]}</svg>`;
  const text = el("div", {}, el("strong", {}, title));
  const small = el("small");
  small.innerHTML = detail; // static strings below, may contain <kbd>
  text.append(small);
  return el("li", {}, ico, text);
}

function setDoneMode(done) {
  $("#install-setup").hidden = done;
  $("#install-footer").hidden = done;
  $("#done").hidden = !done;
  $("#done-footer").hidden = !done;
  if (done) { $("#progress").hidden = true; $("#fda-help").hidden = true; }
}

function showDone(res) {
  const cam = state.camera, card = state.card;
  const auto = state.method === "autoboot";
  $("#done-sub").textContent = `${res.files} files copied to “${card.name}”` +
    (res.dataPartition ? ` and “${res.dataPartition}”` : "") + " and checked." +
    (res.bootable ? " The card is marked bootable." : "");
  const facts = [
    ["Camera", cam.name],
    ["Firmware", fwLabel(state.firmware)],
    ["CHDK", `${res.build.version} · r${res.build.revision}`],
    ["Starts by", auto ? "Autoboot" : "Firmware Update"],
  ];
  $("#done-facts").replaceChildren(...facts.map(([k, v]) => el("div", { className: "fact" }, el("span", {}, k), el("strong", { title: v }, v))));

  const steps = auto
    ? [
        ["eject", "Eject the card", "Use the button above, then take the card out."],
        ["lock", "Slide the lock switch down to LOCK", "CHDK lets the camera keep saving photos to a locked card."],
        ["power", "Insert the card and switch the camera on", "The CHDK logo appears briefly. The camera starts in Playback; half-press the shutter to shoot."],
        ["alt", "Press <kbd>Direct Print</kbd> to enter CHDK", "That’s the &lt;ALT&gt; key. While in ALT mode, <kbd>MENU</kbd> opens the CHDK menu."],
      ]
    : [
        ["eject", "Eject the card", "Use the button above, then put it in the camera, unlocked."],
        ["play", "Switch the camera on with <kbd>▶</kbd>", "It must start in Playback mode, or the Firmware Update entry won’t be there."],
        ["menu", "<kbd>MENU</kbd> → Firmware Update… → OK", "In the settings (wrench) tab. The CHDK logo appears. Repeat after every power-on."],
        ["alt", "Press <kbd>Direct Print</kbd> to enter CHDK", "That’s the &lt;ALT&gt; key. While in ALT mode, <kbd>MENU</kbd> opens the CHDK menu."],
      ];
  $("#next-steps").replaceChildren(...steps.map(([i, t, d]) => {
    const li = howtoItem(i, "", d);
    li.querySelector("strong").innerHTML = t;
    return li;
  }));

  $("#tips-camera").textContent = cam.name;
  const notes = [...cam.notes];
  if (auto) notes.unshift("To use the camera without CHDK, just unlock the card.");
  $("#cam-notes").replaceChildren(...notes.map((n) => el("li", {}, n)));
  $("#cam-notes").closest("details").hidden = !notes.length;
  const trouble = [
    `“Missing module”, odd errors or crashes: the firmware letter probably doesn’t match. Check it (Firmware step → “How do I find my firmware version?”) and install the matching build.`,
    auto
      ? "No CHDK logo at power-on: make sure the card is locked, and that it’s in the camera the right way round."
      : "No “Firmware Update…” entry: switch the camera off, then on again using the ▶ button, not the power button.",
    cam.max_card_gb
      ? `Card error in the camera: the ${cam.name} takes cards up to ${cam.max_card_gb} GB. Bigger cards usually don’t work.`
      : null,
    "Camera freezes: take the battery out for a few seconds. CHDK never changes the camera permanently.",
  ].filter(Boolean);
  $("#trouble-list").replaceChildren(...trouble.map((t) => el("li", {}, t)));

  $("#done-log").replaceChildren(...[...$("#log").children].map((li) => li.cloneNode(true)));
  const eject = $("#eject-btn");
  eject.textContent = "Eject card";
  eject.classList.remove("done-state");
  setDoneMode(true);
  $("main").scrollTop = 0;
}

/* ---------- theme ---------- */

function applyTheme(choice) {
  const root = document.documentElement;
  if (choice === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", choice);
  $$("#theme button").forEach((b) => b.classList.toggle("on", b.dataset.themeChoice === choice));
  try { localStorage.setItem("theme", choice); } catch {}
  // Match the native title bar too.
  window.__TAURI__.window?.getCurrentWindow().setTheme(choice === "system" ? null : choice).catch(() => {});
}

/* ---------- Full Disk Access ---------- */

async function checkFullDiskAccess() {
  try { return await invoke("full_disk_access"); } catch { return null; }
}
/* Show the Full Disk Access dialog if access is missing. Returns true when it's on. */
async function ensureFullDiskAccess() {
  const ok = await checkFullDiskAccess();
  if (ok === false) {
    const d = $("#fda-dialog");
    if (!d.open) d.showModal();
    return false;
  }
  return true;
}

function fdaActions() {
  return {
    open: () => invoke("open_full_disk_access").catch((e) => toast(String(e), true)),
    reveal: () => invoke("reveal_app").catch((e) => toast(String(e), true)),
    restart: () => invoke("restart_app"),
  };
}

/* ---------- wiring ---------- */

$$("[data-next]").forEach((b) => (b.onclick = () => go(b.dataset.next)));
$$("#steps li").forEach((li) => (li.onclick = () => go(li.dataset.step)));
$$("a[data-href]").forEach((a) => (a.onclick = (e) => { e.preventDefault(); invoke("open_external", { target: a.dataset.href }); }));
$("#camera-search").oninput = renderCameras;
$("#camera-search").onkeydown = cameraKeys;
$("#card-refresh").onclick = loadCards;
$("#format-btn").onclick = formatCard;
$("#verreq-btn").onclick = writeVerReq;
$$("#channel button").forEach((b) => (b.onclick = () => {
  if (state.channel === b.dataset.channel) return;
  state.channel = b.dataset.channel;
  loadBuilds();
  renderRail();
}));
$$("input[name=method]").forEach((r) => (r.onchange = () => {
  state.method = r.value;
  renderMethod();
  renderRail();
  if (r.value === "autoboot") ensureFullDiskAccess();
}));
$$("#package button").forEach((b) => (b.onclick = () => { state.full = b.dataset.full === "1"; renderInstall(); }));
$$("#theme button").forEach((b) => (b.onclick = () => applyTheme(b.dataset.themeChoice)));
$("#install-btn").onclick = runInstall;

const fda = fdaActions();
$("#fda-open").onclick = fda.open;
$("#fda-reveal").onclick = fda.reveal;
$("#fda-restart").onclick = fda.restart;
$("#fdad-open").onclick = () => { fda.open(); $("#fdad-restart").hidden = false; };
$("#fdad-reveal").onclick = fda.reveal;
$("#fdad-restart").onclick = fda.restart;
window.addEventListener("focus", async () => {
  const d = $("#fda-dialog");
  if (d.open && (await checkFullDiskAccess()) === true) {
    d.close();
    toast("Full Disk Access is on.");
  }
});

$("#reveal-btn").onclick = () => invoke("open_external", { target: state.card.mount_point }).catch((e) => toast(String(e), true));
$("#eject-btn").onclick = async () => {
  const b = $("#eject-btn");
  try {
    await invoke("eject", { device: state.card.device });
    b.textContent = "✓ Ejected: you can remove the card";
    b.classList.add("done-state");
    b.disabled = true;
    $("#reveal-btn").disabled = true;
  } catch (e) {
    toast(String(e), true);
  }
};
$("#again-btn").onclick = () => {
  state.installed = false;
  state.card = null;
  setDoneMode(false);
  $("#reveal-btn").disabled = false;
  $("#eject-btn").disabled = false;
  go("card");
};

(async function init() {
  let theme = "system";
  try { theme = localStorage.getItem("theme") || "system"; } catch {}
  applyTheme(theme);
  state.cameras = await invoke("cameras");
  buildCameraIndex();
  let last = null;
  try { last = localStorage.getItem("camera"); } catch {}
  state.camera = state.cameras.find((c) => c.id === last) ?? (state.cameras.length === 1 ? state.cameras[0] : null);
  renderCameras();
  scrollSelectedCamera();
  $("#camera-search").focus();
  go("camera");
  ensureFullDiskAccess();
})();
