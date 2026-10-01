# Ratline

Peer-to-peer encrypted chat. No accounts, no server, no cloud.

<img src="assets/demo-conversation.gif" width="640" alt="conversation demo" />

<img src="assets/demo-commands.gif" width="640" alt="commands demo" />

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

**Summon:** `Ctrl+\`` (configurable with `/hotkey`) slides the window in from the top. Close or `×` hides to the tray — it keeps running.

**1:1 (dm):** `/connect` gives a `P-` code (single use, ~10 min). The other person runs `/join <code>`. After you've paired once, `/connect <name>` reconnects without a new code.

**Group (room):** `/room` gives an `R-` code (reuse while the room stays open). Others `/join` the same code.

**Fingerprint:** status bar shows `F-····`. Read it aloud / text it separately from the pairing code to confirm identity. `/fp` and `/fp <name>`.

**Dead drop:** `/seal <peer> <message>` (or `/seal <peer> @ <file>`) writes a sealed envelope file only that peer can open. Hand the file off yourself (USB, email, whatever). They `/drop <path>`. This is not offline messaging — it's a sealed envelope, not a delivery system.

```
/connect [name]       P- code, or reconnect known peer by local name
/room                 R- code for a group
/join <P-|R-code>     enter someone else's code
/go <n|name>          switch chat (bare /go lists like /who)
/who                  list dm/group + path
/clear                clear the screen only
/name <name>          local label
/hist on|off          local encrypted history
/wipe                 erase one conversation history (confirm within 10s)
/burn                 destroy identity + all local data (confirm within 15s)
/leave                leave current chat
/file [path]          send a file
/seal <peer> <msg>    sealed envelope for a known peer
/seal <peer> @ <file> sealed file envelope
/drop <path>          open a sealed envelope addressed to you
/fp [name]            fingerprint (yours, or a known peer)
/sound on|off         soft tones (off by default)
/hotkey [chord]       global summon hotkey (default ctrl+grave)
/update               check GitHub and install latest
/version              show build
/demo [1|2]           scripted demo
/help  /?             commands (/help more for the rest)
```

Paste a `P-`/`R-` code into an empty input — it asks `y/n` before joining. Tab completes commands. Up/Down cycles prior lines. Esc clears the line.

Status bar: fingerprint, `dm:`/`group:`, path (direct / relayed / waiting), and a countdown while a `P-` invite is live.

In 1:1 chats, `>` is you and `<` is them. In groups with more than two people, lines use `<name>` instead.

## Privacy

- Direct links show your IP to the peer.
- If NAT blocks a direct path, traffic can go through a public relay. Relays see connection metadata, not message content.
- History (if on) stays on your machine, encrypted at rest.
- Dead drops are files you move yourself. Ratline does not deliver them.

Uses [iroh](https://github.com/n0-computer/iroh) and [Tauri](https://tauri.app).

## License

MIT
