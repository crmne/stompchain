#!/bin/bash
# Extract HX Edit's model catalog and artwork into a directory this project can
# read, without needing HX Edit installed.
#
#   extract.sh                     find an installed HX Edit and copy from it
#   extract.sh HX_Edit_3.82.dmg    take it from a macOS installer
#   extract.sh HX_Edit_3.82.exe    take it from a Windows installer
#
# Those files are Line 6's and are not redistributable, which is exactly why
# this exists: you supply your own copy and it stays on your machine. Nothing is
# downloaded — Line 6 require an account, so fetching on your behalf is not
# something this can honestly do.
set -euo pipefail

DEST="${HX_RESOURCES_DEST:-$HOME/.local/share/stompchain/hx-resources}"

# The files that matter: names, parameter ranges, display formatting, the
# number-to-model table, and the artwork.
WANTED=(
    HX_ModelCatalog.json
    HelixControls.json
    Helix.sym
    icons_models
)
WANTED_GLOB='*.models'

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf '\033[31m error:\033[0m %s\n' "$*" >&2; exit 1; }

copy_from() {
    local src="$1" copied=0
    [ -d "$src" ] || die "no Resources directory at $src"

    mkdir -p "$DEST"
    for item in "${WANTED[@]}"; do
        if [ -e "$src/$item" ]; then
            cp -R "$src/$item" "$DEST/"
            copied=$((copied + 1))
        fi
    done
    for f in "$src"/$WANTED_GLOB; do
        [ -e "$f" ] || continue
        cp "$f" "$DEST/"
        copied=$((copied + 1))
    done

    [ "$copied" -gt 0 ] || die "found nothing to copy in $src — is that an HX Edit Resources folder?"
    say "copied $copied items to $DEST"
}

from_installed() {
    local candidates=(
        "/Applications/Line6/HX Edit.app/Contents/Resources"
        "$HOME/Applications/Line6/HX Edit.app/Contents/Resources"
        "/c/Program Files/Line 6/HX Edit/resources"
        "/mnt/c/Program Files/Line 6/HX Edit/resources"
    )
    for c in "${candidates[@]}"; do
        [ -d "$c" ] && { say "found HX Edit at $c"; copy_from "$c"; return 0; }
    done
    return 1
}

from_dmg() {
    local dmg="$1" mount
    command -v hdiutil >/dev/null || die "reading a .dmg needs macOS"
    mount="$(mktemp -d)"
    say "mounting $(basename "$dmg")"
    hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$dmg" >/dev/null \
        || die "could not mount $dmg"
    # Detach whatever happens next, so a failure does not leave it mounted.
    trap 'hdiutil detach "$mount" >/dev/null 2>&1 || true; rmdir "$mount" 2>/dev/null || true' EXIT

    local app
    app="$(find "$mount" -maxdepth 2 -name "HX Edit.app" -print -quit)"
    [ -n "$app" ] || die "no HX Edit.app inside $dmg"
    copy_from "$app/Contents/Resources"
}

from_exe() {
    local exe="$1" work
    # Line 6's Windows installer is a self-extracting archive; 7-Zip reads it.
    local sevenzip
    sevenzip="$(command -v 7z || command -v 7za || command -v 7zz || true)"
    [ -n "$sevenzip" ] || die "reading a .exe needs 7-Zip (brew install p7zip, or apt install p7zip-full)"

    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    say "extracting $(basename "$exe")"
    "$sevenzip" x -o"$work" -y "$exe" >/dev/null || die "could not extract $exe"

    # The resources land somewhere under the extracted tree; find the catalog
    # and take its directory as the source.
    local catalog
    catalog="$(find "$work" -name HX_ModelCatalog.json -print -quit)"
    [ -n "$catalog" ] || die "no HX_ModelCatalog.json inside $exe"
    copy_from "$(dirname "$catalog")"
}

main() {
    local source="${1:-}"

    if [ -z "$source" ]; then
        from_installed || die "no HX Edit found. Pass the installer instead:
      $0 /path/to/HX_Edit.dmg
      $0 /path/to/HX_Edit.exe
    Download it from https://line6.com/software/ (a free Line 6 account is required)."
    else
        [ -f "$source" ] || die "no such file: $source"
        case "$source" in
        *.dmg) from_dmg "$source" ;;
        *.exe) from_exe "$source" ;;
        *) die "expected a .dmg or .exe, got $source" ;;
        esac
    fi

    local models
    models=$(find "$DEST" -name '*.models' | wc -l | tr -d ' ')
    local icons
    icons=$(find "$DEST/icons_models" -name '*.png' 2>/dev/null | wc -l | tr -d ' ')
    say "$models model files, $icons images"
    say "stompchain will find these automatically; no configuration needed"
}

main "$@"
