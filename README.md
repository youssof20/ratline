# Ratline

Peer-to-peer encrypted chat. No accounts, no server, no cloud.

https://github.com/youssof20/ratline/raw/main/assets/demo-conversation.mp4

https://github.com/youssof20/ratline/raw/main/assets/demo-commands.mp4

Both sides must be online at the same time. This is a live line, not an inbox.

## Why

- No phone number, email, or account. Identity is a local keypair.
- No application server. Nothing to subpoena, because nothing is collected.
- Fully auditable: client and protocol stack are open source end to end.

Not a Signal or Telegram replacement. Those store and forward. Ratline does not.

## Download

Get the latest build from [Releases](https://github.com/youssof20/ratline/releases):

- Windows: `Ratline_*_x64-setup.exe` (or `.msi`)
- macOS: `.dmg`
- Linux: `.AppImage` / `.deb`

SHA-256 checksums ship with each release (`SHA256SUMS`).

## Build from source

```bash
git clone https://github.com/youssof20/ratline
cd ratline
npm install
npm run build
```

Needs Rust stable, a C/C++ toolchain, Node.js, and WebView2 on Windows.

```bash
npm run dev
```

Demo mode (two real local peers, for recording):

```bash
ratline --demo conversation
ratline --demo commands
```

Or inside the app: `/demo 1` / `/demo 2`.

```bash
ratline --version
```

## Usage

One input line. Text without `/` is a message (Enter sends, Shift+Enter newline).

```
/connect              peer code (1:1, single-use, ~10 min)
/room                 room code (reusable while open)
/join <code>          peer (P-…) or room (R-…)
/who                  connections + DIRECT/RELAYED
/go <n|name>          switch conversation
/name <name>          local label only
/hist on|off          local encrypted history
/wipe                 then /wipe confirm within 10s (this device only)
/leave                leave current (keeps history)
/file [path]          send a file
/demo [1|2]           scripted recording run
/help  /?             commands
```

Connection path shows as DIRECT or RELAYED in the status bar.

## Privacy

- Direct links expose your IP to the peer. Not an anonymity tool.
- If NAT blocks a direct path, traffic falls back to a public relay. Relays see connection metadata, not message content.
- History, when enabled, stays on your machine and is encrypted at rest.

Built on [iroh](https://github.com/n0-computer/iroh) and [Tauri](https://tauri.app).

## License

MIT
