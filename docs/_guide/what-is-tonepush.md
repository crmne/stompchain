---
title: What is TonePush?
description: Why TonePush exists, what it does, and what it honestly does not do yet.
nav_order: 0
---

## The problem

The Line 6 HX Stomp is a small box holding a large amp-and-effects rig. Editing it from the hardware means three footswitches and a knob. Editing it from a computer means HX Edit, which is excellent, closed, and only runs on macOS and Windows. If your studio machine runs Linux, or you want to script your pedal, or you want to build something on top of the device, you were out of luck.

TonePush is an open-source editor for HX-family devices. It talks the same USB protocol HX Edit talks, edits the same scratch buffer, and runs on Linux, macOS, and Windows. Behind it sits the protocol itself, reverse-engineered from USB captures and written up in [PROTOCOL.md](https://github.com/crmne/tonepush/blob/main/PROTOCOL.md), so this editor is not the only thing that can ever be built on it.

![TonePush editing a preset on an HX Stomp: a wah, distortion, amp and cab along the main line with a second cab on a parallel branch, the wah's knobs and its expression pedal assignment below, and the library along the bottom](/screenshot.png)

## What it does

Everything HX Edit does on an HX Stomp, verified operation by operation against the hardware:

- **Your whole rig at a glance.** Blocks, branches, and knobs laid out like the pedalboard they are. Drag a block below the line to run it in parallel, drag the fork and merge to move where the path splits, and choose how it splits: Y, A/B, crossover, or dynamic.
- **Editing.** Swap models from a searchable thumbnail browser with HX Edit's own artwork, turn knobs with values formatted exactly as HX Edit formats them, drag to reorder, undo and redo with the keyboard.
- **Presets.** Select, rename, save, copy, paste, import, export, and back up a whole setlist. A preset travels as the device's own document, byte for byte, so nothing is lost in translation.
- **Snapshots, setlists, tempo, impulse responses, device settings.** Switch, rename, edit, upload, and clear, each verified end to end.
- **Live activity.** The editor follows what you do on the front panel.

## What it does not do yet

This is a young project, and it says so:

- The tested device is an HX Stomp on firmware 3.80. Helix and Helix LT parse and render (two DSP paths, four lanes), but they have not met real hardware yet.
- The tuner is not here because it is not an HX Edit feature either: it lives on the hardware.
- Model names, ranges, and artwork come from HX Edit's own data files, which are Line 6's and are not redistributed. A small extractor pulls them from the installer you download from Line 6. Without them everything still works, just with numbers where names would be.

If something misbehaves, [an issue](https://github.com/crmne/tonepush/issues) with the `tonepush chain` output and what you expected instead is gold.

## Where this is going

The editor is part of [TonePush](https://tonepush.rocks), where players can find complete guitar tones, compare implementations for their hardware, and publish their own.

## Prior art

TonePush stands on earlier efforts: [`kempline/helix_usb`](https://github.com/kempline/helix_usb) found the multi-channel structure, [`allansomensi/openhx`](https://github.com/allansomensi/openhx) listed and selected presets in Rust, and [`AntonyCorbett/HelixBackupFiles`](https://github.com/AntonyCorbett/HelixBackupFiles) and [`frankdeath/hx-tools`](https://github.com/frankdeath/hx-tools) decoded file formats.
