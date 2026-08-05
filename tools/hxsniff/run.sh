#!/bin/bash
# Build the interposer and launch an instrumented copy of HX Edit under it.
#
# The shipped app is signed with the hardened runtime and library validation,
# both of which make dyld ignore DYLD_INSERT_LIBRARIES. We therefore work on a
# *copy* — the installed app is never touched — strip its signature, and re-sign
# it ad-hoc, which drops the runtime flag and re-enables insertion.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
SRC_APP="${SRC_APP:-/Applications/Line6/HX Edit.app}"
WORK="${WORK:-$HOME/.cache/hxsniff}"
APP="$WORK/HX Edit.app"
LOG="${HXSNIFF_LOG:-$WORK/hxsniff.log}"
MARK="${HXSNIFF_MARK:-$WORK/hxsniff.mark}"

mkdir -p "$WORK"

echo "==> building hxsniff.dylib"
clang -arch arm64 -O2 -dynamiclib \
    -I/opt/homebrew/include \
    -o "$WORK/hxsniff.dylib" "$HERE/hxsniff.c" \
    "$SRC_APP/Contents/MacOS/libusb-1.0.0.dylib"

# We link against the *bundled* libusb so our calls to the real functions bind
# to the very same image HX Edit loads rather than a second Homebrew copy — two
# live libusb contexts over one device would not end well. The bundled dylib's
# install name is the bare "libusb-1.0.0.dylib", but HX Edit loads it as
# "@executable_path/../MacOS/libusb-1.0.0.dylib"; dyld keys images by that
# string, so we must match it exactly or we get two copies.
install_name_tool -change libusb-1.0.0.dylib \
    @executable_path/../MacOS/libusb-1.0.0.dylib "$WORK/hxsniff.dylib"
otool -L "$WORK/hxsniff.dylib" | grep -q '@executable_path/../MacOS/libusb' \
    || { echo "!! hxsniff.dylib is not bound to the bundled libusb"; exit 1; }

if [ ! -d "$APP" ] || [ "${REFRESH_APP:-0}" = 1 ]; then
    echo "==> copying app bundle (95M, one time)"
    rm -rf "$APP"
    ditto "$SRC_APP" "$APP"

    echo "==> re-signing ad-hoc without hardened runtime"
    codesign --remove-signature "$APP/Contents/MacOS/libusb-1.0.0.dylib" || true
    codesign --remove-signature "$APP" || true
    codesign --force --sign - "$APP/Contents/MacOS/libusb-1.0.0.dylib"
    codesign --force --sign - "$APP"
    codesign -dvvv "$APP" 2>&1 | grep -E 'flags|Signature' || true
fi

if pgrep -f "HX Edit" >/dev/null; then
    echo "==> quitting running HX Edit (it holds interface 0 exclusively)"
    osascript -e 'quit app "HX Edit"' 2>/dev/null || true
    for _ in $(seq 1 20); do
        pgrep -f "HX Edit" >/dev/null || break
        sleep 0.5
    done
    pkill -f "HX Edit" 2>/dev/null || true
    sleep 1
fi

rm -f "$LOG" "$MARK"
echo "==> launching instrumented HX Edit; log: $LOG"
DYLD_INSERT_LIBRARIES="$WORK/hxsniff.dylib" \
    HXSNIFF_LOG="$LOG" HXSNIFF_MARK="$MARK" \
    "$APP/Contents/MacOS/HX Edit" >"$WORK/stdout.log" 2>&1 &
echo "pid $!"
