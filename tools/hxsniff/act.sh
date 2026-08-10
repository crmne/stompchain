#!/bin/bash
# act.sh "<label>" <x> <y> [clicks] - annotate the capture, then click.
#
# Coordinates are given in the coordinate space of the 1400px-wide window
# screenshots produced by shot.sh, and converted here against the window's
# live position so the numbers stay valid if the window is moved.
set -euo pipefail

MARK="${HXSNIFF_MARK:-$HOME/.cache/hxsniff/hxsniff.mark}"
HERE="$(cd "$(dirname "$0")" && pwd)"
SHOT_W=1400

label="$1"; ix="$2"; iy="$3"; clicks="${4:-1}"

read -r wx wy ww wh < <(osascript -e 'tell application "System Events" to tell process "HX Edit" to get {position, size} of window 1' \
    | tr -d ' ' | tr ',' ' ')

scale=$(python3 -c "print($ww/$SHOT_W)")
sx=$(python3 -c "print(round($wx + $ix*$scale))")
sy=$(python3 -c "print(round($wy + $iy*$scale))")

echo "$label" >>"$MARK"
"$HERE/click" "$sx" "$sy" "$clicks"
sleep "${SETTLE:-1.2}"
echo "clicked '$label' at image($ix,$iy) -> screen($sx,$sy)"
