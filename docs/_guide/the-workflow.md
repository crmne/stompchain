---
title: The Workflow
description: How the pedal, your library and your setlists fit together, and the loop that takes a tone from an idea to a gig.
nav_order: 3
---

The pedal, your library and your setlists are three different things, and knowing which is which is most of knowing how to use tonepush.

**The pedal is where you make a tone**, because it is where you hear it. Everything you change goes to the pedal's scratch buffer, so it is audible the moment you change it, and none of it is permanent until you save.

**Your library is where tones live afterwards.** It is on your computer, not on the pedal, so it outlives any slot on any device. Sell the pedal, buy another, and your tones are still there.

**A setlist is a whole pedal kept as one thing.** All 126 slots, in order, as they were on the night they worked.

## The loop

### 1. Make a tone on the pedal

Swap blocks from the browser, turn knobs, drag to reorder the chain, run something in parallel. The dot beside the preset name goes amber the moment anything differs from what is stored, and `Ctrl+S` commits it.

You do not have to save to keep a tone, though. That is what the next step is for.

### 2. Keep the ones worth keeping

Every preset in the list has a button beside its star that copies it into your library. What goes in is the device's own document, byte for byte, so the snapshots and the routing come with it. Nothing is rebuilt from what the editor happens to show, which means nothing is quietly dropped.

Tones in your library are ordinary files in an ordinary folder. You can back them up, sync them, or read them with something else.

### 3. Build a setlist

Get the pedal holding the presets you want, in the order you want them, then open the Setlists half of the library and choose **Capture the pedal**. That records every slot and what is in it.

Give it the name of the gig, the venue, the date. You will want them later.

### 4. Play it back

**Put this setlist on the pedal** writes the whole thing back. If you only need one preset out of one, each slot has its own **Send**, which puts that preset back in the slot it came from.

## Changing a setlist

Put it back on the pedal, edit there, keep the changed tones to your library, and capture a new setlist.

A setlist is never edited in place. That looks like a limitation and is not: a setlist is a record of a rig that worked on a particular night, and a record you can edit is not a record. Renaming a tone next month should not reach backwards and change what you played in March.

This is also why deleting a tone from your library never breaks a setlist. If a setlist still plays it, the tone is kept for that setlist even after it leaves your library.

## Sending a tone to the web

A tone in your library can be written out for the tonepush tones site with **Export for the web**. You get two files: the tone as Line 6's own `.hlx`, which the site reads to work out what the tone is, and a `.json` of the details only you know, in the fields the site expects.

## Where everything is

| | Where | What it is |
|---|---|---|
| Tones | `~/.local/share/tonepush/library` | One file per tone, the pedal's own document |
| Setlists | `library/setlists` | Small JSON files naming the tones they play |
| Automatic backups | `~/.local/share/tonepush/backups` | The whole pedal, refreshed on connect, with dated snapshots behind it |

On macOS these sit under `~/Library/Application Support`; on Windows, under your profile.

None of it is a lock-in. If you stop using tonepush tomorrow, your tones are still files you can open.
