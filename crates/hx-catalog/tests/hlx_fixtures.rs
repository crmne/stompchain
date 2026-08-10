//! The `.hlx` codec, exercised over real captured presets.
//!
//! `to_hlx` turns a device preset into Line 6's symbolic `.hlx`, and `inspect`
//! reads that back into tone facts. This is the codec the whole multi-backend
//! future rests on, so it is pinned against the same factory presets the byte
//! codec uses (see hx-proto/tests/fixtures). Every one must survive
//! preset -> hlx -> facts with its blocks intact.
//!
//! Skips when HX Edit's catalog is not installed (e.g. CI), rather than
//! failing in silence.

use std::fs;
use std::path::PathBuf;

use hx_catalog::Catalog;
use hx_proto::Preset;

fn fixtures() -> Vec<(String, Vec<u8>)> {
    // The fixtures live with the byte-codec tests; share them rather than copy.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hx-proto/tests/fixtures");
    let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {dir:?}: {e}"))
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("hxpreset"))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&p).unwrap())
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_factory_preset_survives_the_hlx_codec() {
    let Ok(catalog) = Catalog::load() else {
        eprintln!("SKIPPED: HX Edit is not installed, so the .hlx codec cannot run");
        return;
    };

    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, bytes) in fixtures() {
        let Some(preset) = Preset::parse(&bytes) else {
            failures.push(format!("{name}: does not parse"));
            continue;
        };
        checked += 1;
        let block_count = preset.blocks().count();

        // preset -> .hlx
        let written = hx_catalog::to_hlx(&preset, &catalog, &name);
        if !written.skipped.is_empty() {
            // A block the writer could not translate: worth knowing, since a
            // factory preset uses only stock blocks.
            eprintln!("  {name}: to_hlx skipped {:?}", written.skipped);
        }

        // .hlx -> facts
        let tone = hx_catalog::inspect(&written.document, &catalog);

        if block_count > 0 {
            if tone.blocks.is_empty() {
                failures.push(format!(
                    "{name}: {block_count} blocks in the preset, none survived to facts"
                ));
            }
            if tone.models_used.is_empty() {
                failures.push(format!("{name}: has blocks but no models were resolved"));
            }
        }
    }
    assert!(checked > 0, "no fixtures were checked");
    assert!(
        failures.is_empty(),
        "the .hlx codec lost blocks on real presets:\n{}",
        failures.join("\n")
    );
}
