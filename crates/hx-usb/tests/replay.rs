//! Record a live session's byte transport, then replay it with no device.
//!
//! `capture_a_session` (ignored; needs a pedal) records a fixed sequence of
//! commands to tests/fixtures/session.transcript.
//! `a_recorded_session_replays_offline` runs the *same* sequence against that
//! transcript with no hardware: every request is checked byte-for-byte against
//! what was recorded, and every response is parsed. So the command layer -
//! request encoding and response parsing both - stays regression-tested after
//! the hardware is gone. Regenerate the transcript with:
//!
//!     cargo test -p hx-usb --test replay capture_a_session -- --ignored

use std::path::PathBuf;

use hx_proto::msgpack::Value;
use hx_usb::replay::{finish, log, ReplayWire, Transcript};
use hx_usb::Session;

fn transcript_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/session.transcript")
}

struct Summary {
    name: String,
    blocks: usize,
    presets: usize,
    setlists: usize,
    irs: usize,
}

/// A fixed command sequence, run identically on capture and replay so the two
/// produce the same requests. Read-heavy, with one write so a write command's
/// encoding is covered too. Any failed command panics, failing the test.
fn exercise(s: &mut Session) -> Summary {
    let (_, _, name) = s.preset_info().expect("preset_info");
    let preset = s.read_preset().expect("read_preset");
    let presets = s.presets(0).expect("presets");
    let setlists = s.setlists().expect("setlists");
    let irs = s.irs().expect("irs");
    // A write: nudge the first block's first parameter. During capture this
    // edits the buffer (restored afterward by the caller); during replay it is
    // offline. Its request must encode identically both times.
    if let Some((position, _)) = preset.blocks().next() {
        s.set_param(position as i64, 0, Value::F32(0.42))
            .expect("set_param");
    }
    Summary {
        name,
        blocks: preset.blocks().count(),
        presets: presets.len(),
        setlists: setlists.len(),
        irs: irs.len(),
    }
}

#[test]
#[ignore = "records a live session to a fixture"]
fn capture_a_session() {
    let Some(found) = hx_usb::list().ok().and_then(|d| d.into_iter().next()) else {
        eprintln!("SKIPPED: no HX device attached");
        return;
    };
    let log = log();
    let mut session = found.open_recording(log.clone()).expect("open recording");
    let summary = exercise(&mut session);
    drop(session);

    let transcript = finish(&log);
    std::fs::create_dir_all(transcript_path().parent().unwrap()).unwrap();
    std::fs::write(transcript_path(), transcript.to_text()).unwrap();
    eprintln!(
        "recorded {} transfers; loaded {:?} ({} blocks, {} presets, {} setlists, {} irs)",
        transcript.0.len(),
        summary.name,
        summary.blocks,
        summary.presets,
        summary.setlists,
        summary.irs
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
    // the session sends is checked against what was recorded.
    let mut session = Session::replaying(Box::new(ReplayWire::new(transcript)), hx_proto::HX_STOMP)
        .expect("the handshake replays");
    let summary = exercise(&mut session);

    assert!(!summary.name.is_empty(), "a preset name came back");
    assert!(summary.presets > 0, "the preset list came back");
    assert!(recorded > 0, "the transcript held transfers");
}
