//! The JSON-to-document direction, checked by making it undo the other one.
//!
//! Every captured preset is turned into a `.hlx` and then written back into an
//! empty document. If the converter is right, what comes out is the preset that
//! went in — the same models in the same slots, engaged the same way, with the
//! same values. Byte-for-byte where the whole document survives the trip.
//!
//! Skips when HX Edit's catalog is not installed (e.g. CI), rather than failing
//! in silence.

use std::fs;
use std::path::PathBuf;

use hx_catalog::Catalog;
use hx_proto::Preset;

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hx-proto/tests/fixtures");
    let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {dir:?}: {e}"))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("hxpreset"))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&p).unwrap())
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no fixtures to check against");
    out
}

/// The whole point: a chain written from JSON is the chain that JSON described.
#[test]
fn a_chain_written_from_json_is_the_chain_it_described() {
    let Ok(catalog) = Catalog::load() else {
        eprintln!("skipping: HX Edit's catalog is not installed");
        return;
    };

    for (name, bytes) in fixtures() {
        let original = Preset::parse(&bytes).expect("a captured preset parses");
        let document = hx_catalog::to_hlx(&original, &catalog, &name).document;

        // Written into a fresh copy of the same document, so everything the
        // JSON does not describe is identical and only the chain is under test.
        let mut rebuilt = Preset::parse(&bytes).expect("parses again");
        for position in 0..rebuilt.slots.len() {
            let empty = hx_proto::msgpack::Value::Map(vec![
                (hx_proto::msgpack::Key::Int(19), hx_proto::msgpack::Value::Int(8)),
                (hx_proto::msgpack::Key::Int(20), hx_proto::msgpack::Value::Nil),
            ]);
            let _ = rebuilt.paste_slot(position, &empty);
        }
        let built = hx_catalog::slots_from_hlx(&mut rebuilt, &document, &catalog);

        let wanted: Vec<_> = original
            .slots
            .iter()
            .filter(|s| s.model.is_some() && s.kind == hx_proto::preset::Kind::Block)
            .collect();
        assert_eq!(
            built.blocks,
            wanted.len(),
            "{name}: put back {} of {} blocks; skipped {:?}",
            built.blocks,
            wanted.len(),
            built.skipped
        );
        assert!(built.skipped.is_empty(), "{name}: skipped {:?}", built.skipped);

        for (index, (before, after)) in original.slots.iter().zip(rebuilt.slots.iter()).enumerate() {
            if before.kind != hx_proto::preset::Kind::Block || before.model.is_none() {
                continue;
            }
            assert_eq!(before.model, after.model, "{name} slot {index}: model");
            assert_eq!(before.enabled, after.enabled, "{name} slot {index}: engaged");
            assert_eq!(
                before.type_tag, after.type_tag,
                "{name} slot {index}: engine class"
            );
            assert_eq!(
                before.values.len(),
                after.values.len(),
                "{name} slot {index}: value count"
            );
            for (i, (a, b)) in before.values.iter().zip(after.values.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-6,
                    "{name} slot {index} value {i}: {a} became {b}"
                );
            }
        }
    }
}

/// A rebuilt document survives the trip to bytes and back.
///
/// Not byte-exactness: a `.hlx` does not record how the device encoded each
/// number — the same 1.0 may be an integer in one preset and a float in
/// another, and the widths matter because a preset carries a table of byte
/// offsets into itself. What is recomputed on encode is that table, so the
/// document the device receives is coherent; what cannot be reproduced from
/// JSON is the original's choice of tags. So the guarantee is the one that
/// matters for writing to a pedal: encode it, parse it back, and the chain is
/// still the chain.
#[test]
fn a_rebuilt_document_survives_encoding_and_parsing_back() {
    let Ok(catalog) = Catalog::load() else {
        eprintln!("skipping: HX Edit's catalog is not installed");
        return;
    };

    for (name, bytes) in fixtures() {
        let original = Preset::parse(&bytes).expect("parses");
        let document = hx_catalog::to_hlx(&original, &catalog, &name).document;
        let mut rebuilt = Preset::parse(&bytes).expect("parses again");
        hx_catalog::slots_from_hlx(&mut rebuilt, &document, &catalog);

        let round_tripped =
            Preset::parse(&rebuilt.encode()).expect("a rebuilt document parses back");
        assert_eq!(
            round_tripped.slots.len(),
            original.slots.len(),
            "{name}: the slot count changed"
        );
        for (index, (before, after)) in original
            .slots
            .iter()
            .zip(round_tripped.slots.iter())
            .enumerate()
        {
            assert_eq!(before.kind, after.kind, "{name} slot {index}: kind");
            assert_eq!(before.model, after.model, "{name} slot {index}: model");
            assert_eq!(before.enabled, after.enabled, "{name} slot {index}: engaged");
            assert_eq!(
                before.values.len(),
                after.values.len(),
                "{name} slot {index}: value count"
            );
            for (i, (a, b)) in before.values.iter().zip(after.values.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-6,
                    "{name} slot {index} value {i}: {a} became {b}"
                );
            }
        }
    }
}

/// A whole `.hxb` goes back to documents: write a bundle from real presets,
/// read it, rebuild each one, and the chains are the chains that went in.
///
/// This is the round trip that makes `.hxb` restorable rather than only
/// writable, exercised end to end without a pedal.
#[test]
fn a_bundle_written_from_presets_rebuilds_into_the_same_chains() {
    let Ok(catalog) = Catalog::load() else {
        eprintln!("skipping: HX Edit's catalog is not installed");
        return;
    };
    let all = fixtures();

    // Every fixture becomes a slot in one bundle, in order.
    let tones: Vec<(String, Option<serde_json::Value>)> = all
        .iter()
        .map(|(name, bytes)| {
            let preset = Preset::parse(bytes).expect("parses");
            let document = hx_catalog::to_hlx(&preset, &catalog, name).document;
            // The bundle stores what lives under `data`, which is what
            // `read_backup` hands back per preset.
            let tone = document
                .get("data")
                .and_then(|d| d.get("tone"))
                .cloned()
                .expect("a tone");
            (name.clone(), Some(tone))
        })
        .collect();

    let bytes = hx_catalog::write_backup(&hx_catalog::NewBackup {
        setlist: "PRESETS",
        presets: &tones,
        globals: serde_json::json!({}),
        device: 0x0021_0006,
        device_version: 0x0380_0000,
        captured: 0,
    });
    let backup = hx_catalog::read_backup(&bytes).expect("the bundle we just wrote reads back");

    // An empty preset from the fixtures is the template, the way a restore
    // would use a document read off the pedal.
    let (_, empty_bytes) = all
        .iter()
        .find(|(name, _)| name == "gen-01-empty")
        .expect("an empty fixture");
    let template = Preset::parse(empty_bytes).expect("parses");

    let rebuilt = hx_catalog::documents_from_backup(&backup, &template, &catalog);
    let mut checked = 0;
    for (name, original_bytes) in &all {
        let original = Preset::parse(original_bytes).expect("parses");
        let wanted: Vec<_> = original
            .slots
            .iter()
            .filter(|s| s.kind == hx_proto::preset::Kind::Block && s.model.is_some())
            .collect();
        if wanted.is_empty() {
            continue;
        }
        let Some(Some((_, document, built))) = rebuilt
            .iter()
            .position(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|(entry_name, _, _)| entry_name == name)
            })
            .map(|i| &rebuilt[i])
        else {
            panic!("{name}: not in the rebuilt bundle");
        };
        assert!(built.skipped.is_empty(), "{name}: skipped {:?}", built.skipped);

        let got: Vec<_> = document
            .slots
            .iter()
            .filter(|s| s.kind == hx_proto::preset::Kind::Block && s.model.is_some())
            .collect();
        assert_eq!(got.len(), wanted.len(), "{name}: block count");
        for (i, (before, after)) in wanted.iter().zip(got.iter()).enumerate() {
            assert_eq!(before.model, after.model, "{name} block {i}: model");
            assert_eq!(before.enabled, after.enabled, "{name} block {i}: engaged");
            assert_eq!(before.values, after.values, "{name} block {i}: values");
        }
        // And what comes out is a document the parser accepts.
        assert!(
            Preset::parse(&document.encode()).is_some(),
            "{name}: the rebuilt document does not parse back"
        );
        checked += 1;
    }
    assert!(checked > 0, "no bundles were checked");
}
