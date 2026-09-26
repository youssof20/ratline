# Ratline

Peer-to-peer encrypted chat. No accounts, no server, no cloud.
Two people run the app, share a short code, and talk over a direct link.

Both sides must be online at the same time — this is a live line, not an inbox.

## Why this exists

- No phone number, email, or account. Identity is a local keypair.
- No application server — nothing to subpoena, because nothing is collected.
- Fully auditable: client and protocol stack are open source end to end.

It does not try to replace Signal or Telegram. Those store and forward. Ratline does not.

## Install

```bash
git clone https://github.com/yourname/ratline
cd ratline
npm install
npm run build
```

Artifacts: `src-tauri/target/release/bundle/`

Needs: Rust stable, a C/C++ toolchain, Node.js, WebView2 on Windows.

```bash
npm run dev
```

## Usage

One input line. Text without `/` is a message (Enter sends, Shift+Enter newline).
Commands:

```
/connect              peer code (1:1, single-use, ~10 min)
/room                 room code (reusable while the room is open)
/join <code>          peer (P-…) or room (R-…)
/who                  list connections + DIRECT/RELAYED
/go <n|name>          switch conversation
/name <name>          local label only
/hist on|off          local encrypted history
/wipe                 then /wipe confirm within 10s (this device only)
/leave                leave current (keeps history)
/file [path]          send a file
/help  /?             commands
```

Connection path shows as DIRECT or RELAYED in the status bar.

## Privacy

- Direct links expose your IP to the peer, like a phone call exposes a number. Not an anonymity tool.
- If NAT blocks a direct path, traffic falls back to a public relay. Relays see connection metadata, not message content (QUIC/TLS end-to-end).
- History, when enabled, stays on your machine and is encrypted at rest.

Built on [iroh](https://github.com/n0-computer/iroh) and [Tauri](https://tauri.app).

## License

MIT
