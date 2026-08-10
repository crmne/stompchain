//! Record a live session across the *whole* command protocol, then replay it
//! with no device.
//!
//! `capture_a_session` (ignored; needs a pedal) exercises every `Session`
//! command once and records the byte transport to tests/fixtures/. It runs
//! entirely on a factory preset it backs up and restores, an empty IR slot, and
//! a global setting it toggles and restores, so nothing on the device is left
//! changed. `a_recorded_session_replays_offline` reruns the same sequence
//! against the transcript with no hardware: every request is checked
//! byte-for-byte against what was recorded, and every response is parsed. So the
//! command layer - encoding and parsing, for the whole protocol - stays
//! regression-tested after the hardware is gone. Regenerate with:
//!
//!     cargo test -p hx-usb --test replay capture_a_session -- --ignored

use std::path::PathBuf;

use hx_proto::msgpack::Value;
use hx_proto::rpc::Source;
use hx_usb::replay::{finish, log, ReplayWire, Transcript};
use hx_usb::Session;

fn transcript_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/session.transcript")
}

/// The IR slot the capture uses. Confirmed empty before running, cleared after.
const IR_SLOT: i64 = 1;

struct Summary {
    name: String,
    blocks: usize,
    presets: usize,
    setlists: usize,
}

/// Every command once, in one fixed order, args derived only from reads so
/// capture and replay produce identical requests. Write commands ignore their
/// result: a device refusal is a recorded exchange too, and the point is that
/// the *request* still encodes the same. The edit buffer is rebuilt from the
/// preset read at the start; persisting writes land on the loaded (factory)
/// preset, which the caller restores.
fn exercise(s: &mut Session) -> Summary {
    // --- reads ---
    let (setlist, index, name) = s.preset_info().expect("preset_info");
    let preset = s.read_preset().expect("read_preset");
    let presets = s.presets(0).expect("presets");
    let setlists = s.setlists().expect("setlists");
    let _irs = s.irs().expect("irs");
    let _external = s.tempo_is_external().unwrap_or(false);
    let eq = s.object(203).expect("object: global eq enable");
    let _gain = s.object(192).expect("object: low peak gain");
    let _tempo = s.object(16).expect("object: tempo");
    let _raw = s.fetch(16).expect("fetch");
    let _ = s.poll_notifications();

    let pos = preset.blocks().next().map(|(p, _)| p as i64);
    let input = preset.layout().paths.first().and_then(|p| p.input);

    // --- block-scoped edit-buffer writes ---
    if let Some(p) = pos {
        let _ = s.select_block(p);
        let _ = s.set_param(p, 0, Value::F32(0.42));
        let _ = s.set_enabled(p, false);
        let _ = s.set_enabled(p, true);
        let _ = s.assign_bypass_footswitch(p, 1);
        let _ = s.unassign_bypass_footswitch(p, 1);
        let _ = s.assign_parameter(p, 0, Some(Source::Expression(1)));
        let _ = s.assign_bypass_midi(p, true);
        let _ = s.assign_bypass_midi(p, false);
        let _ = s.set_model(p, 100); // change the block; rebuilt below
        let _ = s.clear_block(p);
    }

    // --- routing ---
    if let Some(inp) = input {
        let was = preset.routing(inp).unwrap_or(1);
        let to = if was == 4 { 1 } else { 4 };
        let _ = s.set_routing(inp as i64, to);
        let _ = s.set_routing(inp as i64, was);
    }

    // --- snapshots + tempo ---
    let _ = s.select_snapshot(1);
    let _ = s.rename_snapshot(0, "A");
    let _ = s.select_snapshot(0);
    let _ = s.set_tempo(123.0);

    // --- a global setting: toggle and put back ---
    if let Value::Bool(was) = eq {
        let _ = s.set_object(203, Value::Bool(!was));
        let _ = s.set_object(203, Value::Bool(was));
    }

    // --- impulse response: fill an empty slot, then empty it again ---
    let ir: Vec<f32> = (0..256).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
    let _ = s.upload_ir(IR_SLOT, "TESTIR", &ir);
    let _ = s.clear_ir(IR_SLOT);

    // --- rebuild the edit buffer from what we read, then the flash writes ---
    let _ = s.write_preset(&preset);
    let _ = s.rename_preset(setlist, index, "TEMPNAME");
    let _ = s.rename_preset(setlist, index, &name);
    let _ = s.save_preset(setlist, index, &name);

    Summary {
        name,
        blocks: preset.blocks().count(),
        presets: presets.len(),
        setlists: setlists.len(),
    }
}

#[test]
#[ignore = "records a live session across the whole protocol"]
fn capture_a_session() {
    let Some(found) = hx_usb::list().ok().and_then(|d| d.into_iter().next()) else {
        eprintln!("SKIPPED: no HX device attached");
        return;
    };

    // Do all of this on a factory preset we can put back exactly, never a
    // personal one. Find one by name rather than guessing a slot index.
    let mut setup = found.open().expect("open for setup");
    let where_home = setup.preset_info().expect("home preset");
    let names = setup.presets(0).expect("preset names");
    // Stock preset categories carry these tags; personal tones (CT-, FX-,
    // Soundgarden) never do. Match on the tag, whatever the separator.
    let factory_index = names
        .iter()
        .position(|n| n.contains("DIR") || n.contains("BAS") || n.contains("KEY"))
        .expect("a stock preset to work on") as i64;
    let factory_name = names[factory_index as usize].clone();
    setup
        .select_preset(0, factory_index)
        .expect("select factory");
    let factory_backup = setup.read_preset().expect("back up factory").encode();
    let occupied: Vec<i64> = setup.irs().expect("irs").iter().map(|(n, _)| *n).collect();
    assert!(
        !occupied.contains(&IR_SLOT),
        "IR slot {IR_SLOT} is in use; refusing to touch it"
    );
    drop(setup);

    // Record the whole protocol.
    let log = log();
    let mut session = found.open_recording(log.clone()).expect("open recording");
    let summary = exercise(&mut session);
    drop(session);

    let transcript = finish(&log);
    std::fs::create_dir_all(transcript_path().parent().unwrap()).unwrap();
    std::fs::write(transcript_path(), transcript.to_text()).unwrap();

    // Put the device back exactly: restore the factory preset, clear our IR
    // slot, return to where we started.
    let mut restore = found.open().expect("open for restore");
    restore
        .select_preset(0, factory_index)
        .expect("reselect factory");
    if let Some(preset) = hx_proto::Preset::parse(&factory_backup) {
        restore
            .write_preset(&preset)
            .expect("restore factory buffer");
        restore
            .save_preset(0, factory_index, &factory_name)
            .expect("resave factory");
    }
    let _ = restore.clear_ir(IR_SLOT);
    restore
        .select_preset(where_home.0, where_home.1)
        .expect("return home");

    eprintln!(
        "recorded {} transfers over the whole protocol; {:?} ({} blocks, {} presets, {} setlists)",
        transcript.0.len(),
        summary.name,
        summary.blocks,
        summary.presets,
        summary.setlists
    );
}

#[test]
fn a_recorded_session_replays_offline() {
    let Ok(text) = std::fs::read_to_string(transcript_path()) else {
        eprintln!("SKIPPED: no transcript yet - run capture_a_session with a pedal");
        return;
    };
    let transcript = Transcript::from_text(&text);
    let recorded = transcript.0.len();

    // No hardware: the transcript stands in for the device, and every request
    // the session sends across the whole protocol is checked against what was
    // recorded.
    let wire = ReplayWire::new(transcript);
    let drifted = wire.drifted();
    let mut session =
        Session::replaying(Box::new(wire), hx_proto::HX_STOMP).expect("the handshake replays");
    let summary = exercise(&mut session);

    assert!(!summary.name.is_empty(), "a preset name came back");
    assert!(summary.presets > 0, "the preset list came back");
    assert!(recorded > 0, "the transcript held transfers");

    // The point of the whole fixture. Every write command above is called as
    // `let _ = …`, because what a write returns says nothing about whether it
    // still *encodes* the same - so the mismatch has to be collected by the
    // wire and checked here, or a changed message sails through and the
    // regression test guards nothing. Regenerate with `capture_a_session`
    // whenever a change to the bytes is deliberate.
    let drifted = drifted.lock().unwrap();
    assert!(
        drifted.is_empty(),
        "{} command(s) no longer encode what was recorded:\n  - {}",
        drifted.len(),
        drifted.join("\n  - ")
    );
}
