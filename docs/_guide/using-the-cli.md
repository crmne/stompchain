---
title: Using the CLI
description: Script your HX Stomp from the command line, from preset backups to parameter changes.
nav_order: 2
---

The `stompchain` command does everything the editor does, plus a few things only a command line makes convenient: bulk backups, scripted parameter changes, and protocol work.

## Everyday commands

```sh
stompchain list             # find attached devices
stompchain info             # identity and firmware
stompchain presets          # every preset by name
stompchain select 7         # load index 7, which the device labels 03B
stompchain chain            # the signal chain, named, with values
stompchain topology         # the chain as it is wired, one row per branch
stompchain set 4 Drive 5.0  # set a parameter by name, in displayed units
stompchain enable 4 off     # bypass a block
stompchain snapshot 2       # switch snapshot
stompchain move 4 5         # reorder blocks
stompchain save             # commit the edit buffer; without this, edits are lost
```

Preset indices are zero-based within a setlist: index 7 is `03B`. Parameter values are typed in the units HX Edit displays (`5.0` on a knob shown 0..10, `100` on a percentage, `Limit` on a switch) and converted for you.

## Backups and files

```sh
stompchain backup tone.bin  # the loaded preset, byte for byte
stompchain restore tone.bin # and back again
stompchain backup-all dir/  # every preset in the setlist, one file each
stompchain export tone.json # human-readable export, for diffing
stompchain import a.hlx --dry-run   # preview an .hlx; needs no hardware
```

A backup is the device's own preset document, untouched. Restoring one puts back exactly what was there, including anything this editor does not model.

## Blocks, routing, settings

```sh
stompchain copy-block 1 3   # copy a block over another slot
stompchain copy-snapshot 1 2
stompchain route 0 "Return" # route an input or output
stompchain setting 203      # read a device setting; set-setting writes
stompchain rename 7 "New Name"
stompchain ir-load 1 cab.wav
```

## Watching and poking

```sh
stompchain watch            # stream front-panel activity
stompchain models           # browse the catalog; needs no hardware
stompchain decode capture.log   # decode a USB capture; needs no hardware
```

`watch` prints what the device reports as you touch the front panel, which is also the fastest way to discover what an unlabelled control actually sends. `decode` replays the captures the protocol documentation was reconstructed from; the [opcode dictionary](/opcodes/) is the map.
