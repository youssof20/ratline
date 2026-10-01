# Ratline

A live private line. Both of you online. No accounts, no cloud inbox, nothing queued for later.

<img src="assets/demo-conversation.gif" width="640" alt="conversation demo" />

<img src="assets/demo-commands.gif" width="640" alt="commands demo" />

Not a messenger. A walkie-talkie with crypto — you open the line, talk, then it goes quiet.

## Facts

- No phone number, email, or account. Identity is a local keypair on your machine.
- No app server. Nothing is collected on our side.
- Both people must be online at the same time. That is the product, not a bug.
- Source is open. Read it yourself.

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

## Usage

One input line. Enter sends; Shift+Enter is a newline. `Ctrl+\`` summons the window (hides to tray).

**Open a line:** `/connect` → share the `P-` code → they `/join` it.

**Verify:** after pair, fingerprints print. Read them aloud (or text separately from the code). Then `/verify`.

**Later:** `/connect <name>` reconnects a known peer — no new code.

```
/connect [name]   open a line / reconnect
/join <code>      enter theirs
/verify           confirm fingerprint out of band
/go · /who        switch / list
/leave · /clear   forget chat / clear screen
/name · /fp       label / show fingerprint
/file             send while both online
/help more        groups, seal, hist, …
```

Paste a code → `y/n` before join. Tab completes. Up/Down recalls lines.

## Privacy

- Direct links show your IP to the peer. Relays see connection metadata, not message content.
- Optional local history uses a key derived from your identity file. Disk access on this machine means full compromise.
- `/seal` / `/drop` (under `/help more`) are sealed envelopes you hand off yourself — not offline messaging.

## License

MIT
