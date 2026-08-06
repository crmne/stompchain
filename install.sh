#!/bin/bash
# Build and install stompchain.
#
#   ./install.sh              build and install everything
#   ./install.sh --cli-only   skip the GUI
#   ./install.sh --uninstall  remove what this installed
#
# Installs the `stompchain` command into the first writable directory already on your
# PATH, and on macOS builds the GUI into a double-clickable app. Nothing is
# written outside your home directory unless a system path is already writable
# and on PATH.
set -euo pipefail

APP_NAME="stompchain"
BIN_DIRS=("$HOME/.local/bin" "/usr/local/bin" "$HOME/bin")
MAC_APPS="$HOME/Applications"
LINUX_APPS="$HOME/.local/share/applications"
UDEV_RULE="/etc/udev/rules.d/70-line6-hx.rules"
LINE6_VENDOR="0e41"
HX_RESOURCES="${XDG_DATA_HOME:-$HOME/.local/share}/stompchain/hx-resources"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33m warning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31m error:\033[0m %s\n' "$*" >&2; exit 1; }

bin_dir() {
    for d in "${BIN_DIRS[@]}"; do
        case ":$PATH:" in *":$d:"*) [ -d "$d" ] && [ -w "$d" ] && { echo "$d"; return; };; esac
    done
    # Nothing suitable on PATH: make the conventional one and say so.
    mkdir -p "${BIN_DIRS[0]}"
    echo "${BIN_DIRS[0]}"
}

uninstall() {
    local dir
    for dir in "${BIN_DIRS[@]}"; do
        for bin in stompchain stompchain-gui; do
            [ -f "$dir/$bin" ] && { rm -f "$dir/$bin"; say "removed $dir/$bin"; }
        done
    done
    [ -d "$MAC_APPS/$APP_NAME.app" ] && {
        rm -rf "$MAC_APPS/$APP_NAME.app"
        say "removed $MAC_APPS/$APP_NAME.app"
    }
    [ -f "$LINUX_APPS/$APP_NAME.desktop" ] && {
        rm -f "$LINUX_APPS/$APP_NAME.desktop"
        say "removed $LINUX_APPS/$APP_NAME.desktop"
    }
    local icon="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps/$APP_NAME.svg"
    [ -f "$icon" ] && {
        rm -f "$icon"
        say "removed $icon"
    }
    if [ -f "$UDEV_RULE" ]; then
        warn "left $UDEV_RULE in place; remove it with sudo if you want it gone"
    fi
    say "done. Your presets and device are untouched."
    exit 0
}

make_app_bundle() {
    local gui="$1" app="$MAC_APPS/$APP_NAME.app"
    mkdir -p "$app/Contents/MacOS"
    cp "$gui" "$app/Contents/MacOS/$APP_NAME"
    cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>stompchain</string>
    <key>CFBundleIdentifier</key><string>me.paolino.stompchain</string>
    <key>CFBundleExecutable</key><string>$APP_NAME</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.2</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    # Ad-hoc signing keeps Gatekeeper quiet about an unsigned local build.
    codesign --force --sign - "$app" >/dev/null 2>&1 || true
    echo "$app"
}

# A normal user cannot open a USB device on Linux without being granted access.
# Without this rule everything fails with a permission error that looks like a
# bug in this program, so it is worth doing at install time.
install_udev_rule() {
    if [ -f "$UDEV_RULE" ]; then
        say "udev rule already present"
        return
    fi
    # The canonical rule ships in packaging/, where distro packages take it
    # from; the inline fallback keeps a bare checkout working.
    local rule
    if [ -f "packaging/udev/70-line6-hx.rules" ]; then
        rule="$(grep -v '^#' packaging/udev/70-line6-hx.rules)"
    else
        rule="SUBSYSTEM==\"usb\", ATTR{idVendor}==\"$LINE6_VENDOR\", MODE=\"0666\", TAG+=\"uaccess\""
    fi

    if [ "$(id -u)" = 0 ]; then
        printf '%s\n' "$rule" >"$UDEV_RULE"
    elif command -v sudo >/dev/null; then
        say "USB access needs a udev rule; asking for sudo"
        printf '%s\n' "$rule" | sudo tee "$UDEV_RULE" >/dev/null || {
            warn "could not write $UDEV_RULE — run the installer as root, or create it by hand:"
            printf '      %s\n' "$rule" >&2
            return
        }
    else
        warn "no sudo available. Create $UDEV_RULE containing:"
        printf '      %s\n' "$rule" >&2
        return
    fi

    sudo udevadm control --reload-rules >/dev/null 2>&1 || true
    sudo udevadm trigger >/dev/null 2>&1 || true
    say "installed $UDEV_RULE (replug the device to apply)"
}

make_desktop_entry() {
    local gui="$1"
    local icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
    mkdir -p "$LINUX_APPS" "$icons"
    # The Exec path is written absolute: desktop launchers do not share the
    # shell's PATH, and a bare command name quietly fails there.
    cat >"$LINUX_APPS/$APP_NAME.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Version=1.0
Name=stompchain
GenericName=HX Signal Chain Editor
Comment=Editor for Line 6 HX hardware
Exec=$gui
Terminal=false
Categories=AudioVideo;Audio;
Keywords=Line 6;HX;Helix;stomp;pedal;guitar;preset;tone;
Icon=$APP_NAME
StartupNotify=false
DESKTOP
    [ -f "packaging/icons/$APP_NAME.svg" ] &&
        install -m 644 "packaging/icons/$APP_NAME.svg" "$icons/$APP_NAME.svg"
    update-desktop-database "$LINUX_APPS" >/dev/null 2>&1 || true
    gtk-update-icon-cache "${icons%/hicolor*}/hicolor" >/dev/null 2>&1 || true
    echo "$LINUX_APPS/$APP_NAME.desktop"
}

main() {
    local cli_only=0
    case "${1:-}" in
    --uninstall) uninstall ;;
    --cli-only) cli_only=1 ;;
    --help | -h)
        sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    esac

    command -v cargo >/dev/null || die "Rust is not installed. Get it from https://rustup.rs"

    say "building (this takes a few minutes the first time)"
    if [ "$cli_only" = 1 ]; then
        cargo build --release -p hx-cli
    else
        cargo build --release
    fi

    local dir
    dir="$(bin_dir)"
    install -m 755 target/release/stompchain "$dir/stompchain"
    say "installed $dir/stompchain"

    if [ "$cli_only" = 0 ] && [ "$(uname)" = "Darwin" ]; then
        mkdir -p "$MAC_APPS"
        say "installed $(make_app_bundle target/release/stompchain-gui)"
    elif [ "$cli_only" = 0 ]; then
        install -m 755 target/release/stompchain-gui "$dir/stompchain-gui"
        say "installed $dir/stompchain-gui"
        say "installed $(make_desktop_entry "$dir/stompchain-gui")"
    fi

    if [ "$(uname)" = "Linux" ]; then
        install_udev_rule
    fi

    # Model names, parameter ranges and artwork all come from HX Edit's own
    # data files. Set them up now so the editor is useful the first time it
    # opens rather than showing bare numbers.
    if [ ! -d "$HX_RESOURCES" ]; then
        if ./tools/hxresources/extract.sh >/dev/null 2>&1; then
            say "extracted HX Edit's model data to $HX_RESOURCES"
        else
            warn "no HX Edit found, so models will show as numbers without names or pictures.
      Fix it with: tools/hxresources/extract.sh /path/to/HX_Edit.dmg
      (or .exe — download from https://line6.com/software/)"
        fi
    fi

    case ":$PATH:" in
    *":$dir:"*) ;;
    *) warn "$dir is not on your PATH. Add it with:
      echo 'export PATH=\"$dir:\$PATH\"' >> ~/.zshrc" ;;
    esac

    echo
    say "ready. Quit HX Edit first — it holds the device exclusively — then:"
    echo "      stompchain list      find your device"
    echo "      stompchain chain     show the loaded preset"
    if [ "$cli_only" = 0 ]; then
        if [ "$(uname)" = "Darwin" ]; then
            echo "      open -a $APP_NAME    the editor"
        else
            echo "      stompchain-gui       the editor"
        fi
    fi
    echo
}

main "$@"
