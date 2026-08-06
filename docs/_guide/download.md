---
title: Download
description: Get stompchain for macOS, Windows, or Linux, with install instructions for each.
nav_order: 1
---

{% assign v = site.stompchain_version %}
{% assign base = "https://github.com/crmne/stompchain/releases/download/v" | append: v %}

The current version is **v{{ v }}**. Every file below, with its SHA-256, is
listed in [checksums.txt]({{ base }}/checksums.txt); older versions live on
the [releases page](https://github.com/crmne/stompchain/releases).

## macOS

One download for both Apple Silicon and Intel:

- [stompchain-v{{ v }}-macos-universal.dmg]({{ base }}/stompchain-v{{ v }}-macos-universal.dmg)

Open it and drag **stompchain** to Applications. The DMG also carries the
`stompchain` command-line tool; copy it somewhere on your PATH if you want it.

> **If macOS refuses to open the app**, it is because this build is not yet
> notarized with Apple. Right-click the app, choose Open, and confirm; macOS
> remembers the choice. This one-time step disappears once notarized builds
> ship.
{: .warning }

Homebrew users can instead run:

```sh
brew install --cask crmne/stompchain/stompchain-app   # the app
brew install crmne/stompchain/stompchain              # the CLI
```

## Windows

Almost every PC wants the first one; the second is for Windows on ARM
(Surface and other Snapdragon machines):

- [stompchain-v{{ v }}-x86_64-pc-windows-msvc.zip]({{ base }}/stompchain-v{{ v }}-x86_64-pc-windows-msvc.zip)
- [stompchain-v{{ v }}-aarch64-pc-windows-msvc.zip]({{ base }}/stompchain-v{{ v }}-aarch64-pc-windows-msvc.zip)

Unpack and run `stompchain-gui.exe`. SmartScreen may warn about an unknown
publisher on first run; choose More info, then Run anyway.

## Linux

On Arch and its derivatives, install from the AUR, which also sets up the
udev rule and desktop entry:

```sh
paru -S stompchain        # or: stompchain-git for the development build
```

On any other distro, grab the archive for your machine:

- [stompchain-v{{ v }}-x86_64-unknown-linux-gnu.tar.gz]({{ base }}/stompchain-v{{ v }}-x86_64-unknown-linux-gnu.tar.gz)
- [stompchain-v{{ v }}-aarch64-unknown-linux-gnu.tar.gz]({{ base }}/stompchain-v{{ v }}-aarch64-unknown-linux-gnu.tar.gz)

Unpack it, put the two binaries on your PATH, and install the packaged udev
rule so you can open the device without root:

```sh
sudo install -m644 packaging/udev/70-line6-hx.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Then replug the pedal once. For the first-launch extraction of HX Edit's
model data, install `p7zip` too; the AUR package already suggests it.

## Build from source

Any platform, with [Rust](https://rustup.rs) installed:

```sh
git clone https://github.com/crmne/stompchain
cd stompchain
./install.sh
```

Packagers should read
[PACKAGING.md](https://github.com/crmne/stompchain/blob/main/PACKAGING.md),
which covers offline builds from the vendored-dependencies archive.

## After installing

Whichever route you took, finish with [Getting Started](/getting-started/):
one extraction step gives you model names and artwork, and there are a few
things worth knowing before your first edit.
