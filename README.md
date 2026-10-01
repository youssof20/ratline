# Ratline

Peer-to-peer encrypted chat. No accounts, no server, no cloud.

<img src="assets/demo-conversation.gif" width="640" alt="conversation demo" />

<img src="assets/demo-commands.gif" width="640" alt="commands demo" />

Both people need to be online at the same time. Nothing is queued for later.

## Facts

- No phone number, email, or account. Identity is a local keypair on disk.
- No app server. Nothing is collected on our side.
- Source is open. Read it yourself.

Not Signal or Telegram. Those keep messages on servers. This one does not.

## Download

[Releases](https://github.com/youssof20/ratline/releases)

- Windows: `Ratline_*_x64-setup.exe` or `.msi`
- macOS: `.dmg`
- Linux: `.AppImage` or `.deb`

Checksums: `SHA256SUMS` in each release. `/update` verifies them before install.

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

```bash
ratline --version
```

Recording demos (not listed in-app): `ratline --demo conversation` / `ratline --demo commands`.

## Usage

One input line. Enter sends; Shift+Enter is a newline. `Ctrl+\`` summons the window (tray when hidden).

**1:1:** `/connect` → share the `P-` code → they `/join` it. Later: `/connect <name>`.

**Group:** `/room` → share the `R-` code → others `/join`.

**Verify:** status shows fingerprint `F-····`. Read it out of band. `/fp` / `/fp <name>`.

```
/connect [name]   pair or reconnect
/join <code>      enter theirs
/room             group code
/go · /who        switch / list
/leave · /clear   forget chat / clear screen
/name · /fp       label / fingerprint
/file             send while both online
/help more        seal, drop, hist, wipe, burn, …
```

Paste a code → `y/n` before join. Tab completes. Up/Down recalls lines.

## Privacy

- Direct links show your IP to the peer. Relays see metadata, not content.
- Optional local history uses a key derived from your identity file. Disk access on this machine means full compromise — not “safe if the drive is stolen.”
- `/seal` / `/drop` are sealed envelopes you hand off yourself. Not offline messaging. Claimed sender is not a signature.

## License

MIT
