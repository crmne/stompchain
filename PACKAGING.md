# Packaging TonePush

This repository is the upstream source of truth for release artifacts and shared
packaging assets. Distro-specific package recipes should usually live in the
package repository for that distro, not in this repository.

## Upstream Release Assets

Each tagged release publishes:

- `tonepush-v<version>-x86_64-unknown-linux-gnu.tar.gz`
- `tonepush-v<version>-aarch64-unknown-linux-gnu.tar.gz`
- `tonepush-v<version>-macos-universal.dmg` (the app, drag to Applications, plus the CLI binary)
- `tonepush-v<version>-macos-universal.tar.gz` (bare universal binaries, for Homebrew and scripts)
- `tonepush-v<version>-x86_64-pc-windows-msvc.zip`
- `tonepush-v<version>-aarch64-pc-windows-msvc.zip`
- `tonepush-v<version>-vendor.tar.xz`
- `checksums.txt`
- GitHub's automatic source archive for the tag

The Linux binary archives contain:

- `tonepush`
- `tonepush-gui`
- `README.md`
- `LICENSE`
- `packaging/applications/tonepush.desktop`
- `packaging/icons/tonepush.svg`
- `packaging/udev/70-line6-hx.rules`

## macOS Signing and Notarization

The macOS job signs and notarizes the DMG when these repository secrets
exist; without them it ships the same DMG unsigned:

- `APPLE_CERTIFICATE_P12`: a Developer ID Application certificate with its
  key, exported as .p12 and base64-encoded
- `APPLE_CERTIFICATE_PASSWORD`: the .p12 password
- `APPLE_SIGNING_IDENTITY`: e.g. `Developer ID Application: Name (TEAMID)`
- `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`: notarytool credentials;
  the password is an app-specific password from appleid.apple.com

## Dependencies

Runtime:

- glibc and libgcc are the only linked libraries; the CLI needs nothing else
- the GUI loads the display stack at runtime: libGL, libxkbcommon, and
  Wayland or X11 client libraries, all present on any desktop install
- model names, parameter ranges, and artwork come from HX Edit's own data
  files, which are Line 6's and are not redistributable. The app walks the
  user through extracting them on first launch; packages must not bundle
  them.
- reading an HX Edit installer in-app needs 7-Zip (`7z`, `7za`, or `7zz` on
  PATH, or an ordinary Windows install of 7-Zip, which the app finds where
  the installer left it): package it as an optional dependency on Linux
  (`p7zip`) and Windows. macOS needs nothing extra; it uses hdiutil and pkgutil. A machine
  with HX Edit already installed needs no extraction at all: the app copies
  from the installation by itself.

Build time:

- Rust `1.87` or newer (the pinned toolchain in `rust-toolchain.toml` is what
  CI uses; any newer stable works)
- on Linux, the GUI needs the usual egui build packages: `libxkbcommon-dev`,
  `libwayland-dev`, `libgl1-mesa-dev` or your distro's equivalents

## Build From Source

```sh
cargo build --release --locked
```

The two binaries land in `target/release/tonepush` and
`target/release/tonepush-gui`. `cargo test --workspace --locked` needs no
hardware; tests that do talk to a device are `#[ignore]`d by default.

For offline builds, unpack the vendor archive and build against it:

```sh
tar -xf tonepush-v<version>-vendor.tar.xz
cp -r tonepush-v<version>-vendor/.cargo .
cp tonepush-v<version>-vendor/Cargo.lock .
ln -s tonepush-v<version>-vendor/vendor vendor
cargo build --release --offline
```

The archive's `.cargo/config.toml` redirects crates.io to the vendored
sources; it is the file `cargo vendor` printed at archive time.

## Installed Files

Recommended installed files:

```text
/usr/bin/tonepush
/usr/bin/tonepush-gui
/usr/share/applications/tonepush.desktop
/usr/share/icons/hicolor/scalable/apps/tonepush.svg
/usr/lib/udev/rules.d/70-line6-hx.rules
/usr/share/licenses/tonepush/LICENSE
/usr/share/doc/tonepush/README.md
```

The udev rule is what lets a normal user open the USB device; without it every
connection fails with a permission error that looks like an application bug.
Do not force a udev reload from package scripts beyond the packaging norm for
your distro; tell the user to replug the device after installing.

## Package Status

Current status as of 2026-08-06:

| Channel | Status | Notes |
|---|---|---|
| Arch AUR | Prepared | Stable `tonepush` (release binaries) and VCS `tonepush-git` (source build) recipes are ready in the external packaging workspace. |
| Fedora COPR | Not started | |
| Nixpkgs | Not started | |
| Gentoo GURU | Not started | |
| Alpine aports | Not started | |
| Debian and Ubuntu | Not started | |
| openSUSE OBS | Not started | |

Distro-specific recipes should remain in the distro package repository or the
external packaging workspace until they are accepted upstream. Keep this
repository limited to release assets and shared packaging files.

## Smoke Tests

After packaging, run:

```sh
tonepush --version
tonepush --help
tonepush models >/dev/null && echo "catalog ok"   # only with HX Edit resources extracted
test -f /usr/share/applications/tonepush.desktop
test -f /usr/share/icons/hicolor/scalable/apps/tonepush.svg
test -f /usr/lib/udev/rules.d/70-line6-hx.rules
```

With an HX device on USB and HX Edit closed, also verify:

```sh
tonepush list
tonepush chain
```
