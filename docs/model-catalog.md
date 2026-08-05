# HX model catalog and preset format

Format notes for the data files shipped inside HX Edit, reconstructed by inspection
of a locally installed copy. Nothing here is derived from Line 6 source code.

Reference copy examined:

    /Applications/Line6/HX Edit.app/Contents/Resources/

HX Edit **3.82** (`CFBundleVersion` 3.8.2) for macOS, files dated 2024-11-27,
universal (x86_64 + arm64). All files were opened read-only; none were modified.
Counts and field inventories below are specific to this build — re-run the
snippets against a different version rather than assuming they carry over.

## Licensing

The model catalog, the model definitions and the control tables are Line 6
copyrighted data. **Do not vendor them into this repository.** The intent of this
document is to describe the *format* precisely enough that an open-source tool can
locate and parse the user's own installed copy at runtime, on a machine where the
user has already licensed and installed HX Edit. A reader should:

- discover the resource directory at runtime, not bundle it;
- degrade gracefully (fewer features, no model names) when HX Edit is absent;
- treat every model name, parameter name and enum label as data owned by Line 6.

The handful of verbatim JSON records quoted below are the minimum needed to pin
down field layout, and are reproduced here for interoperability documentation only.

## Confidence key

Statements below are tagged:

- **[C]** confirmed — directly observed in the files, reproducible by re-running
  the snippets in this document.
- **[I]** inferred — consistent with everything observed, but not directly proven.
- **[U]** undetermined — stated explicitly as unknown.

---

## 1. File inventory

| File | Size | Format | Content |
|---|---|---|---|
| `HX_ModelCatalog.json` | 527 KB | JSON | Editor-facing browse tree: categories → subcategories → models, with display names, icons, parameter display order |
| `HX_ModelCatalog.bin` | 154 KB | MessagePack | Decodes to a structure exactly equal to `HX_ModelCatalog.json` **[C]** |
| `HelixModelDefs.bin` | 847 KB | MessagePack | Concatenation of all 19 `*.models` files, 681 records **[C]** |
| `*.models` (19 files) | — | JSON | Model definitions: parameter ranges, defaults, types, DSP load, device availability |
| `Helix.sym` | 121 KB | JSON | 833 firmware DSP symbols → **ordered** parameter-name list |
| `HelixControls.json` | 133 KB | JSON | 301 display/formatting/step definitions referenced by `displayType` |
| `default_preset.hlx`, `default_preset_hxs.hlx`, `default_preset_hfx.hlx`, `empty_preset.hlx` | — | JSON | Preset templates |
| `appStrings_eng.json` | 26 KB | JSON | UI string table (24 top-level keys, nested). Not model data. |
| `prefsDialog.xml` and friends | — | XML | Widget layouts. Occasionally useful — `prefsDialog.xml` carries device IDs as menu item IDs. |

The 19 `.models` files: `amp`, `cab`, `cabmicirs`, `cabmicirswithpan`,
`compressor`, `delay`, `distortion`, `eq`, `filter`, `fixed`, `gate`, `io`,
`modulation`, `pitch-synth`, `preamp`, `reverb`, `sendreturn`, `volumepan`, `wah`.

### 1.1 The `.bin` files are MessagePack — with one quirk

Both `.bin` files are standard MessagePack, and both parse to completion with zero
trailing bytes **[C]**. The quirk: **every string carries a trailing NUL byte
inside the msgpack length**. `0xAB "categories\0"` is a `fixstr` of length 11
holding a 10-character name plus a NUL. Verified over all 14 307 strings in
`HX_ModelCatalog.bin` and all 82 473 strings in `HelixModelDefs.bin` — zero
exceptions, and no zero-length strings **[C]**. A reader must strip one trailing
byte from every decoded string.

Type codes actually used **[C]**:

| File | Codes |
|---|---|
| `HX_ModelCatalog.bin` | `fixstr`, `str16`, `fixmap`, `fixarray`, `array16`, `nil`, `true`, `false`, positive `fixint`, `float32` |
| `HelixModelDefs.bin` | the above plus `uint8/16/32`, `int8`, negative `fixint` |

No `map32`, `array32`, `str32`, `bin*`, `ext*` or `float64` appear. `float64` is
absent, so **all real numbers in `HelixModelDefs.bin` are IEEE float32** — decoding
`load: 28.27` from the `.bin` yields `28.270000457763672`, whereas the `.models`
JSON has the exact decimal. Prefer the JSON when precision matters; otherwise
compare with a tolerance **[C]**.

`HX_ModelCatalog.bin` decodes to a structure that compares **exactly equal** to
`json.load(HX_ModelCatalog.json)` **[C]**. `HelixModelDefs.bin` decodes to a
681-element array that matches the concatenation of the `.models` files
element-for-element (to float32 tolerance) **[C]**, in this order **[C]**:

```
index range   file                      n
    0..110    amp.models              111
  111..151    cab.models               41
  152..197    cabmicirs.models         46
  198..243    cabmicirswithpan.models  46
  244..261    compressor.models        18
  262..309    delay.models             48
  310..370    distortion.models        61
  371..378    eq.models                 8
  379..393    filter.models            15
  394..404    fixed.models             11
  405..406    gate.models               2
  407..421    io.models                15
  422..477    modulation.models        56
  478..508    pitch-synth.models       31
  509..621    preamp.models           113
  622..646    reverb.models            25
  647..664    sendreturn.models        18
  665..669    volumepan.models          5
  670..680    wah.models               11
```

Practical consequence: **an implementation only needs the JSON files.** The `.bin`
files are a load-time optimisation and carry no extra information **[C]**.

---

## 2. Model catalog schema (`HX_ModelCatalog.json`)

Single top-level key `categories`, a list of 23 category objects **[C]**.

### 2.1 Category

```json
{
  "id": 1,
  "name": "Distortion",
  "image": "FX_HX_Category_Distortion.png",
  "shortName": "Dist",
  "color": "0xf5901e",
  "subcategories": [ ... ]
}
```

- `id` — small integer, **not** contiguous: the observed set is
  0–9, 11–23 (10 is absent) **[C]**.
- `color` — `0xRRGGBB` as a *string* **[C]**.
- `image` — filename resolved under `icons_category/` **[C]**: 22 of the 23
  category `image` values exist verbatim in that directory.
- A category has **either** `models` **or** `subcategories`, never both. Category
  23 (`Favorites`) has neither **[C]**.

Full category table **[C]**:

| id | name | shape | subcategories |
|---|---|---|---|
| 0 | None | models (1) | — |
| 1 | Distortion | subcats | Mono, Stereo, Legacy |
| 2 | Dynamics | subcats | Mono, Stereo, Legacy |
| 3 | EQ | subcats | Mono, Stereo |
| 4 | Modulation | subcats | Mono, Stereo, Legacy |
| 5 | Delay | subcats | Mono, Stereo, Legacy |
| 6 | Reverb | subcats | Mono, Stereo, Legacy |
| 7 | Pitch/Synth | subcats | Mono, Stereo, Legacy |
| 8 | Filter | subcats | Mono, Stereo, Legacy |
| 9 | Wah | subcats | Mono, Stereo |
| 11 | Amp | subcats | Guitar, Bass |
| 12 | Preamp | subcats | Guitar, Bass, Mic |
| 13 | Cab | subcats | Single, Dual, Single Legacy, Dual Legacy |
| 14 | IR | subcats | Single, Dual |
| 15 | Volume/Pan | subcats | Mono, Stereo |
| 16 | Send/Return | subcats | Mono, Stereo |
| 17 | Looper | subcats | Mono, Stereo |
| 18 | Input | models (4) | — |
| 19 | Output | models (4) | — |
| 20 | Split | models (4) | — |
| 21 | Merge | models (1) | — |
| 22 | Connected Devices | models (3) | — |
| 23 | Favorites | *empty* | — |

### 2.2 Subcategory

Exactly two keys plus the model list **[C]**:

```json
{ "name": "Mono", "id": 4, "models": [ ... ] }
```

Subcategory `id` is **globally unique across the whole catalog**, not per-category
— Distortion's Mono/Stereo/Legacy are 4/5/6 **[C]**. This matters because
`use_subcategory` (below) references it by that global id.

### 2.3 Model entry

872 model entries exist across all categories, but only **679 distinct `id`
values** — 193 ids appear more than once **[C]**. The duplicates are cross-listings
(a model that is both Mono and Stereo appears in both subcategories).

**Full entry** (the canonical listing):

```json
{
  "id": "HD2_DistKinkyBoost",
  "name": "Kinky Boost",
  "image": "FX_HX_DIST_KinkyBoost.png",
  "params": [
    { "Drive": null },
    { "Boost": null },
    { "Bright": null }
  ]
}
```

**Cross-reference entry** — the same model as listed under `Stereo` (subcategory 5):

```json
{ "id": "HD2_DistKinkyBoost", "use_subcategory": 4 }
```

`use_subcategory: 4` means "the full record lives in subcategory 4". A reader must
resolve these — 146 entries carry `use_subcategory` **[C]**. 726 of 872 entries
carry `name`, and the 146 without it are **exactly** the 146 cross-references —
verified as an identity, not just a count match **[C]**. So the rule is simply:
*an entry with `use_subcategory` has no other content; an entry without it is
complete.*

Cross-referencing is not the only way a model gets listed twice, though: 47 ids
carry a **full record in two different subcategories** (e.g.
`HD2_TremoloOpticalTrem` appears complete under both Modulation/Mono and
Modulation/Stereo) **[C]**. A reader that builds an id-keyed map must decide
whether to keep the first or last; the records are equivalent for parameter
purposes since ranges live in `.models`, not here.

**Entry with pages** (the only model in the catalog with `page_count`) **[C]**:

```json
{
  "id": "HD2_ImpulseResponse1024Dual",
  "name": "IR 1024",
  "image": "FX_HX_IR_Dual_1024.png",
  "params": [
    { "Index": "IR Select A" },
    { "LowCut": "Low Cut A" },
    { "HighCut": "High Cut A" },
    { "A Level": "Level A" },
    { "A Pan": "Pan A" },
    { "A Polarity": "Polarity A" },
    { "Index_1": "IR Select B" },
    { "LowCut_1": "Low Cut B" },
    { "HighCut_1": "High Cut B" },
    { "Level_1": "Level B" },
    { "B Pan": "Pan B" },
    { "B Polarity": "Polarity B" },
    { "Delay": null },
    { "Mix": null }
  ],
  "page_count": 3,
  "page_names": [ "IR A", "IR B", "Both" ],
  "param_pages": {
    "Index": 0, "LowCut": 0, "HighCut": 0,
    "A Level": 0, "A Pan": 0, "A Polarity": 0,
    "Index_1": 1, "LowCut_1": 1, "HighCut_1": 1,
    "Level_1": 1, "B Pan": 1, "B Polarity": 1,
    "Delay": 2, "Mix": 2
  }
}
```

#### Model entry field reference **[C]** (counts out of 872 entries)

| Field | n | Type | Meaning |
|---|---|---|---|
| `id` | 872 | string | Symbolic model id, e.g. `HD2_DistKinkyBoost`. **This is the model's identity.** |
| `name` | 726 | string | Display name |
| `image` | 726 | string | Icon filename resolved under `icons_models/` **[C]** — 725 of 726 exist verbatim there (the exception, `icon-input-category.png`, lives elsewhere) |
| `params` | 725 | array | Display *order* + display-name overrides — see below |
| `use_subcategory` | 146 | int | Cross-reference to the subcategory holding the full record |
| `stereo` | 49 | bool | Marks the entry as the stereo listing |
| `bass` | 24 | bool | Bass-oriented model (used to filter Bass subcategories) **[I]** |
| `hidden` | 22 | bool | Not shown in the browser **[I]** |
| `meterInterval` | 17 | float | Metering refresh/scale hint; co-occurs with `meterChannels`/`meterMin`/`meterMax` in `.models` **[I]** |
| `image_native` | 8 | string | Alternate icon for Helix Native **[I]** |
| `page_count`, `page_names`, `param_pages` | 1 each | — | Multi-page parameter UI |

#### The catalog `params` array is presentation, not definition

Each element is **either**:

- an object `{ "<paramSymbolicID>": <displayNameOverride|null> }` — `null` means
  "use the name from the `.models` definition"; a string overrides it
  (e.g. `{"Bass": "Bass Cut"}` on `HD2_DistTeemah`) **[C]**; or
- a **list** of such objects, meaning "these parameters share one UI cell / knob
  group" (159 such groups exist) **[C]**. Example from `HD2_TremoloOpticalTrem`:

```json
"params": [
  [ { "TempoSync1": null }, { "SyncSelect1": "Note Sync" }, { "Speed": null } ],
  { "Intensity": null },
  { "Level": null }
]
```

Ranges, defaults and types are **not** here — they are in the `.models` files.

---

## 3. Parameter definitions (`*.models` / `HelixModelDefs.bin`)

This is the part a third-party editor actually needs.

Each `.models` file is a JSON **array** of model records. 681 records total,
keyed by `symbolicID` — the same string used as `id` in the catalog and as
`@model` in `.hlx` presets **[C]**.

### 3.1 Verbatim example — a small, complete record

From `volumepan.models`:

```json
{
  "symbolicID": "HD2_VolPanGain",
  "mono": true,
  "stereo": true,
  "name": "Gain",
  "category": 17,
  "load": 0.35,
  "load_stereo": 0.51,
  "params": [
    {
      "symbolicID": "Gain",
      "name": "Gain",
      "valueType": 1,
      "displayType": "volume",
      "min": -120.0,
      "max": 12.0,
      "default": 0.0,
      "assign": 2
    },
    {
      "symbolicID": "@enabled",
      "name": "Enabled",
      "valueType": 2,
      "min": false,
      "max": true,
      "default": true
    },
    {
      "symbolicID": "@stereo",
      "name": "Stereo",
      "valueType": 2,
      "min": false,
      "max": true,
      "default": false
    }
  ]
}
```

From `gate.models`, showing metering fields and a discrete-ish control:

```json
{
  "symbolicID": "HD2_GateNoiseGate",
  "mono": true,
  "stereo": true,
  "name": "Noise Gate",
  "category": 4,
  "load": 1.5,
  "load_stereo": 1.9,
  "meterChannels": 1,
  "meterMin": -90.0,
  "meterMax": 0.0,
  "params": [
    { "symbolicID": "Threshold", "name": "Threshold", "valueType": 1,
      "displayType": "volume", "min": -96.0, "max": 0.0, "default": -48.0, "assign": 1 },
    { "symbolicID": "Decay", "name": "Decay", "valueType": 1,
      "displayType": "comp_decay_10_1000", "min": 0.01, "max": 1.0, "default": 0.5, "assign": 2 },
    { "symbolicID": "Level", "name": "Level", "valueType": 1,
      "displayType": "volume", "min": -60.0, "max": 6.0, "default": 0.0, "assign": 4 },
    { "symbolicID": "@enabled", "name": "Enabled", "valueType": 2,
      "min": false, "max": true, "default": true },
    { "symbolicID": "@stereo", "name": "Stereo", "valueType": 2,
      "min": false, "max": true, "default": false }
  ]
}
```

### 3.2 Model record fields **[C]** (counts out of 681)

| Field | n | Type | Meaning |
|---|---|---|---|
| `symbolicID` | 681 | string | Model identity |
| `params` | 681 | array | Parameter definitions (below) |
| `name` | 677 | string | Display name |
| `category` | 662 | int | Category taxonomy — **different numbering from the catalog's category ids**, see §3.6 |
| `load` | 645 | float | DSP cost, mono. Used for the "DSP usage" bar **[I]** |
| `load_stereo` | 186 | float | DSP cost when the block is stereo **[I]** |
| `load_320` | 2 | float | DSP cost override for firmware ≥ 3.20 **[I]** |
| `devices` | 391 | array | Device availability + minimum firmware — see §5 |
| `exclude_devices` | 1 | array | Negative availability list (one model excludes `0x210006`) |
| `mono` | 210 | bool | Model can run mono |
| `stereo` | 210 | bool | Model can run stereo |
| `cablink` | 111 | string | Default cab model paired with this amp |
| `ircablink` | 111 | string | Default cab+mic-IR model paired with this amp |
| `capEdge` | 92 | float | Amp-model-specific tone constant, values ~0.16–0.32 **[U]** — meaning not determined |
| `meterChannels` / `meterMin` / `meterMax` | 17 each | int/float | Metering geometry and dB range |
| `name_stereo` | 1 | string | Alternate name in stereo |

### 3.3 Parameter record fields **[C]** (counts out of 6 861 parameters)

| Field | n | Required | Meaning |
|---|---|---|---|
| `symbolicID` | 6861 | yes | Parameter key. Names beginning with `@` are **block-structural** (`@enabled`, `@stereo`, `@trails`, `@bypassvolume`) rather than DSP parameters **[C]** |
| `name` | 6861 | yes | Display name |
| `valueType` | 6861 | yes | 0/1/2/3 — see §3.4 |
| `min` | 6861 | yes | Inclusive minimum, typed per `valueType` |
| `max` | 6861 | yes | Inclusive maximum, typed per `valueType` |
| `default` | 6861 | yes | Default value, typed per `valueType` |
| `displayType` | 6028 | no | Key into `HelixControls.json` — formatting, units, step, enum labels |
| `assign` | 1611 | no | Integer 1–9, see §3.5 |
| `stereo-only` | 72 | no | Parameter exists only when the block is stereo |
| `default_stereo` | 37 | no | Default override when stereo |
| `max_stereo` | 11 | no | Max override when stereo |
| `displayType_stereo` | 9 | no | `displayType` override when stereo |
| `min_370` | 33 | no | `min` override on firmware ≥ 3.70 **[I]** |
| `max_315` | 16 | no | `max` override on firmware ≥ 3.15 **[I]** |

The `_<version>` suffix convention is read as a three-digit firmware version
(`370` → 3.70, `315` → 3.15) **[I]**. It is consistent with the `version` strings
in the `devices` array (§5), which are firmware versions in the same family. A
reader that ignores these suffixes will simply clamp a few parameters slightly too
tightly on new firmware.

Verbatim examples of each override form **[C]**:

```json
{ "symbolicID": "Delay", "name": "Delay", "valueType": 1,
  "displayType": "dualCab_time_ms_withauto",
  "min": 0.0, "min_370": -2e-05, "max": 0.05, "default": 0.0 }

{ "symbolicID": "Time", "name": "Time", "valueType": 1,
  "displayType": "time_ms", "min": 0.0, "max": 2.0, "max_315": 2.5,
  "default": 0.47, "assign": 1 }

{ "symbolicID": "Time", "name": "Time", "valueType": 1,
  "displayType": "time_ms_0_8000", "displayType_stereo": "time_ms_0_4000",
  "min": 0.0, "max": 8.0, "max_stereo": 4.0, "default": 0.5, "assign": 1 }

{ "symbolicID": "Detector", "name": "Detector", "valueType": 2,
  "displayType": "detector", "min": false, "max": true,
  "default": true, "stereo-only": true }
```

### 3.4 `valueType` **[C]**

Determined by checking the Python type of `min`/`max`/`default` across all 6 861
parameters — the correlation is exact, with no exceptions:

| `valueType` | n | `min`/`max`/`default` type | Interpretation |
|---|---|---|---|
| 0 | 533 | int | **Integer / enumeration.** `min`..`max` is an inclusive index range; 500 of the 533 point at a `displayType` marked `isDiscrete` whose `format` is a label array |
| 1 | 5108 | float | **Continuous.** Never `isDiscrete`. `min`..`max` are in the parameter's own units (dB, seconds, normalised 0..1, …) as decided by `displayType` |
| 2 | 1217 | bool | **Boolean.** Always `min:false, max:true` |
| 3 | 3 | string | **String.** Only on `@global_params`: `@topology0`, `@topology1`, `@cursor_group`. `min`/`max`/`default` are all `""` |

Four `valueType: 1` parameters have an integer-typed `default` alongside float
`min`/`max` — treat `default` as a float **[C]**.

Note that `valueType: 1` with an `isDiscrete` display type never occurs, but
`valueType: 2` with `isDiscrete` occurs 395 times (booleans rendered as a two-item
segmented control, e.g. `off_on`) **[C]**.

### 3.5 `assign`

Integer 1–9, present on 1 611 parameters, unique within a model for all but 3 of
the 681 models **[C]**. It is stable across models for equivalent controls —
across amps, `Drive`=1, `Bass`=3, `Mid`=4, `Treble`=5, `Presence`/`HighMid`=6,
`Master`=7, `ChVol`=8 **[C]**. It is **not** the display order (that is the
catalog `params` array) and **not** the DSP parameter index (that is `Helix.sym`).

**[U]** The exact device-side meaning is not determined from these files. It is
most plausibly the slot index used for quick controller/footswitch assignment,
but nothing in the resources proves it.

### 3.6 `category` in `.models` vs `id` in the catalog

Two different numberings; do not conflate them **[C]**:

| `.models` `category` | source file(s) |
|---|---|
| 1 | amp.models (111), preamp.models (2) |
| 2 | cab.models |
| 3 | distortion.models |
| 4 | compressor.models, gate.models |
| 5 | pitch-synth.models (1) |
| 6 | filter.models |
| 7 | pitch-synth.models (30) |
| 8 | modulation.models |
| 9 | delay.models |
| 10 | reverb.models |
| 11 | wah.models |
| 12 | sendreturn.models |
| 13 | preamp.models (111) |
| 14 | eq.models |
| 15 | fixed.models — loopers |
| 16 | fixed.models — impulse responses |
| 17 | volumepan.models |
| 19 | cabmicirs.models, cabmicirswithpan.models |

For UI grouping, use the catalog tree; `.models` `category` is best treated as an
internal tag **[I]**.

---

## 4. `HelixControls.json` — display, units, step, enum labels

301 entries **[C]**. `displayType` on a parameter is a key into this map. Keys
observed on the control objects, with counts:

| Key | n | Meaning |
|---|---|---|
| `format` | 239 | Either a printf format string, an **array of labels** (enum), or an array of range-scoped format objects |
| `isDiscrete` | 193 | Value snaps to integers |
| `controlType` | 105 | Always `"segmented"` where present **[C]** |
| `alias` | 58 | Delegate to another control definition (`{"alias": "eq_low_cut"}`) — resolve recursively |
| `step` | 53 | `{fine, coarse}` or an array of range-scoped `{lowerBound, upperBound, fine, coarse}` |
| `displayToWidgetScale` | 39 | Display-value → widget-position scale |
| `dspToDisplayScale` | 29 | **DSP value → display value multiplier** |
| `canDisplayHigherRes` | 17 | Allow extra decimal places |
| `formatUnits` | 9 | Format string including the unit suffix |
| `allowDiscreteStates` | 7 | Enum where individual states can be independently enabled |
| `minimumValue` / `maximumValue` | 4 each | Display-domain clamp, independent of the parameter's own `min`/`max` |
| `dspToDisplayIntegerOffset` | 3 | Added to the integer before display (e.g. 1-based enums) |
| `zeroValue` | 1 | Detent centre (only `pan`) |

Verbatim examples covering each shape **[C]**:

```json
"generic_knob": { "dspToDisplayScale": 10, "displayToWidgetScale": 10,
                  "format": "%.1f", "step": { "fine": 0.1, "coarse": 1.0 } }

"percent": { "dspToDisplayScale": 100, "format": "%.0f", "formatUnits": "%.0f %%",
             "step": { "fine": 1.0, "coarse": 10.0 } }

"off_on": { "isDiscrete": true, "controlType": "segmented",
            "format": ["Off", "On"] }

"wave_shape": { "isDiscrete": true,
                "format": ["Saw Up","Saw Down","Triangle","Sine","Square",
                           "Inverse Sine","Random"] }

"cab_low_cut": { "alias": "eq_low_cut" }

"integer_slider_1based": { "isDiscrete": true, "dspToDisplayIntegerOffset": 1 }

"pan": { "minimumValue": -100, "maximumValue": 100, "zeroValue": 0.0,
         "format": [
           { "lowerBound": -99999, "upperBound": -0.5, "format": "%.0f",
             "formatUnits": "Left %.0f", "unitsMultiplier": -1 },
           { "lowerBound": -0.5, "upperBound": 0.5, "format": "%.0f",
             "formatUnits": "Center" },
           { "lowerBound": 0.5, "upperBound": 999999, "format": "%.0f",
             "formatUnits": "Right %.0f" } ],
         "step": { "fine": 1.0, "coarse": 10.0 } }

"frequency": { "canDisplayHigherRes": false, "displayToWidgetScale": 10,
  "step": [ { "lowerBound": 0.0, "upperBound": 20.0, "fine": 0.1, "coarse": 1 },
            { "lowerBound": 20.0, "upperBound": 100.0, "fine": 1, "coarse": 10 },
            { "lowerBound": 100.0, "upperBound": 1000.0, "fine": 1, "coarse": 10 },
            { "lowerBound": 1000.0, "upperBound": 99999.0, "fine": 100, "coarse": 1000 } ],
  "format": [ { "lowerBound": 0, "upperBound": 20, "format": "%.1f", "formatUnits": "%.1f Hz" },
              { "lowerBound": 20, "upperBound": 1000, "format": "%.0f", "formatUnits": "%.0f Hz" },
              { "lowerBound": 1000, "upperBound": 999999, "unitsMultiplier": 0.001,
                "format": "%.0f", "formatUnits": "%.1f kHz" } ] }
```

**Rendering algorithm** (reconstructed from field names and consistency with the
observed data — **[I]**, not proven):

1. Resolve `alias` chains.
2. `display = dsp * (dspToDisplayScale ?? 1) + (dspToDisplayIntegerOffset ?? 0)`.
3. Clamp to `minimumValue`/`maximumValue` when present.
4. If `format` is an array of strings → the value is an index into it; the label
   is the display text.
5. If `format` is an array of objects → pick the entry whose
   `[lowerBound, upperBound)` contains the display value; apply its
   `unitsMultiplier` if present; render with `formatUnits` (or `format`).
6. Otherwise `format`/`formatUnits` is a printf string applied directly.

`step.fine`/`step.coarse` are the increments for fine (modifier-held) and coarse
knob movement, in *display* units **[I]**.

Enum length note: for a `valueType: 0` parameter, `max - min + 1` should equal the
length of the resolved label array. Worth asserting in a reader, but not verified
exhaustively here **[U]**.

---

## 5. Device IDs

Device ids appear in three places: `data.device` in `.hlx` presets, the `devices`
arrays in `.models`, and `menu_item id` attributes in `prefsDialog.xml` **[C]**.

The encoding is `(family << 16) | member` over the Line 6 SysEx device family
identifiers already documented in `PROTOCOL.md` — family `0x0021`, so all HX ids
have the form `0x0021_XXXX` **[C]**. This is corroborated by `HX Edit.prefs`, which
keys per-device UI settings under the string `"0x00210006"` for a connected
HX Stomp.

Exactly eight ids appear anywhere in the resources **[C]**:

| id (dec) | id (hex) | Device | Evidence |
|---|---|---|---|
| 2162689 | `0x210001` | **Helix Floor** | **[C]** arm64 disassembly: `L6Device::isDeviceHelixFX()` → id `0x210001` and the display-name global holding `"Helix Floor"` (3 independent call sites at `0x10003f5fc`, `0x10003f7a8`, `0x10003f924`). Also the `device` value of `default_preset.hlx`. |
| 2162690 | `0x210002` | **Helix Rack** | **[C]** same code: `L6Device::isDeviceHelixRack()` → `0x210001 + 1`, name `"Helix Rack"` |
| 2162692 | `0x210004` | **Helix LT** | **[C]** same code: `L6Device::isDeviceHelixLT()` → `0x210001 + 3`, name `"Helix LT"` |
| 2162693 | `0x210005` | **HX Effects** | **[C]** `prefsDialog.xml` `<menu_item id="2162693"><label>HX Effects</label>`; `HelixFx_AppDSPFlow*` models list exactly `[0x210005]`; `default_preset_hfx.hlx` has `device: 2162693` |
| 2162694 | `0x210006` | **HX Stomp** | **[C]** `prefsDialog.xml` `<menu_item id="2162694"><label>HX Stomp</label>`; `default_preset_hxs.hlx` has `device: 2162694`; matches the `"0x00210006"` prefs key of the connected HX Stomp |
| 2162699 | `0x21000B` | **HX Stomp XL** | **[I]** the only remaining HX Edit-supported hardware; shares `HelixStomp_AppDSPFlowInput` / `...OutputMain` / `...OutputSend` with `0x210006` and appears with it in a single `{0x210005, 0x210006, 0x21000B}` range test at `0x100003ad0` |
| 2162944 | `0x210100` | **Helix Native**, Helix-hardware compatibility mode | **[C]** `prefsDialog.xml` `<menu_item id="2162944"><label>Helix Floor/Rack/LT</label>` in the *Hardware Compatibility Mode* menu; `HelixPlugin_AppDSPFlow*` list exactly `[0x210100, 0x210101]` |
| 2162945 | `0x210101` | **Helix Native**, compatibility off | **[C]** `<menu_item id="2162945"><label>Off</label>` in the same menu |

`0x210003` is absent from every resource. **[I]** it is Helix Control, the
foot-controller accessory — HX Edit knows the strings `"Helix Control"` /
`"HELIX CONTROL"` and has an `L6Device::isDeviceHelixControl()`, but the device has
no DSP so no model lists it.

Notes on the 391 models that carry an explicit `devices` list **[C]**:

- The two Helix Native ids reference an identical model set (386 each), a strict
  superset of the Helix Floor set (384) — the extra two are
  `HelixPlugin_AppDSPFlow1Input` and `HelixPlugin_AppDSPFlowOutput`.
- The Helix Rack set is identical to the Helix Floor set (384).
- Helix LT, HX Stomp and HX Stomp XL are 375 each; HX Effects is the smallest at
  352.

### 5.1 The `devices` array

```json
"devices": [
  { "id": 2162944, "version": "0x03190100" },
  { "id": 2162945, "version": "0x03190100" },
  { "id": 2162693, "version": "0x03190100" },
  { "id": 2162699, "version": "0x03190100" },
  { "id": 2162694, "version": "0x03190100" },
  { "id": 2162689, "version": "0x03190100" },
  { "id": 2162690, "version": "0x03190100" },
  { "id": 2162692, "version": "0x03190100" }
]
```

Presence of an `{id}` object = the model is available on that device. The optional
`version` is the **minimum firmware** on which the model exists, formatted
`0xMMmmpprr` where `0x03190100` reads as 3.19 **[I]** — the same encoding family as
the `03 50` bytes in the SysEx identity reply documented in `PROTOCOL.md`, and
consistent with the `min_370` / `max_315` parameter suffixes.

**[U]** Whether the last two bytes are patch/build, and whether `0x19` is decimal
19 or BCD, is not settled by these files. As with the firmware-revision question in
`PROTOCOL.md`, this needs a device at a known version.

**290 of 681 models have no `devices` key at all** **[C]**. The key appears only on
models that post-date the original shipping set **[I]** — which is why a list often
mixes versioned and unversioned entries:

```json
[ {"id": 2162944}, {"id": 2162945}, {"id": 2162699},
  {"id": 2162694, "version": "0x02790000"},
  {"id": 2162693, "version": "0x02790000"},
  {"id": 2162689, "version": "0x02790000"}, ... ]
```

Read as: the model is present on Helix Native and HX Stomp XL from their first
firmware (those products shipped after 2.79), but on HX Stomp / HX Effects / Helix
Floor it requires firmware ≥ 2.79 **[I]**.

**Do not treat `devices` as "this model is selectable on this device."** Two facts
block that reading **[C]**:

- Genuine hardware filtering does occur — `HD2_FXLoopMono3` lists only
  `{0x210001, 0x210002, 0x210100, 0x210101}`, i.e. only the units with four FX
  loops, and 9 send/return models do the same.
- But 38 of the 111 amp models list HX Effects (`0x210005`), and HX Effects has no
  Amp block at all. Only 11 amp models exclude it.

**[U]** The most plausible reconciliation is that `devices` records which devices'
*firmware* carries the model's DSP code (a shared codebase ships more than the UI
exposes), while the user-visible gate is the category/block-type the device
supports. This is not proven. A reader should use `devices` for firmware-version
gating and for the send/return-style capability cases, and rely on the catalog
category tree for what the user may actually place.

---

## 6. The `.hlx` preset format

Plain UTF-8 JSON, no wrapper. Top level **[C]**:

```json
{ "version": 6, "data": { "meta": {...}, "device": 2162689, "tone": {...} } }
```

`version: 6` in all four shipped templates **[C]**.

### 6.1 `data.meta`

```json
{
  "name": "New Preset",
  "application": "Helix Edit",
  "build_sha": "30848a7",
  "modifieddate": 1478293021,
  "appversion": 327680
}
```

`empty_preset.hlx` carries only `{"name": "New Preset"}`, so every field except
`name` is optional **[C]**. `modifieddate` is a Unix timestamp (1478293021 =
2016-11-04) **[C]**. `appversion` 327680 = `0x050000` **[I]** — presumably a
packed editor version.

### 6.2 `data.device`

The device id from §5. It identifies the model of hardware the preset targets and
therefore which I/O block symbols and which model subset are legal **[C]**.

### 6.3 `data.tone`

Keys observed in the templates **[C]**: `dsp0`, `dsp1`, `global`,
`snapshot0`…`snapshot7`.

Note `default_preset.hlx` has `snapshot1`…`snapshot6` only, while
`empty_preset.hlx` has `snapshot0`…`snapshot7` — so the snapshot set is sparse and
a reader must not assume all eight are present **[C]**. There are 8 snapshots
maximum **[I]** (`snapshot%d` format string, `SNAPSHOT %d` default name, and the
templates never exceed index 7).

### 6.4 `data.tone.dspN`

Two DSPs (`dsp0`, `dsp1`) on every template, including the single-DSP HX Stomp
template **[C]**. Fixed-name slots plus numbered block slots:

| Key | Present in templates | Purpose |
|---|---|---|
| `inputA`, `inputB` | yes | Path A / path B input |
| `outputA`, `outputB` | yes | Path A / path B output |
| `split` | yes | Path splitter |
| `join` | yes | Path mixer |
| `block0`, `block1`, … | **no** (templates are empty) | Effect block slots |

The `blockN` naming is **[C]** from the app binary, which contains the literals
`block%d`, `block0`, `block1`, `blocks.dsp%d.block%d`, `blocks.dsp%d.%s` and
`blocks.dsp%.split` (their typo). None of the four shipped templates contains a
populated block, so the block object's exact key set could not be observed
directly — but the same string region enumerates the preset key vocabulary:

```
@model  @position  @path  @enabled  bypass  @input  @output
@stereo  @trails  @cab  @type  @favorite  @uuid  @uuid2
@no_snapshot_bypass  category  cab  block
```

**[I]** A populated block is therefore an object of the form
`{"@model": "<symbolicID>", "@position": <int>, "@path": <int>, "@enabled": <bool>,
…parameters by symbolicID…}`.

Confirmed slot examples from `default_preset.hlx` **[C]**:

```json
"split": {
  "@model": "HD2_AppDSPFlowSplitY",
  "@enabled": true,
  "bypass": false,
  "@position": 0,
  "BalanceA": 0.5,
  "BalanceB": 0.5
},
"inputA": {
  "@model": "HD2_AppDSPFlow1Input",
  "@input": 0,
  "noiseGate": false,
  "decay": 0.5,
  "threshold": -48
},
"join": {
  "@model": "HD2_AppDSPFlowJoin",
  "@position": 8,
  "@enabled": true,
  "A Level": 0, "A Pan": 0.5,
  "B Level": 0, "B Pan": 0.5, "B Polarity": false,
  "Level": 0
},
"outputA": {
  "@model": "HD2_AppDSPFlowOutput",
  "@output": 1,
  "pan": 0.5,
  "gain": 0
}
```

Observations **[C]**:

- **A block references its model by the `@model` string**, which is exactly the
  `symbolicID` / catalog `id`. There is no numeric model reference in the format.
- Every other key in the object is a parameter `symbolicID` from that model's
  `.models` record, with a raw DSP-domain value. Compare `inputA`'s `threshold:
  -48` against `HD2_AppDSPFlow1Input`'s `threshold` definition, and `join`'s
  `"A Pan": 0.5` against the `pan` display type.
- Parameter keys **may contain spaces** (`"A Level"`, `"B Polarity"`) — they are
  the literal `symbolicID` values, not identifiers.
- `@position` is the column index along the signal path; `split` sits at 0 and
  `join` at 8 in the stock templates. **[I]** the block grid is 0–8 wide per DSP
  on Helix Floor.
- `@enabled` = the block is *on* (not bypassed). A separate `bypass` boolean also
  appears on `split`. **[U]** the precise division of labour between `@enabled`
  and `bypass` is not determined; `@enabled` is the one defined as a parameter in
  every `.models` record, so treat it as authoritative and `bypass` as
  split-specific.
- `@input` / `@output` are integer selectors into the device's physical I/O, whose
  labels come from `HelixControls.json` `input_type` / `input_type_lt` /
  `input_type_native` (which is why those three exist) **[I]**.

The device-specific input/output models are chosen by `data.device` **[C]** — the
selection logic is visible verbatim in the disassembly at `0x100003b08`:
`0x210005` → `HelixFx_AppDSPFlowInput`, `0x210006` and `0x21000B` →
`HelixStomp_AppDSPFlowInput`, everything else → `HD2_AppDSPFlow1Input`.

### 6.5 `data.tone.global`

Verbatim from `default_preset.hlx` **[C]**:

```json
{
  "@model": "@global_params",
  "@tempo": 120,
  "@pedalstate": 2,
  "@guitarinputZ": 0,
  "@current_snapshot": 0,
  "@topology0": "A",
  "@topology1": "A",
  "@cursor_dsp": 0,
  "@cursor_path": 0,
  "@cursor_position": 0,
  "@cursor_group": "",
  "@variax_model": 0,
  "@variax_volumeknob": -0.1,
  "@variax_toneknob": -0.1,
  "@variax_lockctrls": 0,
  "@variax_customtuning": true,
  "@variax_magmode": true,
  "@variax_str1tuning": 0,
  "@variax_str2tuning": 0,
  "@variax_str3tuning": 0,
  "@variax_str4tuning": 0,
  "@variax_str5tuning": 0,
  "@variax_str6tuning": 0
}
```

`@model: "@global_params"` points at the `@global_params` pseudo-model in
`fixed.models`, which defines every one of these keys with a type and range —
so the global block is validated the same way as any other block **[C]**. This is
also where the three `valueType: 3` (string) parameters live.

`@topology0` / `@topology1` are the per-DSP routing topology **[C]**. Observed
value `"A"`; the app binary contains the complete alphabet of topology strings
`"A"`, `"AB"`, `"ABJ"`, `"SAB"`, `"SABJ"` **[C]** — read as S=split, A=path A,
B=path B, J=join **[I]**.

`fixed.models` also defines the pseudo-models `@dt`, `@powercab` and `@variax`,
matching the binary's `dt0`/`dt1`/`dtdual`, `powercab0`/`powercab1`/`powercabdual`
and `variax` preset keys — external L6 Link device state **[I]**.

### 6.6 `data.tone.snapshotN`

```json
{ "@name": "SNAPSHOT 2", "@tempo": 120, "@pedalstate": 2, "@ledcolor": 0 }
```

The binary additionally references `@valid` in the snapshot key group **[C]**, so a
real snapshot may carry it. **[U]** How per-snapshot *parameter* values are stored
could not be determined — no shipped template has a modified snapshot. The binary's
`@snapshot_disable` and `@no_snapshot_bypass` keys indicate per-parameter and
per-block snapshot opt-outs exist **[C]**.

### 6.7 Sections not present in the templates

The app binary enumerates three more preset sections that none of the four
templates exercises **[C]**:

- **`@assignments`** — controller assignments, with keys `@param`, `@min`, `@max`,
  `@controller`, `@globaldsp`, `@globalblock`, `@snapshot_disable`, and address
  paths `controllers.%s.%s.` / `controllers.dsp%d.%s.%s.`
- **`footswitch`** — `@fs_index`, `@fs_label`, `@fs_enabled`, `@fs_momentary`,
  `@fs_ledcolor`, `@fs_customcolor`, `@fs_customlabel`
- **`commands.%s.`** — Command Center: `@command`, `@cc`, `@overthresh`, `@wait`,
  `@behavior`, `@value`
- **`irUuidTable`** — maps preset-local IR slots to IR UUIDs **[I]**, related to
  the `@uuid` / `@uuid2` block keys

**[U]** Their exact JSON shape is undetermined. Obtaining a real user preset that
uses snapshots, controller assignments and an IR is the single highest-value next
step for completing this section.

### 6.8 Dot-path addressing

The format strings `blocks.dsp%d.block%d`, `blocks.dsp%d.%s`,
`controllers.dsp%d.%s.%s.`, `commands.%s.`, `dsp%d.%s.%s`, `%s.%d.` and
`global_device` **[C]** show that HX Edit addresses individual parameters by a
dotted string path, e.g. `blocks.dsp0.block2.Drive`. This is likely the same
addressing used on the wire for parameter edits — worth checking against
`tools/hxsniff` captures.

---

## 7. ID ↔ name mapping

### 7.1 There is no numeric model id

**[C]** A model's identity throughout the entire format is the **symbolic string**
(`HD2_DistKinkyBoost`). It is used as:

- `id` in `HX_ModelCatalog.json`,
- `symbolicID` in the `.models` files and `HelixModelDefs.bin`,
- `symbol` in `Helix.sym`,
- `@model` in `.hlx` presets.

All 872 catalog `id` values are strings; not one is numeric **[C]**. The only
numeric ids in these files are **category** ids, **subcategory** ids and **device**
ids. If the USB/MIDI wire protocol uses a numeric model index, that mapping is not
in these resource files **[U]** — the only implicit index available is a model's
position in `HelixModelDefs.bin` (0–680), and there is no evidence it is the wire
value.

Coverage between the two tables **[C]**:

- catalog ids not in `.models`: exactly one, the sentinel `"None"` (the empty
  block, category 0);
- `.models` symbols not in the catalog: three — `@global_params`,
  `HelixPlugin_AppDSPFlow1Input`, `HelixPlugin_AppDSPFlowOutput`.

So the useful mapping is **symbolicID → display name**, and it exists in two
places: the catalog (which also gives you the browse tree) and the `.models` files
(which also give you the parameters).

### 7.2 `Helix.sym` — the parameter *index* table

833 entries **[C]**:

```json
{ "symbol": "HD2_DelaySimpleDelayMono",
  "parameters": ["Time","Feedback","Mix","Level","SyncSelect1","TempoSync1"] }
{ "symbol": "HD2_DelaySimpleDelayStereo",
  "parameters": ["Time","Feedback","Mix","Level","Scale","SyncSelect1","TempoSync1"] }
```

This is the **firmware DSP-level** table, one entry per built DSP variant, and it
is where mono and stereo become separate symbols. The editor-level `.models` record
merges them:

```
HD2_DelaySimpleDelay   mono:true stereo:true
  params: Time, Feedback, Mix, Level, Scale, SyncSelect1, TempoSync1,
          @enabled, @trails, @stereo
  ("Scale" carries "stereo-only": true — exactly the parameter the Mono
   symbol lacks)
```

The two tables therefore reconcile **[C]**. Relationship counts: 348 `Helix.sym`
symbols have no `.models` record (they are the `…Mono` / `…Stereo` split forms),
and 196 `.models` symbols have no `Helix.sym` entry (they are the merged forms)
**[C]**. Comparing the 485 that share a name, 376 have an identical parameter
order and 109 differ **[C]** — the differences are either stereo-only parameters
or, as with `HD2_AppDSPFlow1Input`, `Helix.sym` listing device-specific extras
(`select`, `gain`, `guitarSense`, `auxSense`, `micLowCut`) that the `.models`
record omits.

**Why this matters:** `Helix.sym` order is the closest thing in these files to a
numeric parameter address. If the wire protocol addresses parameters by index
rather than by name, `Helix.sym` is the table to try first **[I]**.

`Helix.sym` is **not** sorted alphabetically; the order looks like build order
**[C]**.

### 7.3 Extraction snippet

```python
#!/usr/bin/env python3
"""Read the model tables from a locally installed HX Edit.

Reads the user's own installed copy at runtime. Nothing is redistributed.
"""
import json
import os

RES = "/Applications/Line6/HX Edit.app/Contents/Resources"

MODEL_FILES = [
    "amp", "cab", "cabmicirs", "cabmicirswithpan", "compressor", "delay",
    "distortion", "eq", "filter", "fixed", "gate", "io", "modulation",
    "pitch-synth", "preamp", "reverb", "sendreturn", "volumepan", "wah",
]


def load_catalog(res=RES):
    """symbolicID -> {name, category, subcategory, image, params}

    Resolves `use_subcategory` cross-references so each id appears once.
    """
    doc = json.load(open(os.path.join(res, "HX_ModelCatalog.json")))
    subs = {}          # subcategory id -> (category, subcategory name)
    entries = []       # (category, subcat_name, subcat_id, model)
    for cat in doc["categories"]:
        for m in cat.get("models", []):
            entries.append((cat, None, None, m))
        for sub in cat.get("subcategories", []):
            subs[sub["id"]] = (cat, sub["name"])
            for m in sub.get("models", []):
                entries.append((cat, sub["name"], sub["id"], m))

    out = {}
    for cat, sub_name, _sub_id, m in entries:
        if "use_subcategory" in m:      # cross-reference; canonical record elsewhere
            continue
        out[m["id"]] = {
            "name": m.get("name"),
            "category_id": cat["id"],
            "category": cat["name"],
            "subcategory": sub_name,
            "image": m.get("image"),
            "params": m.get("params", []),   # display order + name overrides
        }
    return out


def load_defs(res=RES):
    """symbolicID -> full model definition (ranges, defaults, devices, load)."""
    defs = {}
    for stem in MODEL_FILES:
        for m in json.load(open(os.path.join(res, stem + ".models"))):
            defs[m["symbolicID"]] = m
    return defs


def load_controls(res=RES):
    """displayType -> control definition, with `alias` chains resolved."""
    raw = json.load(open(os.path.join(res, "HelixControls.json")))

    def resolve(key, depth=0):
        c = raw.get(key)
        if c and "alias" in c and depth < 8:
            return resolve(c["alias"], depth + 1)
        return c

    return {k: resolve(k) for k in raw}


def load_sym(res=RES):
    """firmware symbol -> ordered parameter-name list."""
    return {e["symbol"]: e["parameters"]
            for e in json.load(open(os.path.join(res, "Helix.sym")))}


if __name__ == "__main__":
    cat, defs = load_catalog(), load_defs()
    print(f"{len(cat)} catalog models, {len(defs)} definitions")
    for mid in sorted(cat)[:15]:
        d = defs.get(mid, {})
        n = len([p for p in d.get("params", [])
                 if not p["symbolicID"].startswith("@")])
        print(f"{mid:34s} {cat[mid]['name'] or '':22s} "
              f"{cat[mid]['category']:12s} {n:2d} params")
```

To read the MessagePack `.bin` files instead, remember the trailing-NUL rule:

```python
def decode_str(raw: bytes) -> str:
    # every string in the .bin files carries a NUL inside the msgpack length
    return raw.decode("utf-8").rstrip("\0")
```

In Rust, `rmp-serde` works if you deserialize strings through a wrapper that
trims one trailing `\0`; or use `rmpv::decode::read_value` and post-process.
But since the `.bin` files carry no information the JSON lacks, a Rust reader is
better off with `serde_json` on the `.json` / `.models` files **[I]**.

### 7.4 Sample rows

15 rows produced by the snippet above, showing the shape of the mapping (not a
reproduction of the catalog) **[C]**:

| category | subcategory | symbolic id | display name |
|---|---|---|---|
| None | — | `None` | (empty block) |
| Distortion | Mono | `HD2_DistKinkyBoost` | Kinky Boost |
| Distortion | Mono | `HD2_DistDerangedMaster` | Deranged Master |
| Distortion | Mono | `HD2_DistMinotaur` | Minotaur |
| Dynamics | Mono | `HD2_GateNoiseGate` | Noise Gate |
| Delay | Legacy | `HD2_DL4AnalogDelayStereoMod` | Analog w/Mod |
| Delay | Legacy | `HD2_DL4MultiheadStereo` | Multi-Head |
| Delay | Legacy | `L6BubbleEcho` | Bubble Echo |
| Delay | Mono | `HD2_DelaySimpleDelay` | Simple Delay |
| Amp | Guitar | `HD2_AmpGermanMahadeva` | German Mahadeva |
| Amp | Bass | `HD2_AmpCali400Ch1` | Cali 400 Ch1 |
| Cab | Dual | `HD2_CabMicIr_1x12CaliEXTWithPan` | 1x12 Cali EXT |
| Cab | Dual | `HD2_CabMicIr_2x12BlueBellWithPan` | 2x12 Blue Bell |
| Volume/Pan | Mono | `HD2_VolPanGain` | Gain |
| IR | Dual | `HD2_ImpulseResponse1024Dual` | IR 1024 |

Note the naming convention, useful for heuristics but **not** a substitute for the
table **[I]**: `HD2_` = the main HX model set, `VIC_` / `Victoria` / `L6SPB` /
`L6PhazeEko` / bare names (`TapeEater`, `RezSynth`, `SynthLead`) = legacy M-series
and stompbox models, `HelixStomp_` / `HelixFx_` / `HelixPlugin_` = device-specific
I/O blocks, `@`-prefixed = pseudo-models for global/external state.

---

## 8. What could not be determined

| Question | Status |
|---|---|
| Exact JSON shape of a populated `blockN` object | **[U]** — key vocabulary known from the binary, but no shipped template contains a block. Needs a real user preset. |
| How snapshots store per-parameter values | **[U]** — needs a real preset with edited snapshots |
| Shape of `@assignments`, `footswitch`, `commands`, `irUuidTable` | **[U]** — key names known, structure not |
| Meaning of `assign` (1–9) | **[U]** — behaviour characterised, semantics unproven |
| Meaning of `capEdge` (92 amp/preamp models, ~0.16–0.32) | **[U]** |
| Whether firmware `version` fields are BCD or plain hex | **[U]** — same open question as the SysEx revision bytes in `PROTOCOL.md` |
| Whether a numeric model id exists on the wire | **[U]** — absent from all resource files |
| Difference between `@enabled` and `bypass` | **[U]** |
| Exact semantics of `devices` (firmware presence vs. user-visible availability) | **[U]** — see §5.1 |
| `0x210003` = Helix Control | **[I]** — never appears in the resources |

## 9. Reproducing this analysis

Everything above is reproducible with `python3` and the standard library, except
the device-id confirmations in §5, which used:

```
otool -arch arm64 -tV "/Applications/Line6/HX Edit.app/Contents/MacOS/HX Edit"
```

then locating `L6Device::isDeviceHelixFX` / `isDeviceHelixRack` / `isDeviceHelixLT`
and reading the `movk w23, #0x21, lsl #16` immediates alongside the display-name
globals. The binary is a universal x86_64 + arm64 fat file, so the arm64 slice
starts at file offset 22 740 992 and `__TEXT` has `vmaddr 0x100000000` — a raw
byte offset into the fat file must have both subtracted and added respectively
before it can be matched against disassembly addresses.
