# HX protocol notes

Working notes on the USB protocol used by Line 6 HX-family hardware, reconstructed
by observation. Nothing here is derived from Line 6 source code.

Test rig: HX Stomp (firmware 3.80), macOS 27.0 on Apple Silicon, HX Edit 3.82.

Confidence is marked throughout: **[confirmed]** means verified against captured
traffic on this rig, **[inferred]** means a hypothesis that fits the data but has
not been isolated, and **[open]** means unresolved.

## Device topology

HX Stomp enumerates as a composite device, `0e41:4246`
(vendor `0x0E41` = Line 6), device version 2.00, serial `3196883`.

| Iface | Class | Endpoints | Role |
|---|---|---|---|
| 0 | `0xFF/00` vendor specific | `0x01` bulk OUT, `0x81` bulk IN — 512 B | **editor channel** |
| 1 | `0x01/01` audio control | — | audio |
| 2 | `0x01/02` audio streaming | `0x03` isoc OUT, 224 B (alt 1) | playback |
| 3 | `0x01/02` audio streaming | `0x83` isoc IN, 224 B (alt 1) | capture |
| 4 | `0x01/03` MIDI streaming | `0x02` bulk OUT, `0x82` bulk IN — 512 B | musical MIDI |
| 5 | `0x03/00` HID | `0x84` interrupt IN, 8 B | switches/knobs |

Interface 0 has no kernel driver bound to it, and HX Edit holds it exclusively
while running — so a third-party client must wait for HX Edit to quit.

Known product IDs (`0x0E41` vendor): `0x4246` HX Stomp, `0x4253` HX Stomp XL,
`0x4248` Helix Floor. No public PID is known for HX Effects, POD Go, Helix LT or
Helix Rack.

**The editor protocol is not on MIDI. [confirmed]** Every editor transfer in our
captures is on `0x01`/`0x81` after `libusb_claim_interface(0)`. Interface 4 carries
only ordinary musical MIDI. This decides platform reach: iOS gives third-party
apps no raw USB access, so **iOS cannot be supported** unless a MIDI path is
found. Android is unaffected — its USB Host API reaches interface 0.

The device does answer a standard Universal Identity Request on its CoreMIDI port:

    TX  F0 7E 7F 06 01 F7
    RX  F0 7E 7F 06 02 00 01 0C 21 00 06 00 03 50 00 00 F7

giving manufacturer `00 01 0C` (Line 6), family `0x0021`, member `0x0006`
(HX Stomp), software revision `03 50 00 00`. The concatenation `0x00210006` is
the device identifier reused in `HX Edit.prefs` and in the `device` field of
`.hlx` preset files.

### Firmware version encoding [confirmed]

Earlier notes could not decide whether `03 50` meant 3.50 or 3.80. It is **3.80**,
and both encodings appear:

- Over the wire, the preset payload carries `35: 0x03800000` — byte `0x80` read as
  BCD is 80, giving 3.80. Other version gates in `HelixModelDefs.bin` are
  `0x02990000`, `0x03690000`, `0x03790000`; `0x99` and `0x69` are only meaningful
  as BCD, which fixes the encoding.
- Over MIDI, the same version appears as `0x50` = 80 **decimal**. It has to: SysEx
  data bytes cannot exceed `0x7F`, so `0x80` is unrepresentable and Line 6 switch
  to plain decimal for that field.

So read the internal field as BCD and the MIDI field as decimal. Both yield 3.80,
consistent with the paired HX Edit 3.82.

Note the preset also carries a build string `37: 'v3.71-32-g1039661'`, which does
**not** match 3.80 — it is most likely the firmware that last serialised the
preset rather than the running firmware. **[inferred]**

## How these captures were taken

macOS on Apple Silicon cannot capture USB traffic without disabling SIP, and even
then Apple Silicon is reported to return all-zero payloads. Wireshark's ChmodBPF
does not help — it only chowns `/dev/bpf*` and cannot create the `XHC*`
pseudo-interfaces, which stay hidden while SIP is on.

We sidestep packet capture entirely. HX Edit links a **bundled copy of libusb**
(`@executable_path/../MacOS/libusb-1.0.0.dylib`), so every byte it exchanges with
the device passes through a handful of known entry points. `tools/hxsniff`
interposes them with `DYLD_INSERT_LIBRARIES` and logs complete buffers — better
than a packet capture, because message boundaries are exact and no reassembly of
USB transactions is needed.

The shipped app is signed with the hardened runtime, which makes dyld ignore
`DYLD_INSERT_LIBRARIES`, so `run.sh` works on a *copy* of the bundle and re-signs
it ad-hoc. The installed app is never modified. Its entire entitlement set is four
`com.apple.security.cs.*` keys all set to `false`, so dropping the signature costs
nothing functionally.

### Do not call USB reset

`libusb_reset_device` / `nusb`'s `Device::reset` takes the HX Stomp **off the bus
and it does not re-enumerate** — recovering it needs a physical unplug/replug.
This was tried as a way to clear stale session state and cost a reconnect, so it
is called out here rather than left to be rediscovered.

### The device can wedge, and only a power cycle clears it [confirmed]

A channel left mid-conversation by a client that exited without closing can end
up refusing new sessions: it answers every handshake with a bare acknowledgement
carrying its last sequence number and never opens.

This is **not** a defect in this client. Captured evidence: after it happened,
HX Edit 3.82 was started against the same device, sent its own handshake, got
nothing but repeating acknowledgements on channel `0x1001`, and released the
interface without connecting. Line 6's own editor cannot recover it either.

Recovering it needs the **9V adapter pulled**, not just the USB cable: the unit
is externally powered, so it keeps its session across a USB replug. Confirmed by
watching its sequence counter carry on from `0x86` to `0xcc` across a
re-enumeration onto a different bus.

#### The big one: nothing is posted to read between operations [confirmed]

This was the cause of most lock-ups seen during development, and it is a host
bug rather than a device one.

The device emits notifications whether or not anyone asked. If the client only
posts a USB read buffer while a request is in flight — the obvious way to write
it — then between operations the device has nowhere to put them. Once its
outgoing queue is full it stops draining the **incoming** endpoint as well, and
the next write simply times out with nothing visibly wrong. The symptom is a
device that took several operations happily and then refused, so it reads like
something you did on the last one rather than a backlog from all of them.

Draining the endpoint before each request, and acknowledging any channel that
received bytes while nobody was waiting on it, moved this from failing on the
third preset-document write to comfortably past ten. Measured on an HX Stomp
running 3.80.

Two things that look like fixes and are not:

- **Acknowledging the channel a reply just arrived on, before returning.** The
  acknowledgement already rides in the header of every frame the client sends,
  so this adds nothing — and the extra ACK frame burns a sequence number
  mid-transaction. It made the next read time out immediately.
- **Wrapping the acknowledgement at 16 bits.** Failures cluster near where a
  16-bit counter plus the `0x1000` base would overflow, but masking changed
  nothing and sessions still failed with as little as 21 KB received.

**Resolved: deferred operations must be paced on notification 20.
[confirmed]** The sustained-write failure was the last piece of this story. A
deferred operation (select preset, write preset, IR upload) answers status 1 —
accepted — and announces actual completion later as notification 20 carrying
the same transaction id. HX Edit will not start the next such operation until
that notification arrives: fourteen consecutive captured undo writes all follow
the pattern, each taking ~300 ms reply-to-notification. A client that treats
the status-1 reply as completion races its next write against the device's
still-running commit; the device tolerates roughly a dozen racing commits and
then stops accepting writes. With the wait in place, twenty back-to-back
document writes complete and the device stays healthy —
`crates/hx-usb/tests/device.rs` holds the regression test.

Three further things make it more likely, all learned the hard way:

- **Re-running the handshake on an open channel.** This is the big one. An
  earlier version answered a request timeout by redoing the whole handshake, up
  to four times, sending fresh HELLO frames on channels the device already had
  open. Every failure amplified into a burst of them, and a GUI that polls
  continuously produced them fastest — which is exactly when lock-ups happened.
  **Handshake once per session and never again.** Report a timeout instead of
  trying to fix it. **[inferred, strongly]**
- **Sustained reads.** A drain loop that keeps the IN endpoint busy for tens of
  seconds also coincided with lock-ups. Bound it to a few seconds.
- **Opening service 2 on the control channel at connect time.** HX Edit opens
  service 5 first and only reaches for 2 later in its session. **[inferred]**

The general lesson: this device's session layer is not defensive. It assumes a
well-behaved peer that opens each channel once and then speaks in order. Anything
that looks like a second client arriving mid-conversation can wedge it.

There is also a host-side trap that resembles a dead device but is not one. A
client that exits without closing leaves the device acknowledging into a buffer
nobody reads, and that backlog survives process restarts: the next session reads
hundreds of stale acknowledgements with steadily climbing sequence numbers
instead of its own handshake reply. Draining until reads genuinely come up empty
distinguishes the two — a real backlog clears (about 100 frames here), a wedged
device does not.

**The teardown exists, and is a HELLO. [confirmed]** A clean quit, captured
with the interposer, shows HX Edit acknowledging anything outstanding, sending
a bare type-0x02 frame on each of the three channels, collecting the device's
answering 0x02s, and releasing the interface. The 0x02 message is a session
*boundary*, not just an opening handshake — it appears at both ends of the
conversation, and the closing form is just the 8-byte channel header with the
current sequence and acknowledgement. An earlier version of this section
claimed HX Edit sent nothing recognisable on quit; that conclusion came from
`captures/04-quit.log`, which on re-reading records HX Edit failing against an
already-wedged device — repeating stale ACKs answering every HELLO — not a
quit. This client now performs the same teardown when a session drops.

## Layer 1 — USB framing [confirmed]

Every bulk transfer on `0x01`/`0x81` is one framed message:

```
offset  size  field
0       2     originator: 1 host, 0 device                [confirmed]
2       2     service id; garbage on some device replies  [confirmed]
4       4     MessagePack body length, u32 LE
8       n     MessagePack body
```

`tools/hxsniff/reassemble.py` implements exactly this and walks a whole capture
without desynchronising, which is the evidence that the layering is right: a
strict MessagePack reader fails immediately if the framing is off by a byte.

The two leading fields were resolved by a census of every stream message in the
captures (~900):

- **Offset 0 is the originator.** Host messages always carry 1, device messages
  always 0, with zero exceptions. It is not a flags word.
- **Offset 2 is the service id, but the device does not always initialise it.**
  Host messages always carry the true service. Device messages usually do, but
  certain replies — the first large reply on the control channel, and every
  jumbo preset document — carry junk instead: the *same* reply arrives as
  `0x0000`, `0x28e1` or `0x2100` in different captures of the same action, and
  the junk bytes are recognisable residue (frame-flag values, fragments of the
  blob length, even ASCII from neighbouring buffers). The receiver must trust
  only the length at offset 4 and ignore offset 2 on device messages, which is
  what this implementation does.

Two small related facts fell out of the same census: the reply to opening a
service carries a one-byte body echoing the service number, and a capture that
starts mid-session begins mid-message — the framing recovers at the next
message boundary because the length walk stays consistent from there.

## Layer 4### Global settings: opcode 24 reads, opcode 25 writes [confirmed, writing is hazardous]

Device settings are a flat numbered namespace rather than a structured
document. `op24 {118: id}` reads and `op25 {118: id, 119: value}` writes; 147
of the first 160 ids answer on an HX Stomp. Known ids: **16** tempo in BPM,
**28** current preset index, **192** global EQ low-peak gain, **203** global EQ
enabled. HX Edit's Global EQ window reads 201–203 when it opens, and its
Save Preset is a different opcode entirely (**71**, `{107, 108, 109: name}` —
the operation that moves an edit out of the edit buffer and into storage).

The value's type must match what the device already holds; a float where it
wants a boolean is refused with error −3.

Writing works and round-trips. **A note on how nearly it was mis-recorded:**
this section briefly said op25 destabilised the device, on the strength of an
elimination experiment — removing the settings-write test took the hardware
suite from two passing tests to twelve. The inference was wrong. That test also
called `irs()` on a control channel nothing had opened yet, and a cold control
channel does not answer its first request; the resulting timeout left the
session unhealthy and the *next* session unable to open. The write was carrying
the blame for its neighbour. With the health check corrected, sixteen tests
including the settings round trip pass twice over with no power cycle between.

The lesson generalises: elimination finds *a* thing that changes the outcome,
not necessarily the cause. Two changes in one test is one too many.

### Controller assignments [confirmed]

HX Edit's Bypass/Controller Assign page puts any parameter under any source.
Four opcodes cover it:

| Opcode | Arguments | Does |
|---|---|---|
| 37 | `{98: block, 26: 0, 28: param, 29: true, 74: source, 71: 4, 129: false}` | put a parameter under a controller |
| 36 | `{98, 29, 26, 28}` | read a parameter's assignment |
| 56 | `{98: block, 102: switch}` | put a block's bypass on a footswitch |
| 57 | `{98: block, 102: switch}` | take it off again |
| 33 | `{102: switch}` | read a footswitch's configuration |

Key **74 is the source**, as an ordinal in the order HX Edit lists them: 1–2 the
expression pedals, 3–7 the footswitches, 8 MIDI CC, 9 Snapshots. Keys 72 and 73
carry the ends of the controller's travel, normalised. Bypass is a switch, so
only a footswitch or a CC can drive it — HX Edit lists expression pedals for a
bypass and then steps over them.

**Method: those dropdowns answer the scroll wheel. [method]** HX Edit's
custom-drawn menus ignore synthetic clicks, and its controls are invisible to
the accessibility API, which had this section stuck at "the menu opens and
nothing can be chosen". A scroll event over the closed dropdown steps its
selection and sends the traffic, which is how the whole source list was mapped
one entry at a time.

### Identifying a flag by patching the reply [method]

A boolean nothing acts on is indistinguishable from any other boolean, so the
way to learn what one means is to change it and watch the client. Opcode 99
returns `{63: bool}` and is `false` in every state reachable over USB, which
left it unexplained for a long time — polling it during edits, reloads, flash
writes and even with the tuner engaged never moved it.

`tools/hxsniff` can rewrite a reply on its way into HX Edit while leaving the
wire to the device untouched:

```sh
HXSNIFF_PATCH=68813fc2/68813fc3 tools/hxsniff/run.sh
```

That pattern is `104 -> {63: false}` becoming `true`, which matches only an
opcode-99 reply. With it applied, HX Edit's tempo readout changes from `120.0`
to **`[External]`** — so opcode 99 asks *is the tempo being driven by an
external MIDI clock*. Sending the device real MIDI beat clock confirms it from
the other side: the flag reads true for exactly as long as the clock runs.
`tools/midiclock.swift` generates the clock and
`crates/hx-usb/tests/device.rs` holds the regression test.

Two lesser findings came out of the same hunt. **The device stops answering the
editor entirely while its tuner is engaged** (CC68 over MIDI) and resumes when
it is dismissed — worth knowing before diagnosing a timeout as a wedge. And
**key 63 means "in effect"** rather than anything preset-related: opcode 76
uses it for whether the global EQ is switched on, alongside its eleven
coefficients under key 55.

**Feeding the device MIDI beat clock kills an open editor session.
[confirmed]** The flag tracks the clock faithfully while it runs, but when the
clock stops the device stops answering over USB and does not recover — twenty
seconds of patient polling never gets a reply, and the 9V adapter has to come
out. Nothing the editor does causes it and nothing it does avoids it, so a
client that wants to be safe simply should not be the thing sending clock. The
regression test for the flag is therefore opt-in
(`STOMPCHAIN_DESTRUCTIVE=1`).

## Layer 4 — MessagePack RPC [confirmed]

The body is standard **MessagePack** with integer keys. Line 6's strings are C
strings whose declared length *includes* the trailing NUL, so `0xa9` introduces
`"l6-helix\0"` — strip trailing NULs after decoding. Floats are `0xca` float32,
big-endian per the MessagePack spec.

Three message shapes:

```
request       {102: txn, 100: opcode, 101: args}
response      {102: txn, 103: status, 104: result}
notification  {105: event, 106: args}
```

`txn` starts at 1000 per channel and increments. `status` 0 is success.
Notifications are unsolicited device→host and carry no transaction id — they are
how the device reports front-panel activity, cursor moves and preset switches.

Two real exchanges from our capture:

```
--> {102: 1013, 100: 30, 101: {98: 1, 29: True, 26: 0, 28: 0, 119: 0.78}}
<-- {102: 1013, 103: 0,  104: {98: 1, 29: True, 26: 0, 28: 0, 119: 0.78}}
<-- {105: 39, 106: {82: 1, 68: 3, 121: 19, 106: {98: 1, 26: 0}}}
```

### Opcodes (key 100)

| Op | Meaning | Args |
|---|---|---|
| 1 | list presets | `{107: setlist, 101: 2}` → array of `{index: {109: name, …}}` |
| 20 | select preset | `{107: setlist, 108: index}` |
| 22 | read current preset document | nil |
| 23 | current preset metadata | nil — returns `{107, 108, 109: name}` |
| 24 | fetch object by id | `{118: id}` |
| 30 | set parameter | `{98: block, 29: true, 26: path, 28: index, 119: value}` |
| 41 | enable/bypass block | `{98: block, 59: enabled}` |
| 88 | select snapshot | `{92: zero-based index}` |
| 28 | clear block | `{98: block}` — **must be preceded by opcode 78 selecting that block** |
| 37 | assign a controller | `{98: block, 95: target, 96: scope, 74: flags, 71: MIDI CC}` |
| 9 | upload impulse response | `{112: slot, 113: checksum, 109: name, 114, 115, …}` then raw samples |
| 6 | rename preset | `{107: setlist, 108: index, 109: name}` |
| 61 | set footswitch LED colour | — |
| 59 | set footswitch label | — |
| 68 | set MIDI CC / channel | — |
| 25 | set footswitch function | — |
| 78 | highlight slot | — |

Opcodes 0, 13, 23, 76, 99, 112 and 254 are observed during session setup but
their meaning needs hardware that exposes them: the Command Center opcodes are
inert on an HX Stomp, so a Helix Floor or LT is the prerequisite. Opcodes 6, 25, 59, 61, 68 and 78 come from the
`kempline/helix_usb` project rather than our own captures. **[inferred]**

Common argument keys: `107` setlist, `108` preset index, `109` name, `118` object
id, `119` value, `98` block index, `92` snapshot index.

**What HX Edit actually offers depends on the device. [confirmed]** Command
Center is inert on an HX Stomp — the menu item exists and clicking it does
nothing, because assigning banks of footswitches is a Helix Floor/LT feature. The
tuner is not in HX Edit at all; it lives on the hardware. Scoping "parity with HX
Edit" against a small device therefore covers less than the menu bar suggests.

### Impulse response upload [partly decoded]

Importing an IR sends opcode 9 on the data channel, followed by the audio as raw
bytes across subsequent messages — about 8 KB for a 1024-sample mono file:

```
{102: txn, 100: 9, 101: {112: slot, 113: 0xf7656589, 109: "test-impulse",
                         114: 1, 115: 3, 123: false, 124: false, …}}
```

**Key 113 is the samples summed as little-endian 32-bit words. [confirmed]**
Established by importing two IRs of different length through HX Edit and
testing candidates against both: CRC-32, Adler-32, byte sum and length all
fail; the wrapping word sum reproduces both exactly. Two earlier readings were
wrong — first a CRC-32 guess, then a conclusion that it was an identifier
rather than a digest, drawn from two files similar enough that their sums
shared a high half.

**HX Edit's full control-channel sequence around an upload. [confirmed]**

```
op 254 {}                      -> status 0
op 0   nil                     -> [{0: 'PRESETS'}]        setlists
op 1   {107: 0, 101: 2}        -> 126 preset entries
op 112 nil                     -> status 0
op 13  {101: 2}                -> [{112: slot, 109: name, ...}]   the IR list
op 255 {}                      -> status 0
op 9   {112, 113, 109, 114, 115, 123, 124, 125, 110}  -> status 1 (accepted)
op 254 {}
op 13  {101: 2}                -> the IR list again
```

Note op 9 answers **status 1**, so its outcome arrives later as a notification
rather than in the reply.

**Uploads work once the control channel does. [confirmed]** The long-running
failure was never IR-specific: op 9 rides on the control channel, and that
channel was silently misconfigured. With the channel fixed, a 256-sample IR
uploads, appears in its slot, and clears again.

**A one-frame upload fails too. [confirmed]** A 32-sample IR fits in a single
frame, so chunking and pacing are not the cause: it times out identically. The
fault is in the message itself — a missing or wrong field — not the transport.

Key 112 is the destination slot, 109 the display name, and **110 the samples as
little-endian `f32`** — the whole IR in one MessagePack blob, roughly 8 KB for a
1024-sample file. Key 113 is a wrapping sum of the sample bytes taken as
little-endian u32 words (not a CRC — see the checksum note below).

**Keys 114 and 115 declare the stored length. [confirmed]** The device stores
`114 × 256 × 2^115` samples. Isolated by uploading the same data under varied
values and comparing the stored content hash:

| 114 | 115 | data sent | stored |
|---|---|---|---|
| 1 | 3 | 1024 | 2048, zero-padded |
| 1 | 2 | 1024 | 1024, byte-identical |
| 1 | 1 | 512  | 512, byte-identical |
| 2 | 2 | 1024 | 2048, zero-padded — same image as 1/3 |
| 0 | –​ | any  | hangs the session |

So 115 is a length exponent and 114 a multiplier — plausibly a channel count,
though HX Edit always sends 1 and only the product is observable. Data shorter
than the declared length is zero-padded; data **longer** than declared wedges
the device's transfer state machine badly enough to need the 9V adapter pulled,
which is why this client derives the code from the sample count and refuses
anything over 2048 samples.

**The IR list reveals the stored bytes. [confirmed]** Each op 13 entry carries
key 104: the MD5, as lowercase hex, of the stored sample bytes *after* padding.
Uploading 1024 known samples under code 3 and hashing them locally with 4 KB of
zeros appended reproduces the device's value exactly. That makes end-to-end
verification of an upload free, and it is how the table above was measured.

Keys 123, 124 and 125 are not IR-specific — preset list entries carry the same
trio (`false`, `false`, `0` everywhere so far); their meaning is untested but
they echo back verbatim.

Opcode 15 `{112: slot}` empties a slot.

### Writing a preset document back [confirmed]

Opcode 21 takes a whole preset document. It is unforgiving: the preset carries a
table of byte offsets into itself, so a document that differs from the original
by even one byte of length leaves those offsets pointing at the wrong places, and
the device accepts it and then reads the preset as empty.

The cause here was that our MessagePack encoder normalised widths: a
value the device wrote as `0xcc 05` comes back out as `0x05`. Nothing in the
protocol objects, but the preset carries a **section offset table** (the second
of its three top-level values) holding byte offsets into the document — twelve
little-endian u32s: the offset of the tone map, the offsets of top-level keys
0, 1, 3, 4, 2, 5, 6, 7 and 10 in that fixed slot order (each pointing at the
key byte), then the total length twice. Decoded by matching candidate offsets
against the byte positions of the tone's sections in two captured presets, and
pinned by a test that recomputes the fixture's table byte for byte
(`Preset::computed_sections`). Change
any field's width and every offset after it is wrong, which is exactly the shape
of "device accepts it, preset reads back empty".

**Fixed by making the round trip byte-exact.** Three kinds of value had to keep
the tag width they arrived with — unsigned integers, signed integers and blobs —
because MessagePack lets the same value be written several ways and our encoder
chose the narrowest. In one captured preset, 91 of 103 wide integer tags would
have shrunk.

The diagnosis came from a test rather than from reasoning: a captured preset as
a fixture, re-encoded and diffed byte for byte, reporting where it first
diverges. It located each cause in turn — byte 10 (a blob tag), then byte 789
(an int16 zero) — where inspection had produced only plausible theories. That
test is `crates/hx-proto/tests/roundtrip.rs` and it needs no hardware.

**Uploads are verified end to end.** An earlier version of this section warned
that our upload hung the device; the cause was the control-channel
misconfiguration described above, not the message. The remaining hazard is the
declared-length rule: see keys 114/115 below.

**A message this large must be chunked, and paced. [confirmed]** The device
accepts 256 bytes of stream data per frame and paces the sender with
acknowledgements. Writing all 33 frames back to back fills its receive window and
stalls the endpoint: the transfer times out, and afterwards the interface will
not re-claim until the device is power-cycled. Read between chunks, as HX Edit
does.

**File dialogs are sheets, not windows. [method]** HX Edit's Import opens a
sheet attached to its main window, so `windows` shows one and the dialog looks
absent. `count sheets of window 1` finds it, and ⌘⇧G plus a path drives it.
Getting this wrong cost two rounds of concluding the feature was unreachable.

**Driving HX Edit's menu bar is the reliable way to capture features. [method]**
Its custom-drawn dropdowns ignore synthetic clicks entirely, which made several
operations look as though they generated no traffic. The menu bar is standard
AppleScript-addressable, and `File`, `Edit`, `Snapshots` and `Window` between
them expose preset import/export, block cut/copy/paste/clear, snapshot
operations, Global EQ and Command Center. Copying a block is purely local; only
the operations that change the device speak.

**Opcode 40 carries a model descriptor, not a bare model number. [confirmed]**
`{98: block, 100: {23: paired, 25: model, 26: second model or -1}}`. Sending
`{98, 25}` instead is answered with success and changes nothing — note that key
100 here means "model descriptor" while at the top level of a message it means
"opcode". Must still be preceded by a select.

**Clearing a block requires selecting it first. [confirmed]** Opcode 28 on its
own is answered with success and changes nothing — the quietest possible
failure. HX Edit always sends opcode 78 for the same block immediately before,
and with that the block disappears. Suspect the same pattern for any other
operation that reports success without effect.

**Snapshots switch with opcode 88. [confirmed]** An earlier capture concluded no
opcode existed, because clicking the snapshot menu produced no traffic — the
click was landing on the already-active snapshot, and HX Edit sends nothing for
a no-op. Driving it from the keyboard instead (⌘1/⌘2/⌘3) produced three clean
requests carrying `{92: 1}`, `{92: 2}`, `{92: 0}`, matching the shortcuts
exactly. Worth remembering as a method: when a UI action seems to produce no
traffic, check that it actually changed something.

**Key 108 is a linear zero-based preset index, not a bank number. [confirmed]**
`{107: 0, 108: 7}` is the preset the front panel labels `03B`, so the label is
`index / 3 + 1` followed by `A`/`B`/`C`. Selecting index 7 and reading the
metadata back returns `CT-Sad`, which is what HX Edit shows at 03B.

**Reply statuses (key 103). [confirmed]** Three values cover everything
observed:

| status | meaning |
|---|---|
| 0 | done |
| 1 | accepted; the operation completes later (select preset, write preset, IR upload) |
| 255 | refused; the result is `{111: signed error code}` |

HX Edit's own traffic contains only 0 and 1, which is why the refusals stayed
unmapped until deliberately bad requests were sent. Codes observed so far: `-3`
a bad block or parameter reference, `-46` an out-of-range snapshot, `-302` an
unknown model number. Two sharp edges: **accepted is not validated** — selecting
preset 999 on a 126-preset device answers 1 and simply does nothing — and a
no-op is not an error: clearing an already-empty IR slot answers 0.

The preset *name* is not part of the preset document — it comes from opcode 23.

A global-EQ read decodes as `{63: True, 55: [110.0, 0.707, 0.0, 2000.0, 0.707,
0.0, 8000.0, 0.707, 0.0, 19.9, 20100.0]}` — three bands of frequency/Q/gain plus
low-cut and high-cut, matching the device's Global EQ page.

## Layer 5 — the preset document [confirmed]

A preset arrives as an opcode-22/24 result: a MessagePack string/blob whose
contents are *themselves* MessagePack — three top-level values:

1. `'l6-helix'` — magic
2. a binary section table of u32 LE offsets
3. the preset map

The preset map decodes as:

```python
{7: {36: 'P33',                     # DSP / service name
     35: 0x03800000,                # firmware, BCD -> 3.80
     37: 'v3.71-32-g1039661'},      # build string
 0: {21: 0,
     22: [ {19: <slot type>, 20: {<parameters>}}, ... ]}}
```

Block parameter maps use `{2: n, 3: n, 4: [values]}` groups, and floats carry the
actual parameter values. Snapshot names (`SNAPSHOT 1`…) and the preset name appear
as plain strings.

### Model and parameter numbering [confirmed]

**`Helix.sym`'s array index is the device's model number.** The file ships with
HX Edit as a plain JSON array of 833 entries; entry *n* is model *n*. Index 247
is `HD2_ReverbRoomStereo` ("Room"), 296 is Simple Pitch, 180 is Bubble Vibrato —
each matching the model names that appear as text in captured presets. 829 of the
833 join to the `.models` catalog once mono/stereo suffixes are folded together.

Each entry also lists its parameters **in the order the device indexes them**,
which is what makes parameter addressing legible. A slot holding model 101
(Scream 808 — Gain, Tone, Level) carries exactly three values.

This was verified end to end: reading a preset and rendering it through the
catalog reproduces HX Edit's own parameter panel value for value, in order,
for a Cali Rectifire showing Drive 9.2, Bass 3.2, Mid 6.4, Treble 7.2,
Presence 6.2, Ch Vol 8.9, Master 1.8, Sag 2.0, Hum 2.7, Ripple 5.0, Bias 6.0,
Bias X 6.0.

Values on the wire are in the parameter's **native** units as the catalog defines
them, not normalised: a Mix shown as "100%" is `1.0`, a knob shown 0..10 is
stored 0..1. Switches are MessagePack booleans.

### Slot structure [confirmed]

The tone holds a fixed array of slots at `tone[0][22]`, each `{19: kind, 20: body}`:

| Kind | Meaning | Model number at | Values at |
|---|---|---|---|
| 0 | input | — | `7` |
| 1 | output | — | `7` |
| 2 | split | `15.8` | `15.7` |
| 3 | join | `17.8` | `17.7` |
| 6 | effect, amp or cab | `24.25` | `11` |
| 8 | empty slot | — | — |

Values arrive as `{2: count, 3: count, 4: [...]}`, with switch parameters
appearing as booleans inline among the floats.

Two fields sit beside the values rather than among them, which is why they never
appeared as parameters:

| Key | On | Meaning |
|---|---|---|
| `20.5` | input | `Input From` — indexes the `input_type` menu in `HelixControls.json` |
| `20.6` | output | `Output To` — indexes `output_type` |
| `20.9` | effect slots | the model's engine class — see below **[confirmed]** |

`Input From` and `Output To` are the first control HX Edit shows on Input and
Main L/R. The device **does not apply a change to them from a preset-document
write** — it accepts the document, keeps everything else, and leaves the
routing as it was. They are changed with their own opcode, captured from HX
Edit's routing clicks:

**Opcode 42 `{98: slot, 51: destination}` routes an endpoint. [confirmed]**
Answered synchronously with status 0, echoed as notification 27 carrying the
same arguments, and reflected in the document's key `20.5`/`20.6` on the next
read. One caveat: the destination values are per device model. On an HX Stomp,
HX Edit's three input choices send 1 (Main L/R), 4 (Return L/R) — values that
do not line up with the generic `input_type` menu in `HelixControls.json`
(where 4 is Variax), so the names shown for routing values on a Stomp are
approximate until its own enumeration is mapped. **[partly open]**

Key `20.9` was pinned down by setting one model per category into the same slot
and reading it back:

| models | `20.9` |
|---|---|
| distortion, dynamics, EQ, modulation, pitch, wah, volume/pan, preamp, cab | 1 |
| delay, reverb | 8 |
| amp | 18 |
| IR block, 1024-tap | 19 |
| IR block, 2048-tap | 20 |
| send/return, looper | 25 |

The reading that fits is an **engine or resource class**: simple effects share
one code, effects needing delay RAM share another, amps their own, the two IR
lengths take adjacent codes for double the memory, and the two block types that
touch hardware I/O share the last. It is a function of the model alone — both
copies of the same amp carry 18 whichever branch they sit on (it was first
misread as a branch index; the branch is implied by array position, see below).
Endpoints, splits and joins carry no key 9 at all. The device maintains the
value itself on model changes, so an editor only ever needs to carry it through
byte-exact — never to synthesise it.

### The slot array is a topology, not a running order [confirmed]

The array is laid out per signal path as:

```
input, blocks…, output, split, blocks…, join
```

The split appears *after* the output even though the signal reaches it first.
Read as a running order this puts the split and join on the end of the chain,
which is where they get drawn if you do not know better. HX Edit and Logic's
Pedalboard both draw a split as the wiring dividing into a second lane, not as a
box in the line.

Devices with more than one signal path repeat the whole pattern, so a Helix or
Helix LT preset with both paths split has four lanes. `stompchain topology` prints the
derived structure:

```
 A   0 Input [Multi (Guitar, Aux, Variax)]  ->   1 Cali Q Graphic  ->  …  ->   9 Output [Multi (1/4", XLR, …)]
 B  10 Split Y  ->  13 Line 6 2204 Mod  ->  19 Mixer
```

## Reference data shipped with HX Edit

HX Edit contains its full model catalog in the clear, which a third-party editor
needs for parameter ranges and names:

- `HelixModelDefs.bin` — MessagePack, 681 model definitions with `name`,
  `symbolicID`, `category`, and `params[]` carrying `min`, `max`, `default`,
  `valueType`, `displayType` and `assign` (the numeric parameter id).
- `HX_ModelCatalog.json` — 23 categories matching the editor UI.
- `Helix.sym`, `*.models`, `default_preset.hlx`.

**Licensing:** these are Line 6 proprietary data files. They must not be
redistributed. A third-party tool should read them from the user's own installed
HX Edit at runtime, and degrade gracefully when it is absent.

The `.hlx` preset file format is plain JSON and is independently documented; `.hxb`
bundles are a zlib-sectioned container (signature `AF6L`).

## Related work

- [`kempline/helix_usb`](https://github.com/kempline/helix_usb) — Python; the
  deepest prior effort. Found the three-channel structure and a module catalog.
- [`allansomensi/openhx`](https://github.com/allansomensi/openhx) — Rust;
  implements list and select preset, models a single channel.
- [`AntonyCorbett/HelixBackupFiles`](https://github.com/AntonyCorbett/HelixBackupFiles),
  [`frankdeath/hx-tools`](https://github.com/frankdeath/hx-tools) — file formats.

The Linux kernel's `snd-usb-line6` driver does **not** support HX devices; its
highest PID is `0x415A` and its SysEx protocol is the older POD/Variax one.

Helix Stadium XL uses a completely different transport (Bonjour + TCP + ZMTP),
so none of this applies to it.

## Tools

- `tools/hxsniff` — libusb interposer, capture driver, decoder, reassembler.
- `tools/midiprobe` — CoreMIDI list/listen/send with SysEx reassembly.
- `tools/usbprobe` — libusb interface/endpoint enumeration and claim testing.

Captures live in `captures/`.
