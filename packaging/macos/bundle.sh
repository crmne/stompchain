#!/bin/bash
# Build TonePush.app from a GUI binary, on a macOS machine.
#
#   packaging/macos/bundle.sh <gui-binary> <output.app> <version>
#
# The .icns is generated here from the committed 1024px PNG, because iconutil
# only exists on macOS. The Info.plist template lives next to this script.
set -euo pipefail

gui="$1"
app="$2"
version="$3"
here="$(cd "$(dirname "$0")" && pwd)"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$gui" "$app/Contents/MacOS/tonepush"
chmod 755 "$app/Contents/MacOS/tonepush"
sed "s/__VERSION__/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"

iconset="$(mktemp -d)/tonepush.iconset"
mkdir -p "$iconset"
for size in 16 32 64 128 256 512; do
    sips -z $size $size "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/tonepush.icns"

echo "$app"
