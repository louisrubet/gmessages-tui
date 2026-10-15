# gmessages-tui

A terminal client for Google Messages (RCS and SMS), paired with your Android phone like Google Messages for web. Built on [chat-tui](https://github.com/louisrubet/chat-tui), the interface of [signal-tui](https://github.com/louisrubet/signal-tui).

**Not affiliated with Google.** Google Messages is a trademark of Google LLC.

**Status:** early. Pairs with the phone and lists the conversations; no messages yet.

## Install

Needs Rust ≥ 1.89, nothing else.

```sh
cargo install --locked --git https://github.com/louisrubet/gmessages-tui
```

## Run

```sh
gmessages-tui         # or `cargo run` from a checkout
```

First run pairs with your phone, like *Sign in with Google* on Google Messages for web:

1. In a browser (a private window is best), sign in to the phone's Google account at [this link](https://accounts.google.com/AccountChooser?continue=https://messages.google.com/web/config).
2. Developer tools (F12) › Network › reload › right-click the `config` request › *Copy as cURL*.
3. Paste it into gmessages-tui, then tap the emoji it shows on your phone.
4. Close the browser window **without signing out** (signing out ends the session).

Only the Google cookies the protocol needs are kept, in the session. `gmessages-tui --qr` pairs by QR code instead (*Device pairing › QR code scanner*), if your phone offers it.

The phone stays the relay: keep it online.

Keys and terminals: see [signal-tui](https://github.com/louisrubet/signal-tui#keys).

## How it works

Google Messages has no public API. The protocol is spoken by [libgm](https://github.com/mautrix/gmessages/tree/main/pkg/libgm) (Go, from mautrix-gmessages), wrapped in **gmd** ([gmd/](gmd/)), a small program speaking JSON lines on stdin/stdout.

gmd is built ahead for Linux, macOS and Windows (x86-64, ARM64) and published as a [release](https://github.com/louisrubet/gmessages-tui/releases). At build time, `build.rs` downloads the one for the target, checks its SHA-256 against [gmd/release.txt](gmd/release.txt) and embeds it. At run time it is written to the cache directory (`~/.cache/gmessages-tui/`) and started. One executable, no Go needed.

- **State**: `~/.local/share/gmessages-tui/`: the session (`session.json`, readable by you only: it gives access to your messages) and gmd's log.
- **Offline build**: build gmd yourself (`cd gmd && CGO_ENABLED=0 go build`), then `GMD_PATH=gmd/gmd cargo build`.
- **Reproducible**: `gmd/build.sh <tag>` rebuilds every target; the official Go release named in `gmd/go.mod` gives the same hashes (distribution builds of Go may differ).

## Dev

```sh
GMD_PATH=… cargo run         # with a local gmd
gmd/build.sh gmd-vX.Y.Z      # all targets into gmd/dist/
git tag gmd-vX.Y.Z && git push origin gmd-vX.Y.Z   # CI builds and publishes them
gh release download gmd-vX.Y.Z -p release.txt -O gmd/release.txt --clobber   # then commit it
```

## License

[AGPL-3.0-only](LICENSE), like libgm.
