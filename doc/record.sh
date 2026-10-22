#!/bin/sh
# Records the demo GIF of the README in Ghostty, with its real thumbnails (fake chats, see
# examples/demo.rs).
#
#   doc/record.sh                 opens the demo in a Ghostty window
#   doc/record.sh VIDEO [START [END]]   makes doc/demo.gif from the recording, from START to
#                                       END seconds
#
# 1. Run `doc/record.sh`: a Ghostty window opens on the demo.
# 2. Record that window (GNOME: Print, then the camera icon, select the window; macOS:
#    Cmd+Shift+5), then press Space in it: the scenario of doc/play.py plays, and quits.
# 3. Stop the recording, and run `doc/record.sh ~/Videos/Screencasts/<the recording>.webm`,
#    with the second at which you pressed Space to cut what came before, and the second
#    before the window closes to cut what comes after.
set -e
cd "$(dirname "$0")/.."

if [ $# -eq 0 ]; then
    cargo build --example demo
    exec ghostty --gtk-single-instance=false --title="gmessages-tui" \
        --window-width=110 --window-height=32 --font-size=13 \
        -e python3 doc/play.py target/debug/examples/demo
fi

ffmpeg -y -ss "${2:-0}" ${3:+-to "$3"} -i "$1" -vf \
    "fps=12,scale='min(1200,iw)':-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
    doc/demo.gif
ls -l doc/demo.gif
