# gmessages-tui

A terminal client for Google Messages (RCS and SMS), paired with your Android phone like Google Messages for web. Built on [chat-tui](https://github.com/louisrubet/chat-tui), the interface of [signal-tui](https://github.com/louisrubet/signal-tui).

**Not affiliated with Google.** Google Messages is a trademark of Google LLC.

**Status:** early. Pairs with the phone, shows the conversations, their last 30 messages and their images; no sending yet.

## Install

Needs Rust ≥ 1.89. The default build includes a private Google sign-in window
using [wry](https://github.com/tauri-apps/wry). On Linux, it needs GTK 3 and
WebKitGTK 4.1 development packages to build (Fedora:
`sudo dnf install gtk3-devel webkit2gtk4.1-devel`). A graphical desktop session
is required only when opening the sign-in window.

For a headless build without the native sign-in dependencies, add
`--no-default-features` to the installation command; it uses manual pairing.

```sh
cargo install --locked --git https://github.com/louisrubet/gmessages-tui
```

## Run

```sh
gmessages-tui         # or `cargo run` from a checkout
```

First run opens a private Google sign-in window, based on
[`examples/wry_signin.rs`](examples/wry_signin.rs):

1. Sign in to the Google account used by Google Messages on your phone.
2. gmessages-tui automatically reads the authentication cookies, hides the window,
   and asks the phone to pair. No developer tools, cURL command or manual paste is needed.
3. Tap the emoji shown in the terminal on your phone.
4. Once the conversations are ready, the hidden browser process is stopped and
   the TUI starts. Ctrl+C in the terminal cancels pairing; closing the window
   before it hides also cancels.

The window runs in a subprocess so its graphical event loop does not block the
terminal. Only the cookies required by libgm are transferred over a private pipe;
this path does not print their values, save a browser profile, or export temporary
credential files. The existing gmd session stores the credentials needed to reconnect.
SOCS is not required. If a saved session is still valid, no sign-in window opens.
If the session expires during startup, gmessages-tui opens a new sign-in window.
Google can reject embedded browsers; successful sign-in on one system does not
guarantee it on every platform.

### Reset the local login

```sh
gmessages-tui --reset-login
# From a checkout:
cargo run -- --reset-login
```

This removes only the local `session.json` before starting a fresh pairing.
Preferences, read marks and logs are preserved. It is safe if no session exists;
other deletion errors stop startup rather than silently reusing the old login.
Close other running instances first so they cannot write their session back.
This does not sign you out of Google elsewhere or revoke the old paired device
on your phone; remove that device in Google Messages if needed.

The option can also be combined with `--manual` or `--qr`, in either order.

### Manual and QR fallback

Use `gmessages-tui --manual` (or `cargo run -- --manual`) if the native window is
unavailable or refused by Google. It prints a link to a pairing page, served on
this computer only:

1. Sign in to the phone's Google account from it, in a **Firefox private window**.
   Chrome may bind its Google cookies to the browser (DBSC), preventing their reuse.
   Go nowhere else in that window.
2. Developer tools (F12) › Network › reload › right-click the `config` request › *Copy as cURL*.
3. Paste it into the page (or the terminal), then tap the emoji shown on your phone.
4. Close the Google window **without signing out** (signing out ends the session).

A build with `--no-default-features` uses this manual flow by default.
`gmessages-tui --qr` pairs by QR code instead, for phones that still offer it
(Google is removing it).

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
