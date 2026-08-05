#!/bin/bash
# shot.sh <name> — capture the HX Edit window, downscaled to 1400px wide.
set -euo pipefail
name="${1:-shot}"
out="$HOME/.cache/hxsniff/shots/$name.png"
mkdir -p $HOME/.cache/hxsniff/shots

read -r wx wy ww wh < <(osascript -e 'tell application "System Events" to tell process "HX Edit" to get {position, size} of window 1' \
    | tr -d ' ' | tr ',' ' ')

screencapture -x -o -R"$wx,$wy,$ww,$wh" "$HOME/.cache/hxsniff/shots/rawshot.png"
sips -Z 1400 "$HOME/.cache/hxsniff/shots/rawshot.png" --out "$out" >/dev/null
rm -f "$HOME/.cache/hxsniff/shots/rawshot.png"
echo "$out"
