const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const state = {
  active: null,
  activeKind: null,
  activeLabel: null,
  peers: [],
  rooms: [],
  path: "-",
  wipeDeadline: 0,
  codeTimer: null,
  lastCode: null,
  index: [],
  demoRunning: false,
};

const $ = (id) => document.getElementById(id);

function short(hex) {
  return (hex || "").slice(0, 8);
}

function pathClass(path) {
  const p = (path || "").toUpperCase();
  if (p === "DIRECT") return "direct";
  if (p === "RELAYED") return "relayed";
  if (p === "LIVE" || p === "WAITING") return "live";
  return "offline";
}

function setPath(path, pulse) {
  const prev = state.path;
  state.path = (path || "-").toUpperCase();
  $("path-label").textContent = state.path;
  const dot = $("path-dot");
  dot.className = `dot ${pathClass(state.path)}`;
  if (pulse && prev !== state.path) {
    dot.classList.remove("pulse");
    void dot.offsetWidth;
    dot.classList.add("pulse");
  }
}

function updateStatusBar() {
  if (!state.active) {
    $("status-conv").textContent = "-";
    setPath("-", false);
    return;
  }
  const tag = state.activeKind === "room" ? "room" : "peer";
  const name = state.activeLabel || short(state.active);
  $("status-conv").textContent = `${tag}:${name}`;
}

function sys(text, cls) {
  const log = $("log");
  // remove idle mark if present
  const idle = log.querySelector(".idle-mark");
  if (idle) idle.remove();

  const line = document.createElement("div");
  line.className = `line sys ${cls || ""}`;
  line.textContent = text;
  log.appendChild(line);
  log.scrollTop = log.scrollHeight;
}

function helpLine(html) {
  const log = $("log");
  const idle = log.querySelector(".idle-mark");
  if (idle) idle.remove();
  const line = document.createElement("div");
  line.className = "line help";
  line.innerHTML = html;
  log.appendChild(line);
  log.scrollTop = log.scrollHeight;
}

function appendMsg(body, cls, typewriter, meta) {
  const log = $("log");
  const idle = log.querySelector(".idle-mark");
  if (idle) idle.remove();

  const line = document.createElement("div");
  line.className = `line ${cls}`;
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
        sys(`saved ${m[1]}`);
      } catch (e) {
        sys(String(e));
      }
    };
  }

  const paint = (n) => {
    line.textContent = "";
    const who = document.createElement("span");
    who.className = "who";
    who.textContent = cls === "out" ? "you> " : `${short(meta?.sender_id)}> `;
    line.appendChild(who);
    line.appendChild(document.createTextNode(body.slice(0, n)));
  };

  if (!typewriter) {
    paint(body.length);
    log.scrollTop = log.scrollHeight;
    return;
  }

  let i = 0;
  const cursor = document.createElement("span");
  cursor.className = "tw-cursor";
  const tick = () => {
    paint(i);
    if (i < body.length) {
      line.appendChild(cursor);
      i++;
      log.scrollTop = log.scrollHeight;
      setTimeout(tick, 6 + Math.random() * 10);
    }
  };
  tick();
}

function showIdle() {
  const log = $("log");
  if (log.querySelector(".idle-mark")) return;
  if (log.children.length > 0) return;
  const mark = document.createElement("div");
  mark.className = "idle-mark";
  mark.innerHTML = '&lt;?&gt;<span class="block-cursor"></span>';
  log.appendChild(mark);
  sys("type /help");
}

async function refreshLists() {
  const status = await invoke("get_status");
  $("short-id").textContent = status.short_id;
  state.peers = await invoke("list_peers");
  state.rooms = await invoke("list_rooms");

  if (state.active && state.activeKind === "peer") {
    const p = state.peers.find((x) => x.endpoint_id === state.active);
    if (p) {
      state.activeLabel = p.label || short(p.endpoint_id);
      setPath(p.connected ? p.path : "OFFLINE", true);
    }
  } else if (state.active && state.activeKind === "room") {
    const r = state.rooms.find((x) => x.topic_id === state.active);
    if (r) {
      state.activeLabel = r.label || short(r.topic_id);
      setPath((r.members || []).length ? "LIVE" : "WAITING", true);
    }
  }
  updateStatusBar();

  // code countdown into log (update last code line via dedicated tracker)
  if (status.pairing_code && status.pairing_expires_in != null) {
    trackCodeExpiry(status.pairing_code, status.pairing_kind, status.pairing_expires_in);
  }
  return status;
}

function trackCodeExpiry(code, kind, secs) {
  if (state.lastCode === code && state.codeTimer) return;
  state.lastCode = code;
  clearInterval(state.codeTimer);
  let left = secs;
  const kindLabel = kind === "room" ? "room" : "peer";
  sys(`${kindLabel} code  ${code}`);
  if (kind === "room") {
    sys("reusable while this room is open");
    return;
  }
  const tick = () => {
    if (left <= 0) {
      clearInterval(state.codeTimer);
      state.codeTimer = null;
      sys("code expired");
      return;
    }
    // only spam every 30s or when urgent
    if (left === secs || left % 30 === 0 || left <= 60) {
      const cls = left <= 60 ? "ttl-critical" : left <= 180 ? "ttl-urgent" : "";
      sys(`expires in ${left}s`, cls);
    }
    left -= 1;
  };
  tick();
  state.codeTimer = setInterval(tick, 1000);
}

async function openConv(id, kind, label, path) {
  state.active = id;
  state.activeKind = kind;
  state.activeLabel = label || short(id);
  setPath(path || "-", true);
  updateStatusBar();
  sys(`${kind} ${state.activeLabel}`);
  try {
    const hist = await invoke("get_history", { conversationId: id });
    for (const m of hist) {
      appendMsg(m.body, m.outgoing ? "out" : "in", false, m);
    }
  } catch (e) {
    sys(String(e));
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

async function runCommand(raw) {
  const line = raw.trim();
  const parts = line.split(/\s+/);
  const cmd = parts[0].toLowerCase();
  const args = parts.slice(1);
  const rest = line.slice(parts[0].length).trim();

  switch (cmd) {
    case "/help":
    case "/?": {
      helpLine('<span class="glyph">&lt;?&gt;</span>');
      sys("/connect              peer code (1:1, single-use)");
      sys("/room                 room code (reusable while open)");
      sys("/join <code>          peer or room");
      sys("/who                  connections");
      sys("/go <n|name>          switch conversation");
      sys("/name <name>          local label for current");
      sys("/hist on|off          local history");
      sys("/wipe                 then /wipe confirm within 10s");
      sys("/leave                leave current (keeps history)");
      sys("/file [path]          send file");
      sys("/demo [1|2]           scripted recording run");
      sys("/help  /?             this list");
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
      const code = await invoke("start_pairing");
      await refreshLists();
      try {
        await navigator.clipboard.writeText(code);
        sys("copied");
      } catch {
        /* ignore */
      }
      break;
    }
    case "/room": {
      const [code, topic] = await invoke("start_room");
      await refreshLists();
      try {
        await navigator.clipboard.writeText(code);
        sys("copied");
      } catch {
        /* ignore */
      }
      await openConv(topic, "room", short(topic), "WAITING");
      break;
    }
    case "/join": {
      if (!args[0]) {
        sys("usage: /join <code>");
        break;
      }
      const code = args[0];
      let info;
      try {
        info = await invoke("inspect_code", { code });
      } catch (e) {
        sys(String(e));
        break;
      }
      if (info.kind === "room") {
        sys(
          info.reachable
            ? `joining room - ${info.members ?? "?"} present`
            : "joining room - host not reached yet"
        );
      } else {
        sys("connecting to peer");
      }
      try {
        const result = await invoke("join_code", { code });
        await refreshLists();
        if (result.kind === "peer") {
          const p = result.peer;
          await openConv(p.endpoint_id, "peer", p.label || short(p.endpoint_id), p.path);
        } else {
          const r = result.room;
          await openConv(
            r.topic_id,
            "room",
            r.label,
            (r.members || []).length ? "LIVE" : "WAITING"
          );
        }
      } catch (e) {
        sys(String(e));
      }
      break;
    }
    case "/who": {
      const idx = buildIndex();
      if (!idx.length) {
        sys("no connections");
        break;
      }
      idx.forEach((c, i) => {
        const extra =
          c.kind === "room" ? ` · ${c.members ?? 0}` : "";
        sys(`${i + 1}. [${c.kind}] ${c.label}  ${c.path}${extra}  ${short(c.id)}`);
      });
      break;
    }
    case "/go": {
      if (!args[0]) {
        sys("usage: /go <n|name>");
        break;
      }
      const t = resolveTarget(args[0]);
      if (!t) {
        sys("not found - /who");
        break;
      }
      await openConv(t.id, t.kind, t.label, t.path);
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
      sys(`named ${rest} (local only)`);
      break;
    }
    case "/hist": {
      const mode = (args[0] || "").toLowerCase();
      if (mode !== "on" && mode !== "off") {
        const s = await invoke("get_status");
        sys(`history: ${s.history_enabled ? "on" : "off"} - saved locally, encrypted.`);
        break;
      }
      const on = mode === "on";
      await invoke("set_history", { enabled: on });
      sys(
        on
          ? "history: on - saved locally, encrypted."
          : "history: off - new messages not kept."
      );
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
        sys("local history cleared (this device only)");
        break;
      }
      state.wipeDeadline = Date.now() + 10000;
      sys("clears local copy only - peers keep theirs");
      sys("confirm: /wipe confirm  (10s)");
      break;
    }
    case "/leave": {
      if (!state.active) {
        sys("no conversation");
        break;
      }
      const id = state.active;
      await invoke("leave_conversation", { conversationId: id });
      sys(`left ${state.activeLabel || short(id)}`);
      state.active = null;
      state.activeKind = null;
      state.activeLabel = null;
      updateStatusBar();
      setPath("-", false);
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
        sys(String(e));
      }
      break;
    }
    default:
      sys(`unknown command - /help`);
  }
}

async function onSubmit() {
  const el = $("input");
  const raw = el.value;
  if (!raw.trim()) return;
  el.value = "";
  autoSize();

  if (raw.trimStart().startsWith("/")) {
    sys(`> ${raw.trim()}`);
    try {
      await runCommand(raw.trim());
    } catch (e) {
      sys(String(e));
    }
    return;
  }

  if (!state.active) {
    sys("no conversation - /connect /join /room /go");
    return;
  }
  try {
    await invoke("send_text", { conversationId: state.active, body: raw });
    invoke("send_typing", { conversationId: state.active, active: false });
  } catch (e) {
    sys(String(e));
  }
}

function autoSize() {
  const el = $("input");
  el.style.height = "auto";
  el.style.height = Math.min(el.scrollHeight, 120) + "px";
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

/** Demo conversation lines. */
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
  await openConv(peer.endpoint_id, "peer", "wire", peer.path);
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
      // wait for incoming
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
    await openConv(p.endpoint_id, "peer", "wire", p.path);
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
  await openConv(peer.endpoint_id, "peer", short(peer.endpoint_id), peer.path);
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
    await openConv(p.endpoint_id, "peer", short(p.endpoint_id), p.path);
  }
  // reply to ping
  await waitUntil(async () => {
    // loosely wait for a message event via short sleep loop
    return !!state.active;
  });
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
  mark.innerHTML = '&lt;?&gt;<span class="block-cursor"></span>';
  log.appendChild(mark);
  log.scrollTop = log.scrollHeight;
}

async function boot() {
  showIdle();
  await refreshLists();

  await listen("message", (ev) => {
    const m = ev.payload;
    if (m.conversation_id !== state.active) return;
    appendMsg(m.body, m.outgoing ? "out" : "in", !m.outgoing, m);
  });

  await listen("peer_update", () => refreshLists());
  await listen("presence", () => refreshLists());

  await listen("paired", async (ev) => {
    if (state.demoRunning) {
      await refreshLists();
      return;
    }
    await refreshLists();
    await openConv(
      ev.payload.endpoint_id,
      "peer",
      ev.payload.label || short(ev.payload.endpoint_id),
      ev.payload.path || "..."
    );
  });

  await listen("conn_path", (ev) => {
    if (ev.payload.conversation_id === state.active) {
      setPath(ev.payload.path, true);
    }
    refreshLists();
  });

  await listen("typing", (ev) => {
    if (ev.payload.conversation_id !== state.active) return;
    $("typing").textContent = ev.payload.active ? "..." : "";
  });

  await listen("pairing_expired", () => {
    sys("code expired");
    clearInterval(state.codeTimer);
    state.codeTimer = null;
    state.lastCode = null;
  });

  await listen("file_progress", (ev) => {
    if (ev.payload.conversation_id !== state.active) return;
    if (ev.payload.pct >= 100) return;
    sys(`file ${ev.payload.pct}% · ${ev.payload.phase}`);
  });

  $("input").addEventListener("input", () => {
    autoSize();
    if (!state.active) return;
    if ($("input").value.startsWith("/")) return;
    invoke("send_typing", { conversationId: state.active, active: true });
  });

  $("input").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      onSubmit();
    }
  });

  $("composer").onsubmit = (e) => {
    e.preventDefault();
    onSubmit();
  };

  $("input").focus();
  setInterval(refreshLists, 3000);

  // Auto-start CLI demo
  try {
    const cfg = await invoke("get_launch_config");
    if (cfg.demo) {
      await sleep(800);
      await runDemo(cfg.demo, cfg.role === "joiner" ? "joiner" : "host");
    }
  } catch {
    /* ignore */
  }
}

boot().catch((e) => {
  document.body.textContent = String(e);
});
