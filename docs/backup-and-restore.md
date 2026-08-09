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
- **IRs** — read-back decoded: `op12` for the descriptor, `op11` for the samples
  (see below). Carmine's pedal normally carries **none** (cab models, not IRs),
  so today's backups are complete without them either way.
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

A `tools/hxsniff/capture.sh backup` capture of HX Edit backing up shows it does
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

## Capturing the restore

The backup capture gives the fast **read**; the matching **write** is captured
too — HX Edit doing **File → Restore from Backup** of the `.hxb` it had just
made, so the same data went back and nothing was lost. It is
`captures/mac-restore-capture.log` (`capture.sh restore`), and decoding it is
what turns op 109's write inverse into the restore path we want instead of 126
individual `save_preset`s, pacing included.

## Capturing IRs and the settings HX Edit can write

Two more scenarios, both step-at-a-time with a `### MARK` per click so
`attribute.py CAPTURE --marks` says which messages each one produced:

```sh
bash tools/hxsniff/capture.sh ir         # import, export, rename, copy, clear
bash tools/hxsniff/capture.sh globals    # every device setting HX Edit exposes
```

### What the IR capture settled [confirmed]

Reading an IR back is **not** part of the op-109 object store. It is its own
pair of opcodes on the control channel, and each export sends both:

```
op12 {112: slot}          → {112: slot, 113: checksum, 109: name,
                             114: 1, 115: 3, 123: false, 124: false, 125: 0}
op11 {112: slot, 101: 2}  → <blob of samples>
```

`op12` returns the **descriptor** — the same argument map `upload_ir` sends
under op 9, checksum included — and `op11` returns the **samples**. Op 10
renames a slot. A copy between slots is op12+op11 followed by op 9: the samples
travel to the host and back rather than moving inside the device.

The probe IRs say what the device does to what it is given. Everything comes
back **48 kHz mono 32-bit float, always 2048 samples**, whatever went in:
`ramp1024` returns as its exact `i/4096` ramp zero-padded to 2048, `steps2048`
with its powers of two intact, and `long96k` — 4096 samples at 96 kHz —
resampled rather than decimated (its values fall between the source's, so a
filter ran). Key 115 is 3 on every read, so the stored length is always the
2048-sample code.

**Stereo IR Import is inverted, or its labels are. [confirmed, one import per
setting]** The probe's left channel is `+ramp` and its right is `-ramp`, so the
sign of what comes back names the channel that was kept. Importing under **Use
Left Channel** returned the *negative* ramp — the right channel — and under
**Use Right Channel** the positive one. Mix Both Channels returned silence,
which is exactly `(L + -L)/2` and confirms the mechanism works per import.

**IRs, both directions.** IR *read-back* is the last unknown in the inventory
below, and it cannot be captured on a pedal with no IRs on it — so the scenario
imports first and exports after. The probe files come from
`tools/hxsniff/make-irs.py` and exist to be recognisable in a stream we cannot
yet frame: `ramp1024` is `s[i] = i/4096`, exact in both 24-bit PCM and f32, so
any four bytes on the wire give their own sample index; `steps2048` is a
staircase of powers of two, which makes chunk boundaries visible; `stereo512`
has the channels opposite in sign, so the bytes say what the Stereo IR Import
preference actually did; `long96k` is over both the 2048-sample limit and the
device's rate, so the capture shows what HX Edit converts before it uploads.
The scenario also takes a backup **with IRs present**, which is what places IRs
in the op-109 object store and in the `.hxb` blocks, and it clears the slots
again at the end. Export the files back out and keep them next to the capture —
the wire bytes are only decodable against the files they came back as.

### What the settings capture settled [confirmed]

`op25 {118: id, 119: value}`, one marked change at a time, names them outright.
HX Edit streams a write per intermediate value while a control is dragged, so a
distinct target value per control is what keeps them apart:

| id | Setting | Values seen |
|---|---|---|
| 14 | tempo source | `1` at connect, `0` written — the other two of Per Snapshot / Per Preset / Global / Host Sync are unseen |
| 16 | tempo, BPM | float |
| 27 | preset numbering format | `false` = 01A-42C, `true` = 000-125 |
| 95 | EXP/FS Tip | `false` = EXP 1, `true` = FS4 |
| 96 | EXP/FS Ring | `false` = EXP 2, `true` = FS5 |
| 97 | FS3 function | `0` Tap/Tuner, `1` Stomp 3 |
| 98 | FS4 function | `1` Stomp 4, `10` All Bypass |
| 99 | FS5 function | `2`, `8` FS Mode >, `11` Toggle EXP |
| 190–192 | Global EQ low peak | freq, Q, gain |
| 193–195 | Global EQ mid peak | freq, Q, gain |
| 196–198 | Global EQ high peak | freq, Q, **gain (198 inferred)** |
| 199 | Global EQ low cut | freq |
| 200 | Global EQ high cut | freq — **inferred** |
| 203 | Global EQ enabled | bool |

The two inferred ids come from `op76`, which returns the whole set at once as
`{63: enabled, 55: [110, 0.707, 0, 2000, 0.707, 0, 8000, 0.707, 0, 19.9, 20100]}`
— eleven values that 190–197 and 199 land on in exactly that order, leaving
position 9 as high peak gain and position 11 as high cut. `op77` is the
window's RESET and answers with the same shape. Opening the window reads ids
201 and 202 (202 = 1, 201 = null on an HX Stomp) rather than the coefficients.

**A negative result worth as much as the map.** Show/Hide Names, Manage
Favorites By and Stereo IR Import send **nothing to the device** — they are
HX Edit's own preferences. Preset Numbering Format sits beside Stereo IR Import
on the same tab and *is* a device write. Nothing in the dialog distinguishes
them.

**Settings.** Most of the pedal's globals are reachable only from its own Global
Settings menu; HX Edit writes a subset, and that subset is what a capture can
name. From HX Edit 3.82's resources it is: **Preferences → View** (name labels,
favourites ordering, preset numbering format), **Preferences → Presets/IRs**
(stereo IR import), **Preferences → Hardware Compatibility** — the device half,
pedal jacks and footswitch functions, which is populated per device and so is
prompted for by name at capture time — the **Global EQ** window (bypass, and the
eleven coefficients: Low Cut, three peaks of freq/Q/gain, High Cut — its
apply-to-1/4"-or-XLR selector is in `globaleq.xml` but hidden on an HX Stomp,
which has no XLR outs), the **tempo** field and its per-snapshot /
per-preset / global / host-sync mode, and a **tuner** panel that ships in
`tuner.xml` though whether an HX Stomp exposes it is untested. Each is changed
alone, under its own mark, which is what turns opcode-25 writes into named
settings and closes the id ↔ name item below.

The scenario deliberately leaves alone Clear Preset Library, Clear IR Library,
Restore Factory Setlists and Restore Factory Settings: they wipe the pedal, and
nothing is learned that a single setting write does not already show.

**A partition falls out of the restore dialog.** *Restore From Backup* lets you
tick which items to restore, so restoring **only Global Settings** from a `.hxb`
writes the globals and nothing else — the cleanest way to learn which `64` ids
in the op-109 object store are globals rather than presets or IRs. Same data
going back, so it costs nothing to run.

## Inventory: do we have everything?

| Thing | Backup (read) | Restore (write) |
|---|---|---|
| Presets | ✓ byte-exact; fast via op 109 (decoding) | `write_preset`+`save_preset` (slow); fast via op 109 inverse (capture restore) |
| Global settings | ✓ 154 objects; also in op-109 store | `set_object` (works); fast via op 109 inverse |
| Setlists | ✓ (name `PRESETS` + the 126 presets) | same as presets |
| IRs | ✓ op12 descriptor + op11 samples, decoded | `upload_ir` (works) |
| Favorites (device models) | ✓ in the op-109 object store | via op 109 inverse |
| Favorites (stompchain-local) | ✓ local file | local |

## Other open reverse-engineering

- **IR read-back.** Decoded — op12 then op11, above. What is left is the reply
  framing of op11's blob, against `captures/ir-exports/`: those are the files
  HX Edit wrote from the same reads, so the bytes on the wire have a known
  answer to be checked against.
- **Globals id ↔ name.** The 19 ids HX Edit writes are named above. The rest of
  the 154 still want correlating with the `GLOB` block's 156 named fields by
  value — they are reachable only from the pedal's own menu, so no capture can
  name them. Also unfinished: the FS3/FS4/FS5 function enums (six of roughly ten
  values each), and the tempo source enum (one of four).
