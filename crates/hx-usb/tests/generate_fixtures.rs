//! Regenerate the codec fixtures from presets *we* author, not factory content.
//!
//! Run with a pedal attached to (re)create the round-trip fixtures in
//! hx-proto/tests/fixtures:
//!
//!     cargo test -p hx-usb --test generate_fixtures -- --ignored --nocapture
//!
//! It builds each structure in the edit buffer, reads it back byte-exact, and
//! writes it to a file - then restores the buffer it started with. Nothing is
//! saved to a slot. The presets are our own arrangements; only Line 6 model
//! *ids* are referenced, which is a fact for interoperability, not their
//! content. Every model id below was confirmed on firmware 3.80.

use std::path::PathBuf;

use hx_proto::msgpack::Value;
use hx_proto::Preset;
use hx_usb::Session;

// Confirmed model ids (firmware 3.80).
const DRIVE_MINOTAUR: u32 = 100;
const DRIVE_SCREAM: u32 = 101;
const AMP_US_DOUBLE: u32 = 42;
const CAB_2X12: u32 = 54;
const EQ_SHELF: u32 = 527;
const DELAY_BUCKET: u32 = 76;
const REVERB_PLATE: u32 = 635;

fn device() -> Option<Session> {
    let found = hx_usb::list().ok()?.into_iter().next()?;
    match found.open() {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("SKIPPED: {e}");
            None
        }
    }
}

/// The first block slot and the slot after the last, from the loaded preset.
fn block_span(session: &mut Session) -> (usize, usize) {
    let preset = session.read_preset().expect("read for layout");
    let layout = preset.layout();
    let path = layout.paths.first().expect("a signal path");
    let base = path.input.map(|i| i + 1).unwrap_or(1);
    let ceiling = path.output.unwrap_or(base + 8);
    (base, ceiling)
}

/// Clear every block in the loaded preset, then let the device settle.
fn clear_all(session: &mut Session) {
    let preset = session.read_preset().expect("read to clear");
    for (position, slot) in preset.slots.iter().enumerate() {
        if slot.kind == hx_proto::preset::Kind::Block && slot.model.is_some() {
            let _ = session.clear_block(position as i64);
        }
    }
    let _ = session.read_preset(); // barrier
}

/// A block to build: model, whether it is engaged, and (index, value) params.
struct Block {
    model: u32,
    enabled: bool,
    params: Vec<(i64, f32)>,
}

fn b(model: u32) -> Block {
    Block { model, enabled: true, params: Vec::new() }
}

/// Build a linear chain in the edit buffer and return the bytes.
fn build(session: &mut Session, blocks: &[Block]) -> Vec<u8> {
    clear_all(session);
    let (base, ceiling) = block_span(session);
    for (i, block) in blocks.iter().enumerate() {
        let position = (base + i) as i64;
        if base + i >= ceiling {
            break;
        }
        // A refusal is usually pacing; settle and retry once.
        if session.set_model(position, block.model).is_err() {
            let _ = session.read_preset();
            session.set_model(position, block.model).expect("set_model");
        }
        for (index, value) in &block.params {
            let _ = session.set_param(position, *index, Value::F32(*value));
        }
        if !block.enabled {
            let _ = session.set_enabled(position, false);
        }
    }
    let _ = session.read_preset(); // settle
    session.read_preset().expect("read built preset").encode()
}

fn write_fixture(name: &str, bytes: &[u8]) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hx-proto/tests/fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.hxpreset"));
    std::fs::write(&path, bytes).unwrap();
    // Confirm it round-trips before we trust it.
    let preset = Preset::parse(bytes).expect("generated preset parses");
    assert_eq!(preset.encode(), bytes, "{name} does not round-trip");
    println!("  wrote {name} ({} bytes, {} blocks)", bytes.len(), preset.blocks().count());
}

#[test]
#[ignore = "regenerates fixtures against real hardware"]
fn generate_our_own_fixtures() {
    let Some(mut session) = device() else { return };

    // Save the buffer we start on, to restore at the end.
    let original = session.read_preset().expect("read original").encode();

    // 1. Empty.
    let empty = build(&mut session, &[]);
    write_fixture("gen-01-empty", &empty);

    // 2. A single drive.
    let one = build(&mut session, &[b(DRIVE_MINOTAUR)]);
    write_fixture("gen-02-one-drive", &one);

    // 3. Amp into cab.
    let amp_cab = build(&mut session, &[b(AMP_US_DOUBLE), b(CAB_2X12)]);
    write_fixture("gen-03-amp-cab", &amp_cab);

    // 4. A full linear rig.
    let full = build(
        &mut session,
        &[b(DRIVE_MINOTAUR), b(AMP_US_DOUBLE), b(CAB_2X12), b(EQ_SHELF), b(DELAY_BUCKET), b(REVERB_PLATE)],
    );
    write_fixture("gen-04-full-rig", &full);

    // 5. Effects only, no amp or cab.
    let fx = build(&mut session, &[b(DRIVE_SCREAM), b(DELAY_BUCKET), b(REVERB_PLATE)]);
    write_fixture("gen-05-effects-only", &fx);

    // 6. An amp with several parameters pushed off their defaults.
    let tweaked = build(
        &mut session,
        &[Block {
            model: AMP_US_DOUBLE,
            enabled: true,
            params: vec![(0, 0.8), (1, 0.25), (2, 0.65), (3, 0.4)],
        }],
    );
    write_fixture("gen-06-params-tweaked", &tweaked);

    // 7. Some blocks bypassed.
    let bypassed = build(
        &mut session,
        &[
            Block { model: DRIVE_MINOTAUR, enabled: false, params: vec![] },
            b(AMP_US_DOUBLE),
            b(CAB_2X12),
            Block { model: DELAY_BUCKET, enabled: false, params: vec![] },
        ],
    );
    write_fixture("gen-07-bypassed", &bypassed);

    // 8. Snapshots: the same chain with the delay mix moved per snapshot.
    build(&mut session, &[b(AMP_US_DOUBLE), b(CAB_2X12), b(DELAY_BUCKET)]);
    let (base, _) = block_span(&mut session);
    let delay = (base + 2) as i64;
    for (snap, mix) in [(0i64, 0.1f32), (1, 0.5), (2, 0.9)] {
        session.select_snapshot(snap).expect("snapshot");
        let _ = session.set_param(delay, 1, Value::F32(mix));
    }
    session.select_snapshot(0).expect("snapshot home");
    let _ = session.read_preset();
    let snaps = session.read_preset().expect("read snapshots").encode();
    write_fixture("gen-08-snapshots", &snaps);

    // Put the buffer back exactly as it was.
    if let Some(preset) = Preset::parse(&original) {
        session.write_preset(&preset).expect("restore original buffer");
    }
    println!("done; edit buffer restored");
}
