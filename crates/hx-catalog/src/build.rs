//! Building a device document's slots from a `.hlx`, the direction that did
//! not exist.
//!
//! [`to_hlx`](crate::to_hlx) turns a preset into Line 6's symbolic JSON, and
//! [`inspect`](crate::inspect) reads that JSON into tone facts. Neither goes
//! back to a document the pedal will accept, which is why restoring a `.hxb`
//! natively and importing a `.hlx` faithfully were the same missing piece: both
//! need JSON to become bytes.
//!
//! A slot on the wire is
//!
//! ```text
//! {19: kind, 20: {24: {23: paired?, 25: model, 26: cab or -1},
//!                  9: engine class, 10: enabled,
//!                 11: {2: n, 3: n', 4: [values]},
//!                 12: {…the cab's values…}}}
//! ```
//!
//! and every field of it can be read off a `.hlx` except two — the engine class
//! and that second count `n'` — which is what [`Catalog::type_tag`] and
//! [`Catalog::value_count_2`] exist for. See PROTOCOL.md.
//!
//! This writes slots into an existing document rather than inventing one from
//! nothing. A preset carries a great deal besides its chain — a section table
//! of byte offsets into itself, snapshot state, footswitch assignments — and
//! the honest way to get those right is to start from a document the device
//! wrote and replace the part being described.

use serde_json::Value as Json;

use hx_proto::msgpack::{Key, Value};
use hx_proto::Preset;

use crate::Catalog;

/// Wire keys, named. These mirror `hx_proto::preset::key`, which is private —
/// deliberately, since nothing outside the parser should be reading a document
/// by hand. Writing one is the exception that earns them.
mod key {
    pub const KIND: i64 = 19;
    pub const BODY: i64 = 20;
    pub const MODEL_REF: i64 = 24;
    pub const HAS_PAIRED: i64 = 23;
    pub const MODEL: i64 = 25;
    pub const PAIRED_MODEL: i64 = 26;
    pub const TYPE_TAG: i64 = 9;
    pub const ENABLED: i64 = 10;
    pub const VALUES: i64 = 11;
    pub const PAIRED_VALUES: i64 = 12;
    pub const COUNT: i64 = 2;
    pub const COUNT_2: i64 = 3;
    pub const ARRAY: i64 = 4;
    /// The wire's own number for an occupied block, and for an empty slot.
    pub const BLOCK: i64 = 6;
    pub const EMPTY: i64 = 8;
}

/// What could not be built, so a caller can say so rather than write a preset
/// that quietly lost something.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// How many blocks went in.
    pub blocks: usize,
    /// Models, parameters and slots that could not be resolved, each named.
    pub skipped: Vec<String>,
}

/// Write the chain a `.hlx` describes into `preset`, replacing what is there.
///
/// `preset` supplies everything the JSON does not: the section table, the
/// snapshot section, the endpoints and the junctions. Pass a document the
/// device wrote — an empty preset is the natural template.
///
/// Blocks land in the order the document names them, `block0` first, into the
/// slots the template keeps for them. A block that will not resolve is reported
/// and its slot left empty rather than filled with a guess.
pub fn slots_from_hlx(preset: &mut Preset, document: &Json, catalog: &Catalog) -> Built {
    let mut skipped = Vec::new();
    let mut blocks = 0;

    // Where a path's blocks may go: everything between its input and its
    // output, and between its split and its join. Read off the template rather
    // than assumed, so a device with a different slot count still works.
    let layout = preset.layout();
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for path in &layout.paths {
        if let (Some(input), Some(output)) = (path.input, path.output) {
            runs.push(((input + 1)..output).collect());
        }
        if let (Some(split), Some(join)) = (path.split, path.join) {
            runs.push(((split + 1)..join).collect());
        }
    }

    let tone = document.get("data").and_then(|d| d.get("tone"));
    for (dsp_index, run) in runs.iter().enumerate() {
        // dsp0 holds the main line and the branch alike; a second DSP is a
        // second path on hardware that has one.
        let name = format!("dsp{}", dsp_index.min(1));
        let Some(dsp) = tone.and_then(|t| t.get(&name)).and_then(Json::as_object) else {
            continue;
        };

        // block0, block1, … in numeric order, and the cabs that ride with them.
        let mut named: Vec<(usize, &str)> = dsp
            .keys()
            .filter_map(|k| {
                let digits = k.strip_prefix("block")?;
                Some((digits.parse::<usize>().ok()?, k.as_str()))
            })
            .collect();
        named.sort_unstable();

        let mut free = run.iter().copied();
        for (_, block_key) in named {
            let Some(node) = dsp.get(block_key) else { continue };
            let Some(position) = free.next() else {
                skipped.push(format!("{block_key}: the chain has no room left"));
                continue;
            };
            match build_slot(node, catalog) {
                Ok(slot) => {
                    if preset.paste_slot(position, &slot) {
                        blocks += 1;
                    } else {
                        skipped.push(format!("{block_key}: slot {position} would not take it"));
                    }
                }
                Err(why) => skipped.push(format!("{block_key}: {why}")),
            }
        }
    }

    Built { blocks, skipped }
}

/// Turn a whole `.hxb` backup into documents ready for the pedal.
///
/// This is what makes `.hxb` a format stompchain can *restore* rather than only
/// write. A bundle stores its presets as symbolic JSON — HX Edit's own choice —
/// so putting one back has always needed this direction, and until now the only
/// route was rebuilding a tone through parameter edits, which loses whatever
/// the editor does not model.
///
/// `template` supplies everything a `.hlx` does not describe and must be a
/// document the device wrote. Its chain is emptied first, so nothing of the
/// template's own tone survives into the result.
///
/// Empty slots come back as `None`, so a caller can blank them rather than
/// leaving whatever the pedal happens to hold there.
pub fn documents_from_backup(
    backup: &crate::Backup,
    template: &Preset,
    catalog: &Catalog,
) -> Vec<Option<(String, Preset, Built)>> {
    let bytes = template.encode();
    backup
        .presets
        .iter()
        .map(|entry| {
            if entry.empty {
                return None;
            }
            // A fresh copy per preset: each starts from the same template
            // rather than from whatever the last one left behind.
            let mut document = Preset::parse(&bytes)?;
            empty_the_chain(&mut document);
            let built = slots_from_hlx(&mut document, &entry.hlx, catalog);
            Some((entry.name.clone(), document, built))
        })
        .collect()
}

/// Clear every block slot, leaving the endpoints and junctions alone.
///
/// A template is only a source of the parts a `.hlx` cannot describe; carrying
/// its blocks through would put someone else's tone in the gaps of the one
/// being restored.
pub fn empty_the_chain(preset: &mut Preset) {
    let empty = Value::Map(vec![
        (Key::Int(key::KIND), Value::Int(key::EMPTY)),
        (Key::Int(key::BODY), Value::Nil),
    ]);
    for position in 0..preset.slots.len() {
        // `paste_slot` refuses anything that is not a block or already empty,
        // which is exactly the protection wanted here.
        let _ = preset.paste_slot(position, &empty);
    }
}

/// One `.hlx` block node as the slot the device expects.
fn build_slot(node: &Json, catalog: &Catalog) -> Result<Value, String> {
    let symbol_name = node
        .get("@model")
        .and_then(Json::as_str)
        .ok_or("no @model")?;
    let symbol = resolve(catalog, symbol_name, node)
        .ok_or_else(|| format!("the catalog does not know {symbol_name}"))?;
    let model = symbol.number;

    // The device indexes values by position, and the symbol's parameter list is
    // that order. A parameter the document does not mention keeps the value the
    // catalog gives as its default rather than becoming zero, which for a knob
    // like Master is the difference between a preset and a silent one.
    let mut values = Vec::with_capacity(symbol.parameters.len());
    for id in &symbol.parameters {
        let found = node.get(id).and_then(number_of);
        values.push(found.unwrap_or_else(|| default_of(catalog, model, id)));
    }
    // The values the symbol table does not name, which some models carry after
    // the named ones. `to_hlx` keeps them under `@unnamed`; a file from HX Edit
    // will not have them, and those models then take a zero, which is what the
    // device itself writes into a slot it has just been given.
    if let Some(extra) = node.get("@unnamed").and_then(Json::as_array) {
        values.extend(extra.iter().filter_map(number_of));
    }

    let enabled = node
        .get("@enabled")
        .and_then(Json::as_bool)
        .unwrap_or(true);
    let type_tag = catalog
        .type_tag(model, false)
        .ok_or_else(|| format!("no engine class for {symbol_name}"))?;
    let count_2 = catalog
        .value_count_2(model, values.len())
        .ok_or_else(|| format!("no value count for {symbol_name}"))?;

    Ok(Value::Map(vec![
        (Key::Int(key::KIND), Value::Int(key::BLOCK)),
        (
            Key::Int(key::BODY),
            Value::Map(vec![
                (
                    Key::Int(key::MODEL_REF),
                    Value::Map(vec![
                        (Key::Int(key::HAS_PAIRED), Value::Bool(false)),
                        (Key::Int(key::MODEL), Value::Int(model as i64)),
                        (Key::Int(key::PAIRED_MODEL), Value::Int(-1)),
                    ]),
                ),
                (Key::Int(key::TYPE_TAG), Value::Int(type_tag)),
                (Key::Int(key::ENABLED), Value::Bool(enabled)),
                (Key::Int(key::VALUES), counted(&values, count_2)),
                // No cab rides along: `to_hlx` splits a paired cab into its own
                // block, so a document never asks for one here.
                (Key::Int(key::PAIRED_VALUES), counted(&[], 0)),
            ]),
        ),
    ]))
}

/// Which firmware symbol a `@model` names.
///
/// A `.hlx` writes the *shared* model id — `HD2_DistMinotaur` — where the
/// firmware has a mono symbol and a stereo one, each with its own wire number
/// and its own parameter list. The name alone therefore does not say which, and
/// picking the first would silently turn every stereo block mono.
///
/// The parameters do say. A block node lists the parameters it actually has, so
/// the candidate whose list those keys match is the one that was written. Ties
/// go to the lower wire number, which is the mono variant and the one a file
/// naming neither is likelier to have meant.
fn resolve<'a>(catalog: &'a Catalog, name: &str, node: &Json) -> Option<&'a crate::Symbol> {
    let candidates: Vec<&crate::Symbol> = catalog
        .symbols()
        .iter()
        .filter(|s| s.symbol == name || s.model.as_deref() == Some(name))
        .collect();
    if candidates.len() <= 1 {
        return candidates.into_iter().next();
    }
    let present = |s: &crate::Symbol| -> (usize, usize) {
        let hit = s
            .parameters
            .iter()
            .filter(|p| node.get(p.as_str()).is_some())
            .count();
        // Most parameters accounted for, then fewest left unexplained.
        (hit, s.parameters.len().saturating_sub(hit))
    };
    candidates.into_iter().min_by_key(|s| {
        let (hit, missing) = present(s);
        // Sorted ascending, so negate the hits to prefer more of them.
        (std::cmp::Reverse(hit), missing, s.number)
    })
}

/// A value array in the shape the wire uses: the count, the second count, and
/// the values.
fn counted(values: &[f32], count_2: i64) -> Value {
    Value::Map(vec![
        (Key::Int(key::COUNT), Value::Int(values.len() as i64)),
        (Key::Int(key::COUNT_2), Value::Int(count_2)),
        (
            Key::Int(key::ARRAY),
            Value::Array(values.iter().map(|v| Value::F32(*v)).collect()),
        ),
    ])
}

/// A `.hlx` value as the number the wire holds. A switch is written as a bool
/// and stored as 0 or 1.
fn number_of(value: &Json) -> Option<f32> {
    match value {
        Json::Bool(b) => Some(*b as u8 as f32),
        Json::Number(n) => n.as_f64().map(|f| f as f32),
        _ => None,
    }
}

/// What a parameter should be when the document does not say.
fn default_of(catalog: &Catalog, model: u32, id: &str) -> f32 {
    catalog
        .model_number(model)
        .and_then(|m| catalog.ordered_params(m).into_iter().find(|p| p.id == id))
        .map(|p| p.default)
        .unwrap_or(0.0)
}
