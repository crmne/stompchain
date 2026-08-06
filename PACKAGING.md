# Packaging stompchain

This repository is the upstream source of truth for release artifacts and shared
packaging assets. Distro-specific package recipes should usually live in the
package repository for that distro, not in this repository.

## Upstream Release Assets

Each tagged release publishes:

- `stompchain-v<version>-x86_64-unknown-linux-gnu.tar.gz`
- `stompchain-v<version>-aarch64-unknown-linux-gnu.tar.gz`
- `stompchain-v<version>-x86_64-apple-darwin.tar.gz`
- `stompchain-v<version>-aarch64-apple-darwin.tar.gz`
- `stompchain-v<version>-x86_64-pc-windows-msvc.zip`
- `stompchain-v<version>-aarch64-pc-windows-msvc.zip`
- `stompchain-v<version>-vendor.tar.xz`
- `checksums.txt`
- GitHub's automatic source archive for the tag

The Linux binary archives contain:

- `stompchain`
- `stompchain-gui`
- `README.md`
- `PROTOCOL.md`
- `LICENSE`
- `packaging/applications/stompchain.desktop`
- `packaging/icons/stompchain.svg`
- `packaging/udev/70-line6-hx.rules`

## Dependencies

Runtime:

- glibc and libgcc are the only linked libraries; the CLI needs nothing else
- the GUI loads the display stack at runtime: libGL, libxkbcommon, and
  Wayland or X11 client libraries, all present on any desktop install
- model names, parameter ranges, and artwork come from HX Edit's own data
  files, which are Line 6's and are not redistributable. Users extract them
  once with `tools/hxresources/extract.sh` from an HX Edit installer they
  download themselves. Packages must not bundle them. Everything degrades
  gracefully without them.

Build time:

- Rust `1.87` or newer (the pinned toolchain in `rust-toolchain.toml` is what
  CI uses; any newer stable works)
- on Linux, the GUI needs the usual egui build packages: `libxkbcommon-dev`,
  `libwayland-dev`, `libgl1-mesa-dev` or your distro's equivalents

## Build From Source

```sh
cargo build --release --locked
```

The two binaries land in `target/release/stompchain` and
`target/release/stompchain-gui`. `cargo test --workspace --locked` needs no
hardware; tests that do talk to a device are `#[ignore]`d by default.

For offline builds, unpack the vendor archive and build against it:

```sh
tar -xf stompchain-v<version>-vendor.tar.xz
cp -r stompchain-v<version>-vendor/.cargo .
cp stompchain-v<version>-vendor/Cargo.lock .
ln -s stompchain-v<version>-vendor/vendor vendor
cargo build --release --offline
```

The archive's `.cargo/config.toml` redirects crates.io to the vendored
sources; it is the file `cargo vendor` printed at archive time.

## Installed Files

Recommended installed files:

```text
/usr/bin/stompchain
/usr/bin/stompchain-gui
/usr/share/applications/stompchain.desktop
/usr/share/icons/hicolor/scalable/apps/stompchain.svg
/usr/lib/udev/rules.d/70-line6-hx.rules
/usr/share/licenses/stompchain/LICENSE
/usr/share/doc/stompchain/README.md
```

The udev rule is what lets a normal user open the USB device; without it every
connection fails with a permission error that looks like an application bug.
Do not force a udev reload from package scripts beyond the packaging norm for
your distro; tell the user to replug the device after installing.

## Package Status

Current status as of 2026-08-06:

| Channel | Status | Notes |
|---|---|---|
| Arch AUR | Prepared | Stable `stompchain` (release binaries) and VCS `stompchain-git` (source build) recipes are ready in the external packaging workspace. |
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
stompchain --version
stompchain --help
stompchain models >/dev/null && echo "catalog ok"   # only with HX Edit resources extracted
test -f /usr/share/applications/stompchain.desktop
test -f /usr/share/icons/hicolor/scalable/apps/stompchain.svg
test -f /usr/lib/udev/rules.d/70-line6-hx.rules
```

With an HX device on USB and HX Edit closed, also verify:

```sh
stompchain list
stompchain chain
```
