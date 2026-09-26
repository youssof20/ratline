const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const COMMANDS = [
  "/help",
  "/connect",
  "/join",
  "/room",
  "/go",
  "/who",
  "/clear",
  "/wipe",
  "/name",
  "/hist",
  "/leave",
  "/file",
  "/update",
  "/version",
  "/demo",
];

const state = {
  active: null,
  activeKind: null,
  activeLabel: null,
  peers: [],
  rooms: [],
  path: "",
  wipeDeadline: 0,
  codeTimer: null,
  lastCode: null,
  pendingCode: null,
  lastCopiedCode: null,
  myShort: "",
  myId: "",
  lastSys: "",
  lastSysAt: 0,
  index: [],
  demoRunning: false,
  typingTimer: null,
  typingStopTimer: null,
  lastPeerOnline: {},
};

const $ = (id) => document.getElementById(id);

function short(hex) {
  return (hex || "").slice(0, 8);
}

function humanError(err) {
  const s = String(err);
  if (/your own code|ourself/i.test(s)) {
    return "that's your own code - give it to someone else";
  }
  if (/No addressing information|pkarr|TXT record|dns/i.test(s)) {
    return "could not find them - code expired, wrong, or offline";
  }
  const line = s.split("\n")[0].trim();
  if (/^dial pairing host/i.test(line)) {
    return "could not reach them (code invalid or expired?)";
  }
  if (/already on latest/i.test(line)) {
    return line;
  }
  if (line.length > 120) {
    return line.slice(0, 117) + "...";
  }
  return line;
}

function setCodeTtl(text, cls) {
  const el = $("code-ttl");
  el.textContent = text || "";
  el.className = "code-ttl" + (cls ? " " + cls : "");
}

function clearCodeTimer() {
  clearInterval(state.codeTimer);
  state.codeTimer = null;
  state.lastCode = null;
  setCodeTtl("");
}

function prettyPath(path) {
  const p = (path || "").toUpperCase();
  if (!p || p === "...") return "linking…";
  if (p === "DIRECT") return "direct";
  if (p === "RELAYED") return "relayed";
  if (p === "OFFLINE") return "offline";
  if (p === "WAITING") return "waiting";
  if (p === "LIVE") return "live";
  return p.toLowerCase();
}

function setPath(path) {
  const raw = (path || "").toUpperCase();
  const prev = state.path;
  state.path = raw;
  const el = $("path-label");
  el.textContent = prettyPath(raw);
  el.classList.remove("live", "relay", "wait", "offline");
  if (raw === "DIRECT") {
    if (prev !== "DIRECT") {
      void el.offsetWidth;
      el.classList.add("live");
    } else {
      el.classList.add("live");
    }
  } else if (raw === "RELAYED") {
    if (prev !== "RELAYED") {
      void el.offsetWidth;
      el.classList.add("relay");
    } else {
      el.classList.add("relay");
    }
  } else if (raw === "WAITING" || raw === "...") {
    el.classList.add("wait");
  } else if (raw === "OFFLINE") {
    el.classList.add("offline");
  }
}

function updateStatusBar() {
  if (!state.active) {
    $("status-conv").textContent = "";
    setPath("");
    updatePlaceholder();
    return;
  }
  const tag = state.activeKind === "room" ? "group" : "dm";
  const name = state.activeLabel || short(state.active);
  $("status-conv").textContent = `${tag}:${name}`;
  updatePlaceholder();
}

function updatePlaceholder() {
  const el = $("input");
  if (!el) return;
  if (state.active) {
    el.placeholder = "message";
  } else {
    el.placeholder = "/connect  /join  /help";
  }
}

function roomMemberCount() {
  if (state.activeKind !== "room" || !state.active) return 0;
  const r = state.rooms.find((x) => x.topic_id === state.active);
  return (r?.members || []).length;
}

function isGroupRoom() {
  return state.activeKind === "room" && roomMemberCount() + 1 > 2;
}

function peerLabel(senderId) {
  if (!senderId) return short(state.active) || "peer";
  if (state.myId && senderId === state.myId) return "me";
  const p = state.peers.find((x) => x.endpoint_id === senderId);
  if (p?.label) return p.label;
  const r = state.rooms.find((x) => x.topic_id === state.active);
  const m = (r?.members || []).find((x) => x === senderId || x?.id === senderId);
  if (m && typeof m === "object" && m.label) return m.label;
  return short(senderId);
}

function clearScreen() {
  $("log").innerHTML = "";
  $("typing").textContent = "";
}

function clearIdle() {
  const log = $("log");
  log.querySelectorAll(".idle-mark, .idle-hint").forEach((n) => n.remove());
}

function sys(text, cls) {
  const t = String(text);
  const now = Date.now();
  if (t === state.lastSys && now - state.lastSysAt < 2500) return;
  state.lastSys = t;
  state.lastSysAt = now;

  const log = $("log");
  clearIdle();
  const line = document.createElement("div");
  let kind = cls || "";
  if (!kind && /own code|could not|unknown|not found|too slow|expired|offline/i.test(t)) {
    kind = "err";
  } else if (!kind && /copied|connected|direct|joined|named |history on|erased|line live|group live|up to date/i.test(t)) {
    kind = "ok";
  }
  line.className = `line sys ${kind}`.trim();
  line.textContent = t;
  log.appendChild(line);
  log.scrollTop = log.scrollHeight;
}

function sysValue(prefix, value, suffix) {
  const log = $("log");
  clearIdle();
  const line = document.createElement("div");
  line.className = "line sys";
  line.appendChild(document.createTextNode(prefix));
  const val = document.createElement("span");
  val.className = "val";
  val.textContent = value;
  val.title = "click to copy";
  val.onclick = async (e) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(value);
      state.lastCopiedCode = value;
      sys("copied", "ok");
    } catch {
      /* ignore */
    }
  };
  line.appendChild(val);
  if (suffix) line.appendChild(document.createTextNode(suffix));
  log.appendChild(line);
  log.scrollTop = log.scrollHeight;
}

function sysProgress(text) {
  const log = $("log");
  clearIdle();
  let line = log.querySelector(".line.sys.progress");
  if (!line) {
    line = document.createElement("div");
    line.className = "line sys progress";
    log.appendChild(line);
  }
  line.textContent = text;
  log.scrollTop = log.scrollHeight;
}

function clearProgress() {
  const line = $("log").querySelector(".line.sys.progress");
  if (line) line.remove();
}

function appendMsg(body, cls, opts = {}, meta) {
  if (!opts || typeof opts !== "object") opts = {};
  const log = $("log");
  clearIdle();

  const line = document.createElement("div");
  line.className = `line ${cls}${opts.hist ? " hist" : ""}`;
  log.appendChild(line);

  if (meta?.kind === "file") {
    line.style.cursor = "pointer";
    line.title = "click to save";
    line.onclick = async () => {
      const m = body.match(/FILE (.+) \((\d+)b\) (.+)$/);
      if (!m) return;
      const dest = await invoke("pick_save", { defaultName: m[1] });
      if (!dest) return;
      try {
        await invoke("download_file", {
          fromEndpoint: meta.sender_id,
          hash: m[3],
          dest,
        });
        sys(`saved ${m[1]}`, "ok");
      } catch (e) {
        sys(humanError(e));
      }
    };
  }

  let prefix;
  if (isGroupRoom()) {
    const shown = cls === "out" ? "me" : peerLabel(meta?.sender_id);
    prefix = `<${shown}> `;
  } else {
    prefix = cls === "out" ? "> " : "< ";
  }

  line.textContent = prefix + body;
  log.scrollTop = log.scrollHeight;
}

function showIdle(mode = "home") {
  const log = $("log");
  if (log.querySelector(".idle-mark")) return;
  if (log.children.length > 0) return;

  const mark = document.createElement("div");
  mark.className = "idle-mark";
  mark.textContent = "<?>";
  log.appendChild(mark);

  if (mode === "home") {
    const a = document.createElement("div");
    a.className = "idle-hint";
    a.textContent = "one line. encrypted.";
    log.appendChild(a);
    const b = document.createElement("div");
    b.className = "idle-hint";
    b.textContent = "/connect  ·  /join  ·  /help";
    log.appendChild(b);
  } else if (mode === "chat") {
    const a = document.createElement("div");
    a.className = "idle-hint";
    a.textContent = "quiet";
    log.appendChild(a);
  } else if (mode === "waiting") {
    const a = document.createElement("div");
    a.className = "idle-hint";
    a.textContent = "waiting…";
    log.appendChild(a);
  }
}

async function refreshLists() {
  const status = await invoke("get_status");
  state.myId = status.endpoint_id || "";
  state.myShort = status.short_id || "";
  $("short-id").textContent = state.myShort || "····";
  state.pendingCode = status.pairing_code || null;

  const prevPeers = state.peers;
  state.peers = await invoke("list_peers");
  state.rooms = await invoke("list_rooms");

  // Soft disconnect notice for active dm
  if (state.active && state.activeKind === "peer") {
    const p = state.peers.find((x) => x.endpoint_id === state.active);
    const was = prevPeers.find((x) => x.endpoint_id === state.active);
    if (p && was?.connected && !p.connected) {
      sys("line dropped", "err");
      $("typing").textContent = "";
    } else if (p && was && !was.connected && p.connected) {
      sys("line live", "ok");
    }
  }

  if (state.active && state.activeKind === "peer") {
    const p = state.peers.find((x) => x.endpoint_id === state.active);
    if (p) {
      state.activeLabel = p.label || short(p.endpoint_id);
      setPath(p.connected ? (p.path || "").toUpperCase() : "OFFLINE");
    }
  } else if (state.active && state.activeKind === "room") {
    const r = state.rooms.find((x) => x.topic_id === state.active);
    if (r) {
      state.activeLabel = r.label || short(r.topic_id);
      setPath((r.members || []).length ? "LIVE" : "WAITING");
    }
  }
  updateStatusBar();

  if (status.pairing_code && status.pairing_expires_in != null) {
    trackCodeExpiry(
      status.pairing_code,
      status.pairing_kind,
      status.pairing_expires_in
    );
  } else if (!status.pairing_code) {
    clearCodeTimer();
  }
  return status;
}

function trackCodeExpiry(code, kind, secs) {
  if (kind === "room") {
    setCodeTtl("room");
    return;
  }
  if (state.lastCode === code && state.codeTimer) return;
  state.lastCode = code;
  clearInterval(state.codeTimer);

  let left = Math.max(0, Math.floor(secs));
  const updateTtl = () => {
    if (left <= 0) {
      clearCodeTimer();
      sys("code expired");
      return;
    }
    let cls = "";
    if (left <= 60) cls = "critical";
    else if (left <= 180) cls = "warn";
    const m = Math.floor(left / 60);
    const s = left % 60;
    const label =
      m > 0 ? `${m}:${String(s).padStart(2, "0")}` : `${left}s`;
    setCodeTtl(label, cls);
    left -= 1;
  };
  updateTtl();
  state.codeTimer = setInterval(updateTtl, 1000);
}

async function openConv(id, kind, label, path, opts = {}) {
  const force = opts.force === true;
  if (!force && state.active === id) return;

  state.active = id;
  state.activeKind = kind;
  state.activeLabel = label || short(id);
  setPath(path || "");
  updateStatusBar();
  clearScreen();

  try {
    const hist = await invoke("get_history", { conversationId: id });
    for (const m of hist) {
      appendMsg(m.body, m.outgoing ? "out" : "in", { hist: true }, m);
    }
  } catch (e) {
    sys(humanError(e));
  }

  if ($("log").children.length === 0) {
    const waiting =
      kind === "room" && (!path || String(path).toUpperCase() === "WAITING");
    showIdle(waiting ? "waiting" : "chat");
  }
}

function buildIndex() {
  state.index = [];
  for (const p of state.peers) {
    state.index.push({
      id: p.endpoint_id,
      kind: "peer",
      label: p.label || short(p.endpoint_id),
      path: p.connected ? p.path : "OFFLINE",
    });
  }
  for (const r of state.rooms) {
    const n = (r.members || []).length;
    state.index.push({
      id: r.topic_id,
      kind: "room",
      label: r.label || short(r.topic_id),
      path: n ? "LIVE" : "WAITING",
      members: n,
    });
  }
  return state.index;
}

function resolveTarget(arg) {
  const idx = buildIndex();
  if (!arg) return null;
  const asNum = Number(arg);
  if (Number.isInteger(asNum) && asNum >= 1 && asNum <= idx.length) {
    return idx[asNum - 1];
  }
  const q = arg.toLowerCase();
  return (
    idx.find((c) => c.label.toLowerCase() === q) ||
    idx.find((c) => c.label.toLowerCase().startsWith(q)) ||
    idx.find((c) => c.id.startsWith(q) || short(c.id) === q) ||
    null
  );
}

function isValidJoinCode(code) {
  const c = (code || "").trim().toUpperCase();
  return c.startsWith("P-") || c.startsWith("R-");
}

function isOwnPendingCode(code) {
  if (!code) return false;
  const norm = code.trim().toUpperCase();
  if (state.pendingCode && state.pendingCode.toUpperCase() === norm) return true;
  return false;
}

function listWho() {
  const idx = buildIndex();
  if (!idx.length) {
    sys("no connections");
    return;
  }
  idx.forEach((c, i) => {
    const kindLabel = c.kind === "room" ? "group" : "dm";
    const extra = c.kind === "room" ? ` · ${c.members ?? 0}` : "";
    sys(
      `${i + 1}. ${kindLabel} ${c.label}  ${prettyPath(c.path)}${extra}  ${short(c.id)}`
    );
  });
}

async function copyCode(code) {
  state.lastCopiedCode = code;
  try {
    await navigator.clipboard.writeText(code);
    sys("copied", "ok");
  } catch {
    /* ignore */
  }
}

function normalizeCommand(cmd) {
  const map = {
    "/c": "/connect",
    "/j": "/join",
    "/r": "/room",
    "/h": "/help",
    "/w": "/who",
    "/u": "/update",
    "/v": "/version",
    "/?": "/help",
  };
  return map[cmd] || cmd;
}

async function runCommand(raw) {
  const line = raw.trim();
  const parts = line.split(/\s+/);
  const cmd = normalizeCommand(parts[0].toLowerCase());
  const args = parts.slice(1);
  const rest = line.slice(parts[0].length).trim();

  switch (cmd) {
    case "/help": {
      if (args[0] === "more" || args[0] === "all") {
        sys("<?>", "help");
        sys("/name <name>  local label for current chat");
        sys("/hist on|off  local encrypted history");
        sys("/wipe  erase saved history here (confirm in 10s)");
        sys("/leave  disconnect current");
        sys("/file [path]  send a file");
        sys("/update  install latest from GitHub");
        sys("/version  show build");
        sys("/demo [1|2]  scripted recording run");
        break;
      }
      sys("<?>", "help");
      sys("/connect  P- code for 1:1  ·  they /join it");
      sys("/room  R- code for a group");
      sys("/join <code>  enter theirs");
      sys("/go <n|name>  switch  ·  /who  list  ·  /clear");
      sys("/help more");
      break;
    }
    case "/version": {
      const cfg = await invoke("get_launch_config");
      sys(`ratline v${cfg.version}`, "ok");
      break;
    }
    case "/update": {
      try {
        const info = await invoke("check_update");
        if (!info.available) {
          sys(`up to date · ${info.current}`, "ok");
          break;
        }
        sysValue("update  ", `${info.current} → ${info.latest}`);
        if (info.notes) sys(info.notes.slice(0, 120));
        sys("downloading…");
        await invoke("run_update");
        sys("installer launched · restarting", "ok");
      } catch (e) {
        clearProgress();
        sys(humanError(e));
      }
      break;
    }
    case "/clear": {
      clearScreen();
      showIdle(state.active ? "chat" : "home");
      break;
    }
    case "/demo": {
      const which = args[0] || "1";
      const kind =
        which === "2" || which === "commands" || which === "cmd"
          ? "commands"
          : "conversation";
      await runDemo(kind, "host");
      break;
    }
    case "/connect": {
      if (args.length > 0) {
        sys("/connect takes no arguments");
        break;
      }
      const code = await invoke("start_pairing");
      await refreshLists();
      sysValue("code  ", code);
      await copyCode(code);
      break;
    }
    case "/room": {
      const [code, topic] = await invoke("start_room");
      await refreshLists();
      sysValue("code  ", code);
      await copyCode(code);
      await openConv(topic, "room", short(topic), "WAITING", { force: true });
      break;
    }
    case "/join": {
      if (!args[0]) {
        sys("usage: /join <P-|R-code>");
        break;
      }
      await doJoin(args[0].trim());
      break;
    }
    case "/who": {
      listWho();
      break;
    }
    case "/go": {
      if (!args[0]) {
        listWho();
        break;
      }
      const t = resolveTarget(args[0]);
      if (!t) {
        sys("not found - /who");
        break;
      }
      await openConv(t.id, t.kind, t.label, t.path, { force: true });
      break;
    }
    case "/name": {
      if (!state.active) {
        sys("no conversation");
        break;
      }
      if (!rest) {
        sys("usage: /name <name>");
        break;
      }
      await invoke("set_label", { endpointId: state.active, label: rest });
      state.activeLabel = rest;
      updateStatusBar();
      sys(`named ${rest}`, "ok");
      break;
    }
    case "/hist": {
      const mode = (args[0] || "").toLowerCase();
      if (mode !== "on" && mode !== "off") {
        const s = await invoke("get_status");
        sys(`history ${s.history_enabled ? "on" : "off"} · local, encrypted`);
        break;
      }
      const on = mode === "on";
      await invoke("set_history", { enabled: on });
      sys(on ? "history on · local, encrypted" : "history off · nothing new kept");
      break;
    }
    case "/wipe": {
      if (!state.active) {
        sys("no conversation");
        break;
      }
      if (args[0] === "confirm") {
        if (Date.now() > state.wipeDeadline) {
          sys("wipe expired - /wipe again");
          break;
        }
        await invoke("wipe_history", { conversationId: state.active });
        state.wipeDeadline = 0;
        sys("local history cleared", "ok");
        break;
      }
      state.wipeDeadline = Date.now() + 10000;
      sys("clears local copy only");
      sys("confirm: /wipe confirm  (10s)");
      break;
    }
    case "/leave": {
      if (!state.active) {
        sys("no conversation");
        break;
      }
      const id = state.active;
      const label = state.activeLabel || short(id);
      await invoke("leave_conversation", { conversationId: id });
      sys(`left ${label}`);
      state.active = null;
      state.activeKind = null;
      state.activeLabel = null;
      updateStatusBar();
      clearScreen();
      showIdle("home");
      break;
    }
    case "/file": {
      if (!state.active) {
        sys("no conversation");
        break;
      }
      let path = rest || null;
      if (!path) {
        path = await invoke("pick_file");
      }
      if (!path) {
        sys("cancelled");
        break;
      }
      try {
        await invoke("send_file", { conversationId: state.active, path });
      } catch (e) {
        sys(humanError(e));
      }
      break;
    }
    default:
      sys("unknown - /help");
  }
}

async function doJoin(code) {
  if (!isValidJoinCode(code)) {
    sys("codes start with P- (1:1) or R- (group)");
    return;
  }
  if (isOwnPendingCode(code)) {
    sys("that's your own code - give it to someone else");
    return;
  }
  let info;
  try {
    info = await invoke("inspect_code", { code });
  } catch (e) {
    sys(humanError(e));
    return;
  }
  if (info.kind === "room") {
    sys(
      info.reachable
        ? `joining · ${info.members ?? "?"} present`
        : "joining · host not reached yet"
    );
  } else {
    sys("connecting…");
  }
  try {
    const result = await invoke("join_code", { code });
    await refreshLists();
    if (result.kind === "peer") {
      const p = result.peer;
      await openConv(
        p.endpoint_id,
        "peer",
        p.label || short(p.endpoint_id),
        p.path,
        { force: true }
      );
      sys("line live", "ok");
    } else {
      const r = result.room;
      const live = (r.members || []).length > 0;
      await openConv(
        r.topic_id,
        "room",
        r.label,
        live ? "LIVE" : "WAITING",
        { force: true }
      );
      sys(live ? "group live" : "waiting for others", "ok");
    }
  } catch (e) {
    sys(humanError(e));
  }
}

async function tryWipeConfirm(raw) {
  if (!state.wipeDeadline || Date.now() > state.wipeDeadline) return false;
  if (raw.trim().toLowerCase() !== "confirm") return false;
  if (!state.active) {
    state.wipeDeadline = 0;
    return false;
  }
  await invoke("wipe_history", { conversationId: state.active });
  state.wipeDeadline = 0;
  sys("local history cleared", "ok");
  return true;
}

async function onSubmit() {
  const el = $("input");
  const raw = el.value;
  if (!raw.trim()) return;
  el.value = "";
  autoSize();
  updatePlaceholder();

  if (raw.trimStart().startsWith("/")) {
    sys(`> ${raw.trim()}`);
    try {
      await runCommand(raw.trim());
    } catch (e) {
      sys(humanError(e));
    }
    return;
  }

  if (await tryWipeConfirm(raw)) return;

  // Bare P-/R- code in the input = join
  const maybe = raw.trim();
  if (isValidJoinCode(maybe) && !state.active) {
    sys(`> /join ${maybe}`);
    await doJoin(maybe);
    return;
  }

  if (!state.active) {
    sys("no line yet - /connect /join /room");
    return;
  }
  try {
    await invoke("send_text", { conversationId: state.active, body: raw });
    stopTyping();
  } catch (e) {
    sys(humanError(e));
  }
}

function autoSize() {
  const el = $("input");
  el.style.height = "auto";
  el.style.height = Math.min(el.scrollHeight, 120) + "px";
}

function stopTyping() {
  clearTimeout(state.typingTimer);
  clearTimeout(state.typingStopTimer);
  state.typingTimer = null;
  state.typingStopTimer = null;
  if (state.active) {
    invoke("send_typing", { conversationId: state.active, active: false });
  }
}

function pulseTyping() {
  if (!state.active) return;
  if ($("input").value.startsWith("/")) return;
  clearTimeout(state.typingTimer);
  state.typingTimer = setTimeout(() => {
    invoke("send_typing", { conversationId: state.active, active: true });
  }, 180);
  clearTimeout(state.typingStopTimer);
  state.typingStopTimer = setTimeout(() => {
    if (state.active) {
      invoke("send_typing", { conversationId: state.active, active: false });
    }
  }, 1600);
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function waitUntil(pred, timeoutMs = 45000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (await pred()) return true;
    await sleep(200);
  }
  return false;
}

const DEMO_LINES = [
  { from: "host", text: "line still dark?" },
  { from: "peer", text: "dark enough" },
  { from: "host", text: "relay saw us once" },
  { from: "peer", text: "then we cut it" },
  { from: "host", text: "hold" },
];

async function runDemo(kind, role) {
  state.demoRunning = true;
  try {
    if (kind === "conversation") {
      if (role === "joiner") await demoConversationJoiner();
      else await demoConversationHost();
    } else {
      if (role === "joiner") await demoCommandsJoiner();
      else await demoCommandsHost();
    }
  } finally {
    state.demoRunning = false;
  }
}

async function demoConversationHost() {
  sys("demo · conversation");
  sys("> /connect");
  const code = await invoke("start_pairing");
  await refreshLists();
  sysValue("code  ", code);
  await invoke("spawn_demo_peer", { code });
  const ok = await waitUntil(async () => {
    await refreshLists();
    return state.peers.some((p) => p.connected);
  });
  if (!ok) {
    sys("demo: peer did not connect");
    return;
  }
  const peer = state.peers.find((p) => p.connected);
  await openConv(peer.endpoint_id, "peer", "wire", peer.path, { force: true });
  await sleep(600);
  for (const line of DEMO_LINES) {
    if (line.from === "host") {
      await sleep(400);
      await invoke("send_typing", { conversationId: state.active, active: true });
      await sleep(500);
      await invoke("send_text", { conversationId: state.active, body: line.text });
      await invoke("send_typing", { conversationId: state.active, active: false });
      await sleep(900);
    } else {
      await waitUntil(async () => true, 50);
      await sleep(1400);
    }
  }
  await sleep(800);
  showIdleMarkOnly();
}

async function demoConversationJoiner() {
  const cfg = await invoke("get_launch_config");
  const code = cfg.demo_code;
  if (!code) {
    sys("demo joiner: no code");
    return;
  }
  await sleep(1500);
  sys("demo · joiner");
  sys(`> /join ${code}`);
  const result = await invoke("join_code", { code });
  await refreshLists();
  if (result.kind === "peer") {
    const p = result.peer;
    await openConv(p.endpoint_id, "peer", "wire", p.path, { force: true });
  }
  await sleep(400);
  for (const line of DEMO_LINES) {
    if (line.from === "peer") {
      await sleep(700);
      await invoke("send_typing", { conversationId: state.active, active: true });
      await sleep(450);
      await invoke("send_text", { conversationId: state.active, body: line.text });
      await invoke("send_typing", { conversationId: state.active, active: false });
      await sleep(800);
    } else {
      await sleep(1200);
    }
  }
  await sleep(600);
  showIdleMarkOnly();
}

async function demoCommandsHost() {
  sys("demo · commands");
  await sleep(400);
  sys("> /connect");
  const code = await invoke("start_pairing");
  await refreshLists();
  sysValue("code  ", code);
  await invoke("spawn_demo_peer", { code });
  const ok = await waitUntil(async () => {
    await refreshLists();
    return state.peers.some((p) => p.connected);
  }, 60000);
  if (!ok) {
    sys("demo: peer did not connect");
    return;
  }
  const peer = state.peers.find((p) => p.connected);
  await openConv(
    peer.endpoint_id,
    "peer",
    short(peer.endpoint_id),
    peer.path,
    { force: true }
  );
  await sleep(500);
  sys("> ping");
  await invoke("send_typing", { conversationId: state.active, active: true });
  await sleep(600);
  await invoke("send_text", { conversationId: state.active, body: "ping" });
  await invoke("send_typing", { conversationId: state.active, active: false });
  await sleep(1800);
  sys("> /who");
  await runCommand("/who");
  await sleep(1200);
}

async function demoCommandsJoiner() {
  const cfg = await invoke("get_launch_config");
  const code = cfg.demo_code;
  if (!code) return;
  await sleep(2000);
  sys(`> /join ${code}`);
  const result = await invoke("join_code", { code });
  await refreshLists();
  if (result.kind === "peer") {
    const p = result.peer;
    await openConv(p.endpoint_id, "peer", short(p.endpoint_id), p.path, {
      force: true,
    });
  }
  await waitUntil(async () => !!state.active);
  await sleep(2500);
  await invoke("send_typing", { conversationId: state.active, active: true });
  await sleep(400);
  await invoke("send_text", { conversationId: state.active, body: "pong" });
  await invoke("send_typing", { conversationId: state.active, active: false });
}

function showIdleMarkOnly() {
  const log = $("log");
  const mark = document.createElement("div");
  mark.className = "idle-mark";
  mark.textContent = "<?>";
  log.appendChild(mark);
  log.scrollTop = log.scrollHeight;
}

function tabComplete() {
  const el = $("input");
  const v = el.value;
  if (!v.startsWith("/") || v.includes(" ")) return false;
  const matches = COMMANDS.filter((c) => c.startsWith(v.toLowerCase()));
  if (matches.length === 1) {
    el.value = matches[0] + (matches[0] === "/join" || matches[0] === "/go" || matches[0] === "/name" || matches[0] === "/hist" || matches[0] === "/file" || matches[0] === "/demo" ? " " : "");
    return true;
  }
  if (matches.length > 1) {
    sys(matches.join("  "));
    return true;
  }
  return false;
}

async function playBoot() {
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const bootEl = $("boot");
  const appEl = $("app");
  if (reduce || !bootEl) {
    if (bootEl) bootEl.classList.add("done");
    if (appEl) appEl.classList.add("ready");
    return;
  }
  await sleep(1180);
  bootEl.classList.add("done");
  appEl.classList.add("ready");
  await sleep(160);
}

async function boot() {
  await playBoot();
  showIdle("home");
  await refreshLists();
  updatePlaceholder();

  invoke("check_update")
    .then((info) => {
      if (info?.available) {
        sysValue("update  ", `${info.current} → ${info.latest}`);
        sys("type /update");
      }
    })
    .catch(() => {});

  await listen("message", (ev) => {
    const m = ev.payload;
    if (m.conversation_id !== state.active) {
      if (!m.outgoing && !state.demoRunning) {
        const label =
          peerLabel(m.sender_id) || short(m.conversation_id) || "peer";
        sys(`· ${label}`, "ok");
      }
      return;
    }
    appendMsg(m.body, m.outgoing ? "out" : "in", false, m);
  });

  await listen("peer_update", () => refreshLists());
  await listen("presence", () => refreshLists());

  await listen("paired", async (ev) => {
    if (state.demoRunning) {
      await refreshLists();
      return;
    }
    const id = ev.payload.endpoint_id;
    if (state.myId && id === state.myId) {
      await refreshLists();
      return;
    }
    if (state.active === id) {
      await refreshLists();
      return;
    }
    await refreshLists();
    await openConv(
      id,
      "peer",
      ev.payload.label || short(id),
      ev.payload.path || "DIRECT",
      { force: true }
    );
    sys("line live", "ok");
  });

  await listen("conn_path", (ev) => {
    if (ev.payload.conversation_id === state.active) {
      setPath((ev.payload.path || "").toUpperCase());
    }
    refreshLists();
  });

  await listen("typing", (ev) => {
    if (ev.payload.conversation_id !== state.active) return;
    if (!ev.payload.active) {
      $("typing").textContent = "";
      return;
    }
    const who = isGroupRoom()
      ? peerLabel(ev.payload.sender || ev.payload.sender_id)
      : null;
    $("typing").textContent = who ? `${who} ···` : "···";
  });

  await listen("pairing_expired", () => {
    clearCodeTimer();
    sys("code expired");
  });

  await listen("file_progress", (ev) => {
    if (ev.payload.conversation_id !== state.active) return;
    const pct = ev.payload.pct ?? 0;
    if (pct >= 100) {
      clearProgress();
      return;
    }
    const width = 24;
    const filled = Math.floor((pct * width) / 100);
    const bar = `[${"█".repeat(filled)}${"░".repeat(width - filled)}] ${pct}%`;
    sysProgress(`${bar}  ${ev.payload.phase || "file"}`);
  });

  await listen("update_progress", (ev) => {
    const p = ev.payload || {};
    if (p.phase === "install") {
      sysProgress(`${p.bar}  installing ${p.to}`);
      return;
    }
    if (p.phase === "start") {
      sysProgress(`${p.bar}  ${p.from} → ${p.to}`);
      return;
    }
    sysProgress(`${p.bar}  ${p.to}`);
  });

  $("input").addEventListener("input", () => {
    autoSize();
    pulseTyping();
  });

  $("input").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      onSubmit();
      return;
    }
    if (e.key === "Tab") {
      if (tabComplete()) e.preventDefault();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      $("input").value = "";
      autoSize();
      stopTyping();
    }
  });

  $("input").addEventListener("paste", (e) => {
    const text = (e.clipboardData || window.clipboardData)?.getData("text") || "";
    const trimmed = text.trim();
    if (!isValidJoinCode(trimmed)) return;
    if ($("input").value.trim()) return;
    e.preventDefault();
    $("input").value = `/join ${trimmed}`;
    autoSize();
  });

  $("short-id").addEventListener("click", async () => {
    if (!state.myShort) return;
    try {
      await navigator.clipboard.writeText(state.myId || state.myShort);
      sys("id copied", "ok");
    } catch {
      /* ignore */
    }
  });

  $("composer").onsubmit = (e) => {
    e.preventDefault();
    onSubmit();
  };

  $("input").focus();
  setInterval(refreshLists, 3000);

  try {
    const cfg = await invoke("get_launch_config");
    if (cfg.demo) {
      await sleep(400);
      await runDemo(cfg.demo, cfg.role === "joiner" ? "joiner" : "host");
    }
  } catch {
    /* ignore */
  }
}

boot().catch((e) => {
  document.body.textContent = String(e);
});
