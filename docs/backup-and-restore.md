# Backup and restore

Goal: **bulletproof backup and restore** for the HX Stomp — never lose a preset
again — covering presets, global settings, IRs, setlists, and favorites, with
automatic backups so a mistake is a shrug, not a crisis.

## Two formats, on purpose

| | MessagePack bundle (`.hxpreset` per preset) | HX Edit `.hxb` |
|---|---|---|
| Fidelity | **byte-exact** (what the pedal stores) | faithful, needs conversion |
| Portability | same pedal / firmware | **any firmware, any Helix-family unit** |
| Write path | `write_preset` + `save_preset` (hardware-validated) | needs a device→JSON converter (see below) |
| Risk | none — no transform | a wrong transform loses routing/snapshots |

Decision: the **MessagePack bundle is the safety/auto-backup format and our
restore path** — it cannot lose anything. The `.hxb` is a portable **export**,
added once the faithful converter exists. We already **read** `.hxb`
(`hx_catalog::read_backup`) and **write** the container byte-for-byte
(`hx_catalog::Container`, proven against four real backups).

## What a complete backup holds

- **Presets** — 126 slots, byte-exact `.hxpreset`. `backup-all` already does this;
  measured **~114 s** for a full read (each preset must be loaded; see fast-read below).
- **Global settings** — **154** answering device objects (`object(id)`), which line
  up with HX Edit's `GLOB` block (156 named fields: EQ 13, Tuner 11, DSP 5, System 127).
  Store id→value; labels can come later.
- **Setlists** — one on the HX Stomp, named `PRESETS`.
- **IRs** — read-back not yet reverse-engineered (see below). Carmine's pedal has
  **none** (cab models, not IRs), so today's backups are complete without them.
- **Favorites** — stompchain-local.

## Cadence

Full backup on demand / on quit / nightly (~114 s, disruptive: it loads each
preset). **Incremental after every save** (instant, one preset) for continuous
safety. Not on every connect, until a fast read exists.

## The `.hxb` container (AF6L) — decoded

```
[0:4]   "AF6L"
[4:8]   version (u32, = 1)
[8:16]  offset of the block table (u64)
[16:24] block count (u64)
[24:..] blocks, packed back to back (first is IDXH, at offset 24)
[table] 36-byte entries: tag(4) offset(u64) stored_len(u64) flags(u32) raw_len(u64) reserved(u32)
```

Blocks: `IDXH` (device id + Unix timestamp — the field that looked like a
checksum), `GLOB` (globals JSON), `SDMU`, `MNLS` (the name `PRESETS`), `SL00`
(setlist: `data.presets[]`, all 126 as `{meta,device,tone,device_version}`).
Compressed blocks are one zlib stream (flags==1); integrity is zlib's adler32,
so there is **no container checksum**. Implemented in `hx-catalog/src/hxb.rs`.

## MessagePack tone ↔ HX Edit JSON (the faithful converter, still to build)

The device document (`Preset.tone`, numeric keys) and HX Edit's `.hlx`/`.hxb`
JSON (symbolic) are two encodings of the same tone. `to_hlx` today emits blocks +
params + models correctly but **drops routing, snapshots, and the sections** — it
is lossy. A faithful converter must map, both directions:

- **Paths** `tone[0]`, `tone[1]` → `dsp0`, `dsp1`. Each path is `{21: routing, 22: [slots]}`.
- **Slots** (positional array) → **named nodes**. Slot `kind`: 0 input, 6 block,
  and 1/2/3 the routing nodes, 8 empty. JSON names them `inputA/inputB`,
  `block0..N`, `split`, `join`, `outputA/outputB`, `cab0`. Block bodies:
  `24` model-ref (`25` model number → symbol), `10` enabled, `11` values,
  `12` paired (cab) values, `9` routing/attach. Even factory presets use a Y-split.
- **Sections** → named: `tone[2..7,10]` carry the three snapshots (per-block
  bypass, controller values, name/color/tempo/valid), `global`
  (`@tempo/@topology*/@guitarinputZ/...`), `variax`, `controller`, `footswitch`.
  Each needs byte-exact identification against the golden pairs.

Validation is fully offline: all 126 presets exist as both `.hxpreset` (from
`backup-all`) and HX Edit JSON (from the `.hxb`), matched by name — golden pairs
for both directions.

## The fast read: opcode 109 (found in the Mac capture)

A `tools/hxsniff/capture-backup.sh` capture of HX Edit backing up shows it does
**not** load each preset. After a short handshake (`END`, `LIST_SETLISTS`,
`LIST_PRESETS` args=2 → 126 names, `READY`, `LIST_IRS`, `BEGIN`) it drives the
whole backup with **opcode 109**, a chunked *transfer* read, and the device
streams **342 KB** back on the reply channel — the pedal shows "transferring
data" rather than cycling presets. This is the fast read we could not find by
probing (`READ_PRESET`+index returns the *loaded* preset; `LIST_PRESETS`
selectors 0–6 return only names+flags).

Opcode 109 request args (RPC envelope: `100`=opcode, `101`=args, `102`=txn):

```
{ 64: <id>, 106: false }              # start transferring object <id>
{ 64: <id>, 106: true, 105: 48 }      # continue; 105 = flow-control offset/ack
{ 64: <id>, 106: true, 105: 687 }     # ...
```

In the backup capture, opcode 109 transfers **803 distinct objects** (`64` ids
0..832, with gaps), and opcode **4** is sent once per preset (126×, likely a
per-preset prepare/lock). So op 109 is not a preset reader - it is a general
**object-store transfer**, and reading every object *is* the whole backup:
presets, globals, IRs, favorites, and setlist, one mechanism. This is the unified
fast backup, and its write inverse is the unified fast restore.

Still to pin down before implementing: the reply framing on `1001→03ef` (where
each object's bytes land), how a transfer terminates, what op 4 does, and which
`64` ids are presets vs globals vs IRs vs favorites (so a partial restore is
possible). Capture: `captures/mac-backup-capture.log`.

## Capturing the restore (still needed)

The backup capture gives the fast **read**. We also want the fast, safe
**write**: capture HX Edit doing **File → Restore from Backup** (of the `.hxb` it
just made - same data, so it is harmless to the pedal) under
`tools/hxsniff/capture-backup.sh`. That reveals op 109's write inverse and how
HX Edit paces flash writes across a whole restore - the safe path we want instead
of 126 individual `save_preset`s.

## Inventory: do we have everything?

| Thing | Backup (read) | Restore (write) |
|---|---|---|
| Presets | ✓ byte-exact; fast via op 109 (decoding) | `write_preset`+`save_preset` (slow); fast via op 109 inverse (capture restore) |
| Global settings | ✓ 154 objects; also in op-109 store | `set_object` (works); fast via op 109 inverse |
| Setlists | ✓ (name `PRESETS` + the 126 presets) | same as presets |
| IRs | none on this pedal; come via op-109 store if present | `upload_ir` (works); IR *read* needs an IR loaded to capture |
| Favorites (device models) | ✓ in the op-109 object store | via op 109 inverse |
| Favorites (stompchain-local) | ✓ local file | local |

## Other open reverse-engineering

- **IR read-back.** Comes via the op-109 object store when IRs are present; to
  capture the IR-specific framing, load a test IR first. Low priority (none here).
- **Globals id ↔ name.** Correlate the 154 device object values with the `GLOB`
  block's 156 named fields (match by value) to label globals for an editor. Not
  needed for a value-faithful backup.
