//! The preset document: what is actually in the signal chain.
//!
//! A preset arrives as a blob whose contents are themselves MessagePack — the
//! magic string `l6-helix`, a table of section offsets, and the tone. The tone
//! is a fixed array of slots, most of them empty on a small device.
//!
//! Slots are keyed by integers throughout, so the constants here are doing the
//! work a schema would do elsewhere. They were read off captured presets and
//! cross-checked against the model catalog: a slot holding model 101 carries
//! exactly three values, and model 101 is Scream 808, which has exactly three
//! parameters.

use crate::msgpack::{Decoder, Encoder, Value};

/// A parsed preset.
pub struct Preset {
    /// Section offset table. Not yet understood, kept so a preset can be
    /// written back unchanged.
    pub sections: Vec<u8>,
    /// Header width the section table arrived with, so it goes back unchanged.
    sections_width: u8,
    pub slots: Vec<Slot>,
    /// The whole tone map, for fields this type does not model yet.
    pub tone: Value,
}

/// Where each part of the signal path lives in the slot array.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// Each independent signal path, in order. Most devices have one; Helix and
    /// Helix LT have two, so a preset can hold four lanes once both split.
    pub paths: Vec<Path>,
}

impl Lane {
    /// Positions in this lane with nothing in them, in order — where a new
    /// block can go.
    pub fn free(&self, preset: &Preset) -> Vec<usize> {
        self.span
            .clone()
            .filter(|p| preset.slots.get(*p).is_some_and(|s| s.model.is_none()))
            .collect()
    }
}

impl Layout {
    /// Every lane across every path, which is what a renderer draws row by row.
    pub fn lanes(&self) -> impl Iterator<Item = &Lane> {
        self.paths.iter().flat_map(|p| p.lanes.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// One signal path: an input, its lanes, and an output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Path {
    pub input: Option<usize>,
    pub output: Option<usize>,
    /// The slot holding the split, when the path divides. It is not a block in
    /// the line — it is where the wiring parts — so it is kept out of the lanes.
    pub split: Option<usize>,
    pub join: Option<usize>,
    /// One lane if the path runs straight through, two if it splits.
    pub lanes: Vec<Lane>,
}

/// A single row of blocks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lane {
    /// Which branch of the split this is: 0 for the upper, 1 for the lower.
    pub branch: usize,
    /// Positions of the occupied blocks, in signal order. Empty slots are not
    /// listed; use [`Lane::free`] to find somewhere to put a new block.
    pub blocks: Vec<usize>,
    /// Every slot position this lane covers, occupied or not.
    pub span: std::ops::Range<usize>,
}

/// One position in the signal chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub kind: Kind,
    /// Model number, resolvable through `hx-catalog`'s symbol table.
    pub model: Option<u32>,
    /// A second model sharing the slot. This is how Amp+Cab works: the amp is
    /// the primary and the cab rides along with its own parameters. `None` on
    /// the great majority of blocks.
    pub paired: Option<u32>,
    pub enabled: bool,
    /// Parameter values in the order the device indexes them, which is the
    /// order `Helix.sym` lists for this model.
    pub values: Vec<f32>,
    /// The paired model's values, in its own parameter order.
    pub paired_values: Vec<f32>,
    /// Key 9. Determined by the model rather than by the slot: every amp
    /// carries 18, every EQ and dynamics block 1, and the endpoints 0. It was
    /// first read as a branch index, which it is not — the branch is implied by
    /// the slot's position, which is what [`Preset::layout`] uses. Kept because
    /// it must be written back unchanged and its meaning is still open.
    pub type_tag: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Input,
    Output,
    Split,
    Join,
    /// An effect, amp or cab.
    Block,
    Empty,
    /// A slot kind we have not identified; carried through rather than dropped.
    Unknown(i64),
}

impl Kind {
    fn from_wire(k: i64) -> Kind {
        match k {
            0 => Kind::Input,
            1 => Kind::Output,
            2 => Kind::Split,
            3 => Kind::Join,
            6 => Kind::Block,
            8 => Kind::Empty,
            other => Kind::Unknown(other),
        }
    }
}

/// Field keys inside the tone.
mod key {
    /// Metadata: DSP name, firmware, build string.
    pub const META: i64 = 7;
    /// The signal path.
    pub const PATH: i64 = 0;
    /// Slot array within the path.
    pub const SLOTS: i64 = 22;

    /// Slot kind.
    pub const KIND: i64 = 19;
    /// Slot body.
    pub const BODY: i64 = 20;

    /// Model reference on an effect slot.
    pub const MODEL_REF: i64 = 24;
    /// Model number within a model reference.
    pub const MODEL: i64 = 25;
    /// Second model in the same slot, or -1 when the slot holds only one.
    pub const PAIRED_MODEL: i64 = 26;
    /// The paired model's parameter values.
    pub const PAIRED_VALUES: i64 = 12;
    /// Where an input slot takes its signal from. Sits beside the value array
    /// rather than in it, which is why it never showed up as a parameter.
    pub const INPUT_FROM: i64 = 5;
    /// Where an output slot sends its signal.
    pub const OUTPUT_TO: i64 = 6;
    /// Model number on a split or join, which carries it directly.
    pub const INLINE_MODEL: i64 = 8;
    /// Whether the block is switched on.
    pub const ENABLED: i64 = 10;
    /// A per-model type tag whose meaning is still open. See `Slot::type_tag`.
    pub const TYPE_TAG: i64 = 9;
    /// Parameter values on an effect slot.
    pub const VALUES: i64 = 11;
    /// Parameter values on input, output, split and join slots.
    pub const IO_VALUES: i64 = 7;

    /// Split and join keep their model and values one level down.
    pub const SPLIT_BODY: i64 = 15;
    pub const JOIN_BODY: i64 = 17;

    /// Inside a value array: the count, then the values.
    pub const ARRAY_VALUES: i64 = 4;

    pub const FIRMWARE: i64 = 35;
    pub const BUILD: i64 = 37;

    /// Preset-wide settings.
    pub const SETTINGS: i64 = 5;
    /// Tempo in BPM, within the settings.
    pub const TEMPO: i64 = 16;

    /// Snapshots and footswitch assignments.
    pub const SNAPSHOT_SECTION: i64 = 10;
    pub const SNAPSHOTS: i64 = 10;
    pub const SNAPSHOT_NAME: i64 = 4;
}

impl Preset {
    pub const MAGIC: &'static str = "l6-helix";

    /// Parse the blob carried by a read-preset response.
    pub fn parse(blob: &[u8]) -> Option<Preset> {
        let mut values = Decoder::decode_all(blob).ok()?.into_iter();
        if values.next()?.as_str()? != Self::MAGIC {
            return None;
        }
        let table = values.next()?;
        let sections_width = match &table {
            Value::Bin(_, w) => *w,
            _ => 0,
        };
        let sections = table.as_raw()?.to_vec();
        let tone = values.next()?;

        let slots = tone
            .get(key::PATH)
            .and_then(|p| p.get(key::SLOTS))
            .map(collect_slots)
            .unwrap_or_default();

        Some(Preset {
            sections,
            sections_width,
            slots,
            tone,
        })
    }

    /// Firmware version, from the BCD-packed field: `0x03800000` is 3.80.
    pub fn firmware(&self) -> Option<String> {
        let raw = self.tone.get(key::META)?.get(key::FIRMWARE)?.as_i64()? as u32;
        Some(format!("{}.{:02x}", raw >> 24, (raw >> 16) & 0xff))
    }

    pub fn build(&self) -> Option<&str> {
        self.tone.get(key::META)?.get(key::BUILD)?.as_str()
    }

    /// Serialise back to the blob the device exchanges.
    ///
    /// The inverse of [`parse`](Self::parse), and what makes editing possible
    /// at all: several operations have no dedicated opcode, and the way to
    /// perform them is to change the document and write the whole thing back.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Encoder::encode(&Value::Str(Self::MAGIC.to_owned()));
        out.extend(Encoder::encode(&Value::Bin(
            self.sections.clone(),
            self.sections_width,
        )));
        out.extend(Encoder::encode(&self.tone));
        out
    }

    /// Exchange two slots, moving a block along the chain.
    ///
    /// Returns false if either position does not exist, rather than producing
    /// a document the device would reject.
    pub fn swap_slots(&mut self, a: usize, b: usize) -> bool {
        let Some(Value::Array(slots)) = self.tone.at_mut(&[key::PATH, key::SLOTS]) else {
            return false;
        };
        if a >= slots.len() || b >= slots.len() {
            return false;
        }
        slots.swap(a, b);
        self.slots.swap(a, b);
        true
    }

    /// Set the tempo, in BPM.
    ///
    /// There is no dedicated opcode for this, so the caller writes the whole
    /// document back afterwards. That is only safe because re-encoding is
    /// byte-exact — see `tests/roundtrip.rs`.
    pub fn set_tempo(&mut self, bpm: f32) -> bool {
        match self.tone.at_mut(&[key::SETTINGS, key::TEMPO]) {
            Some(slot) => {
                *slot = Value::F32(bpm);
                true
            }
            None => false,
        }
    }

    /// Rename a snapshot.
    pub fn set_snapshot_name(&mut self, index: usize, name: &str) -> bool {
        let Some(Value::Array(entries)) =
            self.tone.at_mut(&[key::SNAPSHOT_SECTION, key::SNAPSHOTS])
        else {
            return false;
        };
        match entries
            .get_mut(index)
            .and_then(|e| e.get_mut(key::SNAPSHOT_NAME))
        {
            Some(slot) => {
                *slot = Value::Str(name.to_owned());
                true
            }
            None => false,
        }
    }

    /// Tempo in BPM.
    pub fn tempo(&self) -> Option<f32> {
        self.tone.get(key::SETTINGS)?.get(key::TEMPO)?.as_f32()
    }

    /// Snapshot names, in order.
    pub fn snapshots(&self) -> Vec<String> {
        let Some(Value::Array(entries)) = self
            .tone
            .get(key::SNAPSHOT_SECTION)
            .and_then(|s| s.get(key::SNAPSHOTS))
        else {
            return Vec::new();
        };
        entries
            .iter()
            .map(|e| {
                e.get(key::SNAPSHOT_NAME)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    /// How the slots are wired together.
    ///
    /// The slot array is a fixed topology, not a running order: the input, the
    /// upper path's blocks, the output, the split, the lower path's blocks and
    /// the join each occupy known positions. Reading it as a flat list puts the
    /// split and join at the end, which is how they came to be drawn in the
    /// wrong place.
    ///
    /// Positions are derived from the slot kinds rather than hard-coded, so a
    /// device with a different number of block slots still works.
    pub fn layout(&self) -> Layout {
        let mut paths: Vec<Path> = Vec::new();

        for (position, slot) in self.slots.iter().enumerate() {
            match slot.kind {
                // An input opens a path. Its blocks accumulate into the first
                // lane until something says otherwise.
                Kind::Input => paths.push(Path {
                    input: Some(position),
                    lanes: vec![Lane {
                        branch: 0,
                        blocks: Vec::new(),
                        span: position + 1..position + 1,
                    }],
                    ..Path::default()
                }),
                Kind::Output => {
                    if let Some(path) = paths.last_mut() {
                        path.output = Some(position);
                    }
                }
                // A split adds the second lane. It appears after the output in
                // the slot array even though the signal reaches it first, which
                // is why reading the array as a running order goes wrong.
                Kind::Split => {
                    if let Some(path) = paths.last_mut() {
                        path.split = Some(position);
                        path.lanes.push(Lane {
                            branch: 1,
                            blocks: Vec::new(),
                            span: position + 1..position + 1,
                        });
                    }
                }
                Kind::Join => {
                    if let Some(path) = paths.last_mut() {
                        path.join = Some(position);
                    }
                }
                Kind::Block | Kind::Empty | Kind::Unknown(_) => {
                    if let Some(lane) = paths.last_mut().and_then(|p| p.lanes.last_mut()) {
                        lane.span.end = position + 1;
                    }
                    // Blocks belong to whichever lane is currently open: the
                    // lower one once a split has been seen, otherwise the upper.
                    // Empty slots are skipped — a lane is mostly empty slots on
                    // a small device, and they are positions, not blocks.
                    if slot.model.is_none() {
                        continue;
                    }
                    if let Some(lane) = paths.last_mut().and_then(|p| p.lanes.last_mut()) {
                        lane.blocks.push(position);
                    }
                }
            }
        }

        // A path with a split but no blocks below it is drawn as a single lane;
        // an empty second row is noise.
        for path in &mut paths {
            if path.lanes.len() > 1 && path.lanes[1].blocks.is_empty() {
                path.lanes.truncate(1);
            }
        }

        Layout { paths }
    }

    /// Where an endpoint slot is routed: which physical input it listens to,
    /// or which output it feeds.
    ///
    /// HX Edit shows this as the first control on Input and Main L/R, but the
    /// device does not send it among the parameter values — it lives beside
    /// them under its own key, so it has to be read and written separately.
    /// The number indexes the model's `@input`/`@output` menu.
    pub fn routing(&self, position: usize) -> Option<i64> {
        let key = Self::routing_key(self.slots.get(position)?.kind)?;
        self.slot_body(position)?.get(key).and_then(Value::as_i64)
    }

    /// Point an endpoint slot somewhere else. Returns false if the slot is not
    /// an input or output, or has no routing field to change.
    pub fn set_routing(&mut self, position: usize, to: i64) -> bool {
        let Some(key) = self
            .slots
            .get(position)
            .map(|s| s.kind)
            .and_then(Self::routing_key)
        else {
            return false;
        };
        let Some(field) = self.slot_body_mut(position).and_then(|b| b.get_mut(key)) else {
            return false;
        };
        // Written at the width it arrived at: a preset carries a table of byte
        // offsets into itself, so a narrower integer shifts everything after it
        // and the device reads the preset back as empty.
        *field = match field {
            Value::Wide(_, w) => Value::Wide(to as u64, *w),
            Value::WideInt(_, w) => Value::WideInt(to, *w),
            _ => Value::Int(to),
        };
        true
    }

    fn routing_key(kind: Kind) -> Option<i64> {
        match kind {
            Kind::Input => Some(key::INPUT_FROM),
            Kind::Output => Some(key::OUTPUT_TO),
            _ => None,
        }
    }

    fn slot_body(&self, position: usize) -> Option<&Value> {
        match self.tone.get(key::PATH)?.get(key::SLOTS)? {
            Value::Array(items) => items.get(position)?.get(key::BODY),
            _ => None,
        }
    }

    fn slot_body_mut(&mut self, position: usize) -> Option<&mut Value> {
        match self.tone.at_mut(&[key::PATH, key::SLOTS])? {
            Value::Array(items) => items.get_mut(position)?.get_mut(key::BODY),
            _ => None,
        }
    }

    /// Slots holding an actual effect, with their position in the chain.
    pub fn blocks(&self) -> impl Iterator<Item = (usize, &Slot)> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == Kind::Block && s.model.is_some())
    }
}

fn collect_slots(slots: &Value) -> Vec<Slot> {
    let Value::Array(items) = slots else {
        return Vec::new();
    };
    items.iter().map(read_slot).collect()
}

fn read_slot(raw: &Value) -> Slot {
    let kind = Kind::from_wire(raw.get(key::KIND).and_then(Value::as_i64).unwrap_or(-1));
    let Some(body) = raw.get(key::BODY) else {
        return Slot {
            kind,
            model: None,
            paired: None,
            enabled: false,
            values: Vec::new(),
            paired_values: Vec::new(),
            type_tag: 0,
        };
    };

    // Splits and joins nest their model and values one level deeper than
    // effects do, and inputs and outputs have no model at all.
    let body = match kind {
        Kind::Split => body.get(key::SPLIT_BODY).unwrap_or(body),
        Kind::Join => body.get(key::JOIN_BODY).unwrap_or(body),
        _ => body,
    };

    let reference = body.get(key::MODEL_REF);
    let model = reference
        .and_then(|r| r.get(key::MODEL))
        .or_else(|| body.get(key::INLINE_MODEL))
        .and_then(Value::as_i64)
        .map(|n| n as u32);

    // A paired model is written as -1 when absent, so a plain "is it there"
    // check would report every block as having a cab.
    let paired = reference
        .and_then(|r| r.get(key::PAIRED_MODEL))
        .and_then(Value::as_i64)
        .filter(|n| *n >= 0)
        .map(|n| n as u32);

    let values = body
        .get(key::VALUES)
        .or_else(|| body.get(key::IO_VALUES))
        .map(read_values)
        .unwrap_or_default();

    Slot {
        kind,
        model,
        paired,
        // Inputs, outputs and empty slots have no switch; treat them as on so
        // callers do not have to special-case them when drawing the chain.
        enabled: body
            .get(key::ENABLED)
            .is_none_or(|v| v == &Value::Bool(true)),
        values,
        paired_values: body
            .get(key::PAIRED_VALUES)
            .map(read_values)
            .unwrap_or_default(),
        type_tag: body.get(key::TYPE_TAG).and_then(Value::as_i64).unwrap_or(0),
    }
}

/// Read a `{2: count, 3: count, 4: [...]}` value array.
///
/// Booleans appear inline among the floats — a switch parameter sits in the
/// same array as the knobs — so they are folded to 0.0 and 1.0 rather than
/// dropped, which keeps positions aligned with the parameter list.
fn read_values(array: &Value) -> Vec<f32> {
    match array.get(key::ARRAY_VALUES) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::Bool(b) => *b as u8 as f32,
                other => other.as_f32().unwrap_or(0.0),
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msgpack::Encoder;

    /// Rebuild the shape of a real captured preset: a Scream 808 in slot 1
    /// with its three parameters, and an empty slot after it.
    fn sample() -> Vec<u8> {
        let slot = crate::msgmap! {
            key::KIND => Value::Int(6),
            key::BODY => crate::msgmap! {
                key::MODEL_REF => crate::msgmap! {
                    key::MODEL => Value::Int(101),
                    key::PAIRED_MODEL => Value::Int(-1),
                },
                key::ENABLED => Value::Bool(true),
                key::VALUES => crate::msgmap! {
                    2 => Value::Int(3),
                    key::ARRAY_VALUES => Value::Array(vec![
                        Value::F32(0.01), Value::F32(0.19), Value::F32(0.98),
                    ]),
                },
            },
        };
        let empty = crate::msgmap! { key::KIND => Value::Int(8), key::BODY => Value::Nil };
        let tone = crate::msgmap! {
            key::META => crate::msgmap! {
                key::FIRMWARE => Value::UInt(0x0380_0000),
                key::BUILD => Value::Str("v3.71-32-g1039661".into()),
            },
            key::PATH => crate::msgmap! {
                key::SLOTS => Value::Array(vec![slot, empty]),
            },
        };

        let mut blob = Encoder::encode(&Value::Str(Preset::MAGIC.into()));
        blob.extend(Encoder::encode(&Value::Bin(vec![0x3d, 0, 0, 0], 0)));
        blob.extend(Encoder::encode(&tone));
        blob
    }

    #[test]
    fn reads_blocks_and_their_values() {
        let preset = Preset::parse(&sample()).expect("parses");
        assert_eq!(preset.slots.len(), 2);

        let (position, block) = preset.blocks().next().expect("one block");
        assert_eq!(position, 0);
        assert_eq!(block.model, Some(101));
        assert!(block.enabled);
        assert_eq!(block.values, vec![0.01, 0.19, 0.98]);
    }

    #[test]
    fn empty_slots_hold_no_model() {
        let preset = Preset::parse(&sample()).unwrap();
        assert_eq!(preset.slots[1].kind, Kind::Empty);
        assert_eq!(preset.slots[1].model, None);
        assert_eq!(preset.blocks().count(), 1);
    }

    #[test]
    fn a_paired_model_of_minus_one_means_none() {
        let preset = Preset::parse(&sample()).unwrap();
        assert_eq!(preset.slots[0].paired, None);
    }

    #[test]
    fn reads_metadata() {
        let preset = Preset::parse(&sample()).unwrap();
        assert_eq!(preset.firmware().as_deref(), Some("3.80"));
        assert_eq!(preset.build(), Some("v3.71-32-g1039661"));
    }

    #[test]
    fn round_trips_through_the_wire_format() {
        let original = Preset::parse(&sample()).unwrap();
        let again = Preset::parse(&original.encode()).expect("re-parses");

        assert_eq!(again.slots, original.slots);
        assert_eq!(again.sections, original.sections);
        assert_eq!(again.firmware(), original.firmware());
        // Byte-for-byte, which is what makes writing a document back safe.
        assert_eq!(again.encode(), original.encode());
    }

    #[test]
    fn swapping_slots_moves_the_block_and_the_document_together() {
        let mut preset = Preset::parse(&sample()).unwrap();
        assert_eq!(preset.slots[0].model, Some(101));

        assert!(preset.swap_slots(0, 1));
        assert_eq!(preset.slots[1].model, Some(101));
        assert_eq!(preset.slots[0].kind, Kind::Empty);

        // The re-encoded document must agree with the in-memory view.
        let written = Preset::parse(&preset.encode()).unwrap();
        assert_eq!(written.slots, preset.slots);

        assert!(!preset.swap_slots(0, 99));
    }

    #[test]
    fn tempo_can_be_changed_and_survives_a_round_trip() {
        let mut preset = Preset::parse(&sample()).unwrap();
        assert!(
            !preset.set_tempo(96.0),
            "the sample has no settings section"
        );

        // With a settings section present it takes, and comes back out again.
        preset.tone = crate::msgmap! {
            key::SETTINGS => crate::msgmap! { key::TEMPO => Value::F32(120.0) },
        };
        assert!(preset.set_tempo(96.0));
        assert_eq!(preset.tempo(), Some(96.0));
        assert_eq!(Preset::parse(&preset.encode()).unwrap().tempo(), Some(96.0));
    }

    #[test]
    fn snapshots_can_be_renamed() {
        let mut preset = Preset::parse(&sample()).unwrap();
        preset.tone = crate::msgmap! {
            key::SNAPSHOT_SECTION => crate::msgmap! {
                key::SNAPSHOTS => Value::Array(vec![
                    crate::msgmap! { key::SNAPSHOT_NAME => Value::Str("SNAPSHOT 1".into()) },
                ]),
            },
        };
        assert!(preset.set_snapshot_name(0, "Verse"));
        assert_eq!(preset.snapshots(), vec!["Verse".to_string()]);
        assert!(!preset.set_snapshot_name(9, "Nope"));
    }

    /// Build a preset from a bare list of slot kinds. Blocks are given a model
    /// so they count as occupied — a slot with no model is a free position.
    fn shaped(kinds: &[i64]) -> Preset {
        let slot = |k: i64| {
            let mut fields = vec![(crate::msgpack::Key::Int(key::KIND), Value::Int(k))];
            if k == BLOCK {
                fields.push((
                    crate::msgpack::Key::Int(key::BODY),
                    crate::msgmap! {
                        key::MODEL_REF => crate::msgmap! { key::MODEL => Value::Int(101) },
                    },
                ));
            }
            Value::Map(fields)
        };
        let tone = crate::msgmap! {
            key::PATH => crate::msgmap! {
                key::SLOTS => Value::Array(kinds.iter().map(|k| slot(*k)).collect()),
            },
        };
        let mut blob = Encoder::encode(&Value::Str(Preset::MAGIC.into()));
        blob.extend(Encoder::encode(&Value::Bin(vec![0x3d], 0)));
        blob.extend(Encoder::encode(&tone));
        Preset::parse(&blob).unwrap()
    }

    /// A real preset captured from an HX Stomp, shared with the round-trip test.
    const FIXTURE: &[u8] = include_bytes!("../tests/preset.bin");

    // Kinds, for readable fixtures.
    const IN: i64 = 0;
    const OUT: i64 = 1;
    const SPLIT: i64 = 2;
    const JOIN: i64 = 3;
    const BLOCK: i64 = 6;

    #[test]
    fn a_split_puts_its_blocks_on_a_second_lane() {
        let layout = shaped(&[IN, BLOCK, BLOCK, OUT, SPLIT, BLOCK, JOIN]).layout();

        assert_eq!(layout.paths.len(), 1);
        let path = &layout.paths[0];
        assert_eq!(path.input, Some(0));
        assert_eq!(path.output, Some(3));
        assert_eq!(path.split, Some(4));
        assert_eq!(path.join, Some(6));
        assert_eq!(path.lanes.len(), 2);
        assert_eq!(path.lanes[0].blocks, vec![1, 2]);
        assert_eq!(path.lanes[1].blocks, vec![5]);
    }

    /// Helix and Helix LT carry two independent signal paths, so a preset that
    /// splits both has four lanes. Anything that assumes two is wrong there.
    #[test]
    fn a_device_with_two_signal_paths_yields_four_lanes() {
        let layout = shaped(&[
            IN, BLOCK, OUT, SPLIT, BLOCK, JOIN, // path 1, split
            IN, BLOCK, OUT, SPLIT, BLOCK, JOIN, // path 2, split
        ])
        .layout();

        assert_eq!(layout.paths.len(), 2);
        assert_eq!(layout.lanes().count(), 4);
        assert_eq!(layout.paths[1].input, Some(6));
        assert_eq!(layout.paths[1].lanes[1].blocks, vec![10]);
    }

    /// The split slot exists in every preset whether or not anything is on the
    /// lower branch, and an empty second row would be noise.
    #[test]
    fn a_split_with_nothing_below_it_stays_one_lane() {
        let layout = shaped(&[IN, BLOCK, OUT, SPLIT, JOIN]).layout();
        assert_eq!(layout.paths[0].lanes.len(), 1);
        assert_eq!(layout.paths[0].split, Some(3));
    }

    #[test]
    fn a_preset_without_a_split_has_one_lane() {
        let layout = Preset::parse(&sample()).unwrap().layout();
        assert!(layout.lanes().all(|l| l.branch == 0));
    }

    /// The real thing. This preset's lower branch is empty, so it draws as one
    /// lane — but its split slot still sits *after* the output in the array,
    /// which is exactly the trap: read as a running order it puts the split and
    /// join on the end of the chain, which is where they were being drawn.
    #[test]
    fn the_captured_preset_keeps_its_split_out_of_the_running_order() {
        let layout = Preset::parse(FIXTURE).unwrap().layout();
        assert_eq!(layout.paths.len(), 1);
        let path = &layout.paths[0];

        assert!(
            path.split.unwrap() > path.output.unwrap(),
            "the split sits after the output in the slot array"
        );
        assert_eq!(path.lanes.len(), 1, "nothing is on the lower branch");
        assert!(
            !path.lanes[0].blocks.contains(&path.split.unwrap())
                && !path.lanes[0].blocks.contains(&path.join.unwrap()),
            "the split and join must not appear as blocks in the lane"
        );
        // And there is room to put something on the lower branch.
        let preset = Preset::parse(FIXTURE).unwrap();
        let lower = Lane {
            branch: 1,
            blocks: Vec::new(),
            span: path.split.unwrap() + 1..path.join.unwrap(),
        };
        assert_eq!(lower.free(&preset).len(), 8);
    }

    /// The routing selector is the first thing HX Edit shows on Input and
    /// Main L/R, and it is not among the parameter values — it sits beside them
    /// under its own key, which is why it was missing from the editor.
    #[test]
    fn endpoint_routing_reads_and_writes() {
        let mut preset = Preset::parse(FIXTURE).unwrap();
        let input = preset.layout().paths[0].input.unwrap();
        let output = preset.layout().paths[0].output.unwrap();

        assert_eq!(preset.routing(input), Some(1));
        assert_eq!(preset.routing(output), Some(1));
        // A block has no routing field of its own.
        assert_eq!(preset.routing(1), None);
        assert!(!preset.set_routing(1, 3));

        assert!(preset.set_routing(output, 6));
        assert_eq!(preset.routing(output), Some(6));

        // And the document still re-encodes to the same length, so the section
        // offsets it carries stay valid.
        let before = Preset::parse(FIXTURE).unwrap().encode().len();
        assert_eq!(preset.encode().len(), before);
    }

    #[test]
    fn rejects_a_blob_that_is_not_a_preset() {
        assert!(Preset::parse(b"\xa3abc").is_none());
    }
}
