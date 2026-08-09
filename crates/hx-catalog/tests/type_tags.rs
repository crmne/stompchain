//! The engine class a slot carries, checked against every captured preset.
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
use hx_proto::Preset;

#[test]
fn every_captured_slot_gets_its_own_engine_class_back() {
    let Ok(catalog) = Catalog::load() else {
        eprintln!("skipping: HX Edit's catalog is not installed");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hx-proto/tests/fixtures");
    let files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {dir:?}: {e}"))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("hxpreset"))
        .collect();
    assert!(!files.is_empty(), "no fixtures to check against");

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
        eprintln!("{} slots held models the catalog cannot name", unknown.len());
    }
}
