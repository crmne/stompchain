//! Writing a `.hlx` preset from a device-free [`Preset`], the save half of the
//! reader in [`inspect`](crate::inspect).
//!
//! This is the exact inverse of the inspector. The inspector turns Line 6's
//! symbolic JSON into "what this tone is"; this turns a parsed preset back into
//! that same JSON, so a library can hold tones as `.hlx` files and read them
//! back with the piece it was saved by. Like the inspector it runs entirely
//! from the catalog and the preset, never the pedal.
//!
//! Three shapes are worth calling out, all read off a real HX Edit `.hlx`:
//!
//! * Blocks are keyed `block0`, `block1`, … densely and in chain order, with
//!   the endpoints and junctions living under their own keys rather than in the
//!   block run. We number the blocks we emit, so the numbering stays dense even
//!   when a model cannot be written.
//! * An amp and its cab are two separate blocks. On the wire a paired cab rides
//!   in the amp's slot, so we split it back out into its own block, right after
//!   the amp - the way a standalone amp and cab already appear in a file.
//! * `@model` is matched back by exact string, both here and on the apply path,
//!   which has no base-name fallback. So we write the precise firmware symbol
//!   the wire number resolves to - suffix and all - rather than the base name a
//!   real file pairs with `@stereo`. It is unambiguous and it is what both of
//!   our readers require to resolve it.
//!
//! Parameter values are written in their native units, the way the format
//! stores them - no display scaling - keyed by the parameter's symbolic id, and
//! a switch as a bool. That is the mirror image of how the inspector reads them
//! back.

use serde_json::{json, Map, Value};

use hx_proto::preset::{Kind as SlotKind, Preset};

use crate::Catalog;

/// A serialised `.hlx` document together with a note of anything the catalog
/// could not name.
///
/// The `skipped` list is the same discipline the readers keep: a model with no
/// symbol, or a value with no parameter, is reported rather than dropped in
/// silence, so a caller can surface it.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    /// The `.hlx` JSON, ready to hand to [`inspect`](crate::inspect) or to
    /// serialise to a file.
    pub document: Value,
    pub skipped: Vec<String>,
}

impl Written {
    /// The document as pretty-printed JSON with a trailing newline, matching how
    /// the other JSON dumps in this workspace are written to disk.
    pub fn to_pretty_string(&self) -> String {
        serde_json::to_string_pretty(&self.document).unwrap_or_default() + "\n"
    }
}

/// Serialise a [`Preset`] into a Line 6 `.hlx` symbolic document.
///
/// `name` becomes `data.meta.name`. Every occupied block becomes an entry under
/// `data.tone.dsp0` - or `dsp1` for a second signal path, as a two-DSP device
/// carries - with its `@model`, `@enabled` and parameters. See the module docs
/// for the shape this produces and why.
pub fn to_hlx(preset: &Preset, catalog: &Catalog, name: &str) -> Written {
    let mut skipped = Vec::new();

    // One JSON object per DSP path, plus a running blockN counter for each. A
    // new path opens at every input, the way `Preset::layout` reads them; dsp0
    // is always present so blocks that precede any input - as a bare captured
    // tone can carry - still have a home.
    let mut dsps: Vec<Map<String, Value>> = vec![Map::new()];
    let mut next_block: Vec<i64> = vec![0];
    let mut path = 0usize;
    let mut opened = false;

    for slot in &preset.slots {
        match slot.kind {
            SlotKind::Input => {
                if opened {
                    path += 1;
                    dsps.push(Map::new());
                    next_block.push(0);
                }
                opened = true;
            }
            SlotKind::Block if slot.model.is_some() => {
                emit(
                    &mut dsps[path],
                    &mut next_block[path],
                    slot.model,
                    &slot.values,
                    slot.enabled,
                    catalog,
                    &mut skipped,
                );
                // The cab rides in the amp's slot on the wire; write it out as
                // its own block, sharing the amp's bypass state.
                if slot.paired.is_some() {
                    emit(
                        &mut dsps[path],
                        &mut next_block[path],
                        slot.paired,
                        &slot.paired_values,
                        slot.enabled,
                        catalog,
                        &mut skipped,
                    );
                }
            }
            _ => {}
        }
    }

    let mut tone = Map::new();
    for (index, dsp) in dsps.into_iter().enumerate() {
        tone.insert(format!("dsp{index}"), Value::Object(dsp));
    }

    let document = json!({
        "data": {
            "meta": { "name": name },
            "tone": Value::Object(tone),
        }
    });

    Written { document, skipped }
}

/// Write one model into a DSP's block map at its next position, or record why it
/// could not be. The amp and its cab both come through here, so both resolve
/// their model and parameters the same way.
fn emit(
    dsp: &mut Map<String, Value>,
    next_block: &mut i64,
    model: Option<u32>,
    values: &[f32],
    enabled: bool,
    catalog: &Catalog,
    skipped: &mut Vec<String>,
) {
    let Some(number) = model else { return };

    let Some(symbol) = catalog.symbol(number) else {
        skipped.push(format!("model {number}: no symbol in the catalog"));
        return;
    };

    let mut block = Map::new();
    block.insert("@model".into(), Value::String(symbol.symbol.clone()));
    block.insert("@enabled".into(), Value::Bool(enabled));

    // Values come in the order the device indexes them; each resolves to a
    // parameter whose symbolic id keys it in the file. A switch is written as a
    // bool, the way the reader folds it back to 0.0/1.0.
    for (index, value) in values.iter().enumerate() {
        let Some(param) = catalog.param(number, index) else {
            skipped.push(format!("{}: no parameter at index {index}", symbol.symbol));
            continue;
        };
        let written = if param.kind == crate::Kind::Switch {
            Value::Bool(*value >= 0.5)
        } else {
            json!(*value)
        };
        block.insert(param.id.clone(), written);
    }

    dsp.insert(format!("block{next_block}"), Value::Object(block));
    *next_block += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::catalog;
    use crate::{inspect, Category, ChainContent};
    use hx_proto::msgpack::{Encoder, Key};
    // The synthetic-preset helpers build MessagePack values; this explicit
    // import shadows the `serde_json::Value` the glob above brings in.
    use hx_proto::{msgmap, Preset, Value};
    use std::collections::BTreeMap;

    // Tone field keys, mirroring `hx_proto::preset`'s private `key` module. Kept
    // here so a synthetic preset can be built without a device or a fixture.
    const KIND: i64 = 19;
    const BODY: i64 = 20;
    const PATH: i64 = 0;
    const SLOTS: i64 = 22;
    const MODEL_REF: i64 = 24;
    const MODEL: i64 = 25;
    const PAIRED_MODEL: i64 = 26;
    const PAIRED_VALUES: i64 = 12;
    const VALUES: i64 = 11;
    const ENABLED: i64 = 10;
    const ARRAY_VALUES: i64 = 4;

    // Slot kinds on the wire.
    const INPUT: i64 = 0;
    const BLOCK: i64 = 6;

    fn values(vals: &[f32]) -> Value {
        msgmap! { ARRAY_VALUES => Value::Array(vals.iter().map(|v| Value::F32(*v)).collect()) }
    }

    /// An input slot, which opens a DSP path but carries no block.
    fn input() -> Value {
        msgmap! { KIND => Value::Int(INPUT) }
    }

    /// A block slot holding `model`, optionally with a cab riding in the same
    /// slot the way `paired` records it on the wire.
    fn block(model: u32, paired: Option<u32>, vals: &[f32], cab_vals: &[f32], enabled: bool) -> Value {
        let model_ref = Value::Map(vec![
            (Key::Int(MODEL), Value::Int(model as i64)),
            (
                Key::Int(PAIRED_MODEL),
                Value::Int(paired.map(|p| p as i64).unwrap_or(-1)),
            ),
        ]);
        let body = msgmap! {
            MODEL_REF => model_ref,
            ENABLED => Value::Bool(enabled),
            VALUES => values(vals),
            PAIRED_VALUES => values(cab_vals),
        };
        msgmap! { KIND => Value::Int(BLOCK), BODY => body }
    }

    /// Wrap a list of slots in the smallest blob `Preset::parse` accepts.
    fn preset(slots: Vec<Value>) -> Preset {
        let tone = msgmap! { PATH => msgmap! { SLOTS => Value::Array(slots) } };
        let mut blob = Encoder::encode(&Value::Str(Preset::MAGIC.into()));
        blob.extend(Encoder::encode(&Value::Bin(vec![0x3d], 0)));
        blob.extend(Encoder::encode(&tone));
        Preset::parse(&blob).expect("the synthetic blob parses")
    }

    /// The blocks a preset should read back as, derived straight from it through
    /// the same catalog the writer uses - amp then its cab, values folded the
    /// way a switch is - so a faithful round trip compares equal to it.
    fn expected(preset: &Preset, catalog: &Catalog) -> Vec<(u32, bool, BTreeMap<String, f32>)> {
        let mut out = Vec::new();
        let mut push = |number: u32, enabled: bool, vals: &[f32]| {
            let mut params = BTreeMap::new();
            for (i, v) in vals.iter().enumerate() {
                if let Some(param) = catalog.param(number, i) {
                    let folded = if param.kind == crate::Kind::Switch {
                        (*v >= 0.5) as u8 as f32
                    } else {
                        *v
                    };
                    params.insert(param.name.clone(), folded);
                }
            }
            out.push((number, enabled, params));
        };
        for (_, slot) in preset.blocks() {
            push(slot.model.unwrap(), slot.enabled, &slot.values);
            if let Some(cab) = slot.paired {
                push(cab, slot.enabled, &slot.paired_values);
            }
        }
        out
    }

    /// What the inspector read back, in the same shape as `expected`.
    fn read_back(tone: &crate::Tone) -> Vec<(u32, bool, BTreeMap<String, f32>)> {
        tone.blocks
            .iter()
            .map(|b| {
                (
                    b.model_number,
                    b.enabled,
                    b.params.iter().cloned().collect(),
                )
            })
            .collect()
    }

    /// First model number in a browse category, so the paired-cab test names a
    /// genuine amp and cab rather than pretending two effects are one.
    fn first_in_category(catalog: &Catalog, category: u32) -> Option<u32> {
        catalog
            .symbols()
            .iter()
            .find(|s| {
                s.model
                    .as_deref()
                    .and_then(|id| catalog.category_of(id))
                    .is_some_and(|c| c == category)
            })
            .map(|s| s.number)
    }

    #[test]
    fn a_blocks_models_params_and_bypass_round_trip() {
        let Some(catalog) = catalog() else { return };
        // Scream 808 (101) on, Room (247) bypassed - two effects whose parameter
        // counts are known, so the values map cleanly onto names both ways.
        let preset = preset(vec![
            input(),
            block(101, None, &[0.1, 0.2, 0.3], &[], true),
            block(247, None, &[0.4, 0.5, 0.6, 0.7, 0.8, 0.9], &[], false),
        ]);

        let written = to_hlx(&preset, &catalog, "Round Trip");
        assert!(written.skipped.is_empty(), "{:?}", written.skipped);

        let tone = inspect(&written.document, &catalog);
        assert_eq!(tone.name, "Round Trip");
        assert_eq!(read_back(&tone), expected(&preset, &catalog));

        // And spell out the facts the round trip stands on, so a regression
        // reads plainly rather than as a diff of two derived lists.
        assert_eq!(tone.models_used, vec![101, 247]);
        let scream = tone.blocks.iter().find(|b| b.model_number == 101).unwrap();
        assert!(scream.enabled);
        let room = tone.blocks.iter().find(|b| b.model_number == 247).unwrap();
        assert!(!room.enabled);
        let mix = room.params.iter().find(|(n, _)| n == "Mix").unwrap();
        assert!((mix.1 - 0.8).abs() < 1e-6, "Mix did not survive: {}", mix.1);
    }

    #[test]
    fn both_dsps_serialise_and_a_paired_cab_becomes_its_own_block() {
        let Some(catalog) = catalog() else { return };
        let (Some(amp), Some(cab)) = (
            first_in_category(&catalog, Category::AMP),
            first_in_category(&catalog, Category::CAB),
        ) else {
            eprintln!("SKIPPED: the catalog has no amp or cab to pair");
            return;
        };

        // Path 0: an amp with a cab riding in its slot. Path 1: a lone effect.
        let preset = preset(vec![
            input(),
            block(amp, Some(cab), &[0.5, 0.5], &[0.5, 0.5], true),
            input(),
            block(101, None, &[0.2, 0.3, 0.4], &[], true),
        ]);

        let written = to_hlx(&preset, &catalog, "Two DSPs");
        assert!(written.skipped.is_empty(), "{:?}", written.skipped);

        // The document carries both paths, and the cab is its own block on dsp0.
        let dsp0 = written.document.pointer("/data/tone/dsp0").unwrap();
        let dsp1 = written.document.pointer("/data/tone/dsp1").unwrap();
        assert_eq!(dsp0.as_object().unwrap().len(), 2, "amp and cab, split apart");
        assert_eq!(dsp1.as_object().unwrap().len(), 1, "the second path's effect");

        let tone = inspect(&written.document, &catalog);
        assert!(tone.has_amp, "the amp survived");
        assert!(tone.has_cab_or_ir, "the paired cab became a real block");
        assert_eq!(tone.chain_content, ChainContent::FullRig);

        // Two blocks on dsp0 (amp, cab), one on dsp1 (the effect).
        assert_eq!(tone.blocks.iter().filter(|b| b.path == 0).count(), 2);
        let second = tone.blocks.iter().find(|b| b.path == 1).expect("a dsp1 block");
        assert_eq!(second.model_number, 101);
    }

    #[test]
    fn a_model_with_no_symbol_is_reported_not_dropped() {
        let Some(catalog) = catalog() else { return };
        // A number past the end of the symbol table cannot be named.
        let preset = preset(vec![input(), block(999_999, None, &[0.5], &[], true)]);

        let written = to_hlx(&preset, &catalog, "Unknown");
        assert_eq!(written.skipped.len(), 1, "{:?}", written.skipped);
        assert!(written.skipped[0].contains("999999"));

        // Nothing was written for it, and the inspector reads back an empty tone
        // rather than a mystery block.
        let tone = inspect(&written.document, &catalog);
        assert!(tone.blocks.is_empty());
    }
}
