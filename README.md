# Ratline

Peer-to-peer encrypted chat. No accounts, no server, no cloud.

![conversation demo](assets/demo-conversation.gif)

![commands demo](assets/demo-commands.gif)

Both people need to be online at the same time. Nothing is queued for later.

## Facts

- No phone number, email, or account. Identity is a local keypair.
- No app server. Nothing is collected on our side.
- Source is open. Read it yourself.

Not Signal or Telegram. Those keep messages on servers. This one does not.

## Download

[Releases](https://github.com/youssof20/ratline/releases)

- Windows: `Ratline_*_x64-setup.exe` or `.msi`
- macOS: `.dmg`
- Linux: `.AppImage` or `.deb`

Checksums: `SHA256SUMS` in each release.

## Build from source

```bash
git clone https://github.com/youssof20/ratline
cd ratline
npm install
npm run build
```

Needs Rust, a C/C++ toolchain, Node.js, and WebView2 on Windows.

```bash
npm run dev
```

Two-peer demo (for recordings):

```bash
ratline --demo conversation
ratline --demo commands
```

Or in the app: `/demo 1` / `/demo 2`.

```bash
ratline --version
```

## Usage

One input line. Lines without `/` are chat messages (Enter sends, Shift+Enter is a newline).

```
/connect              peer code (1:1, one use, ~10 min)
/room                 room code (reuse while open)
/join <code>          peer (P-...) or room (R-...)
/who                  connections + DIRECT/RELAYED
/go <n|name>          switch conversation
/name <name>          local label
/hist on|off          local encrypted history
/wipe                 then /wipe confirm within 10s (this device only)
/leave                leave current (keeps history)
/file [path]          send a file
/demo [1|2]           scripted demo
/help  /?             commands
```

Status bar shows DIRECT or RELAYED.

## Privacy

- Direct links show your IP to the peer.
- If NAT blocks a direct path, traffic can go through a public relay. Relays see connection metadata, not message content.
- History (if on) stays on your machine, encrypted at rest.

Uses [iroh](https://github.com/n0-computer/iroh) and [Tauri](https://tauri.app).

## License

MIT
