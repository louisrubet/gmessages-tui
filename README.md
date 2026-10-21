# gmessages-tui

A Google Messages client (RCS and SMS) for the terminal, paired with your Android phone like Google Messages for web. Built on [libgm](https://github.com/mautrix/gmessages/tree/main/pkg/libgm) and [chat-tui](https://github.com/louisrubet/chat-tui), the interface it shares with [signal-tui](https://github.com/louisrubet/signal-tui).

**Not affiliated with Google.** Google Messages is a trademark of Google LLC.

- **Images** shown as thumbnails, pasted with `Ctrl+V`, opened full size with `o`
- **History**: past conversations and their last messages, kept in an encrypted store
- **Live**: new messages, typing, desktop notifications
- **Replies, forwards**, reactions shown, fuzzy `:shortcode` emoji picker (fzf style)
- **Pinned chats**, unread counts

## Install

Needs Rust ≥ 1.89 and the OpenSSL development package (Fedora: `openssl-devel`, Debian/Ubuntu: `libssl-dev`). On Linux, the sign-in window also needs GTK 3 and WebKitGTK 4.1 (Fedora: `gtk3-devel webkit2gtk4.1-devel`, Debian/Ubuntu: `libgtk-3-dev libwebkit2gtk-4.1-dev`).

```sh
cargo install --locked --git https://github.com/louisrubet/gmessages-tui
```

or, from a checkout, `cargo install --locked --path .`. It installs `gmessages-tui` in `~/.cargo/bin` (make sure it is in your `PATH`). Add `--no-default-features` for a headless build without the sign-in window (manual pairing only).

### Building gmd yourself

The build downloads gmd, already built for Linux, macOS and Windows (x86-64, ARM64). For another platform, or offline, build it with Go (version in [gmd/go.mod](gmd/go.mod)) and point `GMD_PATH` to it:

```sh
git clone https://github.com/louisrubet/gmessages-tui && cd gmessages-tui
(cd gmd && CGO_ENABLED=0 go build)
GMD_PATH=gmd/gmd cargo install --locked --path .
```

## Run

```sh
gmessages-tui         # or `cargo run` from a checkout
```

First run: sign in to the phone's Google account in the window that opens, then tap the emoji shown in the terminal on your phone. The phone stays the relay: keep it online.

`gmessages-tui --reset-login` forgets the pairing and starts over. Also remove the old device from your phone (*Device pairing*).

`gmessages-tui --manual` pairs from a page in your browser, when the window is unavailable or refused by Google (Firefox private window, Developer tools › Network › `config` › *Copy as cURL*, paste). `gmessages-tui --qr` pairs by QR code, for phones that still offer it.

`gmessages-tui --version` prints the release version, `gmessages-tui --help` the options.

## Uninstall

```sh
gmessages-tui --uninstall        # store key in the keyring, ~/.local/share/gmessages-tui, ~/.cache/gmessages-tui
cargo uninstall gmessages-tui    # the program
```

`--uninstall` asks for confirmation. Also remove the device from your phone (*Device pairing*).

## Terminal

Any terminal works. For thumbnails, use one that speaks the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) **with Unicode placeholders**:

| Terminal | Linux | macOS | Windows |
|---|:-:|:-:|:-:|
| [Ghostty](https://ghostty.org) (recommended) | ✓ | ✓ | |
| [kitty](https://sw.kovidgoyal.net/kitty/) | ✓ | ✓ | |
| [Rio](https://rioterm.com) | ✓ | ✓ | ✓ |

On Windows, Rio is the only native option; kitty also runs under WSL with WSLg.

Elsewhere (WezTerm, Konsole, iTerm2, Warp, Windows Terminal, Alacritty, inside tmux…) everything works but graphics are reduced: images show as `[image #1]` and open full size with `o`.

## Keys

| Key | Action |
|---|---|
| `↑` `↓` | select chat / message (in a chat, `↑` starts selecting) |
| `Enter` | open chat, new message, send |
| `r` | reply to the selected message |
| `f` | forward the selected message |
| `c` | copy the selected message (OSC 52) |
| `o` | open the links and images of the selected message |
| `Ctrl+V` | paste the image in the clipboard |
| `P` | pin / unpin the selected chat |
| `a` | archive the selected chat (from the chat list) |
| `Del` | delete the selected chat: to the trash, for good from there (confirmed) |
| `r` | move the selected chat back to the inbox (from a folder) |
| `p` | parameters (from the chat list) |
| `Esc` | unselect, back, give up a stuck send |
| `q` | quit |

Sending reactions (`e`) and deleting a message (`Del` in a chat) are not implemented yet.

## Good to know

- **History**: at start, the interface opens on the store once connected (or after 10 s, to read offline). Only conversations with new messages are fetched again, their last 30 messages.
- **Folders**: *Archive*, *Spam & blocked* and *Trash* show under the chats when not empty; `Enter` opens one, `Esc` goes back.
- **Phone sync**: conversations archived, trashed, in spam or blocked on the phone move to their folder, those deleted for good go (with their messages here); renames show at once; reading a chat here marks it read on the phone, and the other way round. `Del` sends a chat to the trash, as on the phone; only from *Trash* does it delete for good.
- **Downloads**: images are fetched when they arrive or when a chat is opened; videos and files wait for `o`. Downloaded media are stored.
- **Formatting**: RCS and SMS have none; `**bold**` and the like are sent as plain text.
- **②** counts the messages received since you last opened a chat; opening it shows a "2 new messages" line above them.
- **Device name**: the phone lists this client as *gmessages-tui*.
- **State**: `~/.local/share/gmessages-tui/`. See [Security](#security).
- **Interface**: [chat-tui](https://github.com/louisrubet/chat-tui), pinned by tag in `Cargo.toml`; its demo with fake chats is `cargo run --example mock_tui` there.

## Security

The state holds the session (`session.json`: it gives access to your messages through the phone), the store and gmd's log.

- **Encrypted at rest**: the store (`messages.db`: conversations, messages, media) uses SQLCipher.
- **The key** is 32 random bytes generated on the first start, kept in the OS keyring: Secret Service on Linux (GNOME Keyring, KWallet), Keychain on macOS, Credential Manager on Windows, under the service `gmessages-tui`.
  To see it on Linux: `secret-tool search service gmessages-tui`.
- **No keyring** (headless Linux, SSH): a passphrase is asked at startup instead, or taken from `GMESSAGES_TUI_PASSPHRASE`.
- **Files** are created readable by you only (Unix).
- **Sign-in**: the window is private and saves no browser profile; once paired, the session is all that is kept.
- **Reset**: `gmessages-tui --reset-login` removes the session, then pairs again. Also remove the old device from your phone: that is what revokes it.
- **Uninstall**: `gmessages-tui --uninstall` deletes the keyring entry first, then the store and the rest of the state.
- **Limits**: a program running in your session can read the unlocked keyring. Full disk encryption protects the rest of your machine.

## How it works

Google Messages has no public API. Its protocol is spoken by libgm (Go, from mautrix-gmessages), wrapped in **gmd** ([gmd/](gmd/)), a small program speaking JSON lines on stdin/stdout.

gmd is built ahead for Linux, macOS and Windows (x86-64, ARM64) and published as a [release](https://github.com/louisrubet/gmessages-tui/releases). At build time, `build.rs` downloads the one for the target, checks its SHA-256 against [gmd/release.txt](gmd/release.txt) and embeds it. At run time it is written to `~/.cache/gmessages-tui/` and started. One executable, no Go needed.

- **Offline or other platform**: see [Building gmd yourself](#building-gmd-yourself).
- **Reproducible**: `gmd/build.sh <tag>` rebuilds every target; the official Go release named in `gmd/go.mod` gives the same hashes.

## Dev

```sh
cargo run --example wry_signin --features wry-signin   # does Google accept the webview?
GMD_PATH=… cargo run                                   # with a local gmd
cargo test
gmd/build.sh gmd-vX.Y.Z                                # all targets into gmd/dist/
git tag gmd-vX.Y.Z && git push origin gmd-vX.Y.Z       # CI builds and publishes them
gh release download gmd-vX.Y.Z -p release.txt -O gmd/release.txt --clobber   # then commit it
```

## License

[AGPL-3.0-only](LICENSE), like [libgm](https://github.com/mautrix/gmessages) it is built on.
