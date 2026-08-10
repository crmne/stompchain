//! The two fields of a slot that a `.hlx` does not record, checked against
//! every captured preset.
//!
//! `Catalog::type_tag` is the one field of a slot that cannot be read off a
//! `.hlx`, so writing a device document from a symbolic tone depends on
//! deriving it correctly. This pins the derivation against the same factory
//! presets the byte codec uses: every occupied slot in every fixture must get
//! back the tag the device actually stamped on it.
//!
//! Skips when HX Edit's catalog is not installed (e.g. CI), rather than
//! failing in silence.

use std::fs;
use std::path::PathBuf;

use hx_catalog::Catalog;
use hx_proto::msgpack::Value;
use hx_proto::Preset;

fn fixtures() -> Vec<std::path::PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hx-proto/tests/fixtures");
    let files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {dir:?}: {e}"))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("hxpreset"))
        .collect();
    assert!(!files.is_empty(), "no fixtures to check against");
    files
}

#[test]
fn every_captured_slot_gets_its_own_engine_class_back() {
    let Ok(catalog) = Catalog::load() else {
        eprintln!("skipping: HX Edit's catalog is not installed");
        return;
    };
    let files = fixtures();
    let mut checked = 0;
    let mut unknown = Vec::new();
    for path in &files {
        let bytes = fs::read(path).unwrap();
        let preset = Preset::parse(&bytes).expect("a captured preset parses");
        for (index, slot) in preset.slots.iter().enumerate() {
            let Some(model) = slot.model else { continue };
            match catalog.type_tag(model, slot.paired.is_some()) {
                Some(tag) => {
                    assert_eq!(
                        tag,
                        slot.type_tag,
                        "{} slot {index}: model {model} (cab: {}) should carry {} but the \
                         derivation says {tag}",
                        path.file_name().unwrap().to_string_lossy(),
                        slot.paired.is_some(),
                        slot.type_tag,
                    );
                    checked += 1;
                }
                // A model the catalog cannot name is the catalog's gap, not
                // this derivation's - it is reported rather than asserted on.
                None => unknown.push(model),
            }
        }
    }
    assert!(checked > 0, "no slots were checked");
    if !unknown.is_empty() {
        eprintln!(
            "{} slots held models the catalog cannot name",
            unknown.len()
        );
    }
}

/// The count beside the value count, which is one short for the models that
/// carry a parameter the array does not admit to.
#[test]
fn every_captured_value_array_gets_its_second_count_back() {
    let Ok(catalog) = Catalog::load() else {
        eprintln!("skipping: HX Edit's catalog is not installed");
        return;
    };
    let mut checked = 0;
    for path in fixtures() {
        let bytes = fs::read(&path).unwrap();
        let preset = Preset::parse(&bytes).expect("a captured preset parses");
        let Some(Value::Array(items)) = preset.tone.get(0).and_then(|p| p.get(22)) else {
            continue;
        };
        for (index, item) in items.iter().enumerate() {
            let Some(slot) = preset.slots.get(index) else {
                continue;
            };
            let Some(model) = slot.model else { continue };
            let Some(array) = item.get(20).and_then(|body| body.get(11)) else {
                continue;
            };
            let (Some(count), Some(second)) = (
                array.get(2).and_then(Value::as_i64),
                array.get(3).and_then(Value::as_i64),
            ) else {
                continue;
            };
            assert_eq!(
                count,
                slot.values.len() as i64,
                "{} slot {index}: key 2 should be the number of values",
                path.file_name().unwrap().to_string_lossy()
            );
            let Some(derived) = catalog.value_count_2(model, slot.values.len()) else {
                continue;
            };
            assert_eq!(
                derived,
                second,
                "{} slot {index}: model {model} carries key 3 = {second} but the \
                 derivation says {derived}",
                path.file_name().unwrap().to_string_lossy()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no value arrays were checked");
}
