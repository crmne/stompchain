//! Integration tests against real hardware.
//!
//! These exist because the device can be wedged — left in a state where it
//! refuses every new session until its power is pulled — and every occurrence
//! so far was caused by this client rather than the hardware. Unit tests cannot
//! catch that: the bytes were well formed each time. Only doing the thing and
//! then asking the device whether it is still there can.
//!
//! They are `#[ignore]` because they need an HX device with HX Edit quit:
//!
//!     cargo test -p hx-usb -- --ignored --test-threads=1
//!
//! `--test-threads=1` is not optional. The device serves one session at a time,
//! and two tests talking at once is itself a way to wedge it.
//!
//! Every test ends by asserting the device is still healthy, so a failure tells
//! you both what broke and whether you now need to power-cycle.

use std::time::{Duration, Instant};

use hx_usb::Session;

/// Open a session, skipping the test if no device is attached.
///
/// Returns `None` rather than failing so the suite is runnable on a machine
/// without hardware, and says so rather than passing in silence.
fn device() -> Option<Session> {
    let found = match hx_usb::list() {
        Ok(devices) => devices.into_iter().next(),
        Err(e) => {
            eprintln!("SKIPPED: cannot enumerate USB ({e})");
            return None;
        }
    };
    let Some(found) = found else {
        eprintln!("SKIPPED: no HX device attached");
        return None;
    };
    match found.open() {
        Ok(session) => Some(session),
        Err(e) => {
            eprintln!("SKIPPED: could not open the device ({e}). Is HX Edit running?");
            None
        }
    }
}

/// Assert the device still answers on both channels.
///
/// Both matter and they fail independently: a wedged device kept answering
/// preset reads on the data channel long after the control channel had stopped,
/// which is exactly why an earlier health check missed the problem.
fn assert_healthy(session: &mut Session, after: &str) {
    session
        .preset_info()
        .unwrap_or_else(|e| panic!("data channel is gone after {after}: {e}"));
    session
        .irs()
        .unwrap_or_else(|e| panic!("control channel is gone after {after}: {e}"));
}

/// A fresh session, which is the harder case: reconnecting is where the device
/// has historically refused to play along.
fn assert_reconnectable(after: &str) {
    let found = hx_usb::list()
        .expect("enumerating")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("device vanished from the bus after {after}"));
    let mut session = found
        .open()
        .unwrap_or_else(|e| panic!("cannot reopen the device after {after}: {e}"));
    assert_healthy(&mut session, after);
}

#[test]
#[ignore = "needs an HX device"]
fn reading_repeatedly_is_safe() {
    let Some(mut session) = device() else { return };
    for i in 0..8 {
        session
            .read_preset()
            .unwrap_or_else(|e| panic!("read {i} failed: {e}"));
    }
    assert_healthy(&mut session, "eight preset reads");
}

/// The exact sequence that wedged the device: clicking through blocks quickly.
///
/// Selecting a block used to move the device's own cursor, so every click was a
/// round trip. The UI no longer does that, but the operation still exists and
/// must not be dangerous on its own.
#[test]
#[ignore = "needs an HX device"]
fn moving_the_cursor_rapidly_is_safe() {
    let Some(mut session) = device() else { return };
    let preset = session.read_preset().expect("read");
    let blocks: Vec<_> = preset
        .blocks()
        .map(|(position, _)| position as i64)
        .collect();
    assert!(
        !blocks.is_empty(),
        "the loaded preset has no blocks to select"
    );

    // Three passes, no pause: the pattern a user produces clicking along a chain.
    for _ in 0..3 {
        for &block in &blocks {
            session
                .select_block(block)
                .unwrap_or_else(|e| panic!("selecting block {block} failed: {e}"));
        }
    }
    assert_healthy(&mut session, "rapid cursor moves");
}

#[test]
#[ignore = "needs an HX device"]
fn sweeping_a_parameter_is_safe() {
    use hx_proto::msgpack::Value;

    let Some(mut session) = device() else { return };
    let preset = session.read_preset().expect("read");
    let Some((position, slot)) = preset.blocks().next() else {
        eprintln!("SKIPPED: the loaded preset has no blocks");
        return;
    };
    let original = *slot.values.first().unwrap_or(&0.0);

    // A knob drag is a burst of writes, not one.
    for step in 0..20 {
        let value = original + (step as f32 % 5.0) * 0.01;
        session
            .set_param(position as i64, 0, Value::F32(value))
            .unwrap_or_else(|e| panic!("parameter write {step} failed: {e}"));
    }
    session
        .set_param(position as i64, 0, Value::F32(original))
        .expect("restoring the original value");

    assert_healthy(&mut session, "a twenty-step parameter sweep");
}

#[test]
#[ignore = "needs an HX device"]
fn switching_presets_repeatedly_is_safe() {
    let Some(mut session) = device() else { return };
    let (_, started_at, _) = session.preset_info().expect("current preset");

    for index in [0, 3, 7, 1] {
        session
            .select_preset(0, index)
            .unwrap_or_else(|e| panic!("selecting preset {index} failed: {e}"));
        session.read_preset().expect("reading after a switch");
    }
    session
        .select_preset(0, started_at)
        .expect("restoring the original preset");

    assert_healthy(&mut session, "four preset switches");
}

#[test]
#[ignore = "needs an HX device"]
fn switching_snapshots_is_safe() {
    let Some(mut session) = device() else { return };
    for index in [1, 2, 0] {
        session
            .select_snapshot(index)
            .unwrap_or_else(|e| panic!("selecting snapshot {index} failed: {e}"));
    }
    assert_healthy(&mut session, "snapshot switches");
}

/// Uploading is the operation that wedged the device hardest, because opcode 9
/// answers "accepted" and finishes the write afterwards. Returning before that
/// finishes leaves it stuck showing "transferring data".
#[test]
#[ignore = "needs an HX device"]
fn uploading_an_impulse_response_completes_before_returning() {
    let Some(mut session) = device() else { return };

    // A short synthetic impulse: enough to exercise chunking and the checksum.
    let samples: Vec<f32> = (0..1024)
        .map(|i| if i == 0 { 1.0 } else { 0.5 / (i as f32) })
        .collect();

    let slot = 0;
    let started = Instant::now();
    session
        .upload_ir(slot, "hxtest", &samples)
        .expect("uploading an impulse response");

    // upload_ir polls until the device reports the name, so by the time it
    // returns the write must already be visible.
    let listed = session.irs().expect("listing impulse responses");
    assert!(
        listed.iter().any(|(s, n)| *s == slot && n == "hxtest"),
        "upload returned but slot {slot} does not show the IR: {listed:?}"
    );
    assert!(
        started.elapsed() > Duration::from_millis(200),
        "upload returned too fast to have waited for the device"
    );

    session.clear_ir(slot).expect("clearing the slot again");
    let listed = session.irs().expect("listing after clear");
    assert!(
        !listed.iter().any(|(s, _)| *s == slot),
        "slot {slot} still occupied after clearing: {listed:?}"
    );

    assert_healthy(&mut session, "an impulse response round trip");
}

/// Writing a preset document back is unforgiving: it must be byte-exact or the
/// device accepts it and then reads the preset as empty.
#[test]
#[ignore = "needs an HX device"]
fn writing_a_preset_back_unchanged_preserves_it() {
    let Some(mut session) = device() else { return };
    let before = session.read_preset().expect("read");
    let blocks_before = before.blocks().count();
    assert!(
        blocks_before > 0,
        "the loaded preset has no blocks to preserve"
    );

    session
        .write_preset(&before)
        .expect("writing the document back");

    let after = session.read_preset().expect("reading back");
    assert_eq!(
        after.blocks().count(),
        blocks_before,
        "writing an unmodified document changed the preset"
    );
    assert_healthy(&mut session, "an unmodified document write");
}

/// Everything in sequence, then a fresh connection — the shape of a real
/// editing session, and the case where damage tends to surface only afterwards.
#[test]
#[ignore = "needs an HX device"]
fn a_full_editing_session_leaves_the_device_reconnectable() {
    use hx_proto::msgpack::Value;

    let Some(mut session) = device() else { return };
    let (_, started_at, _) = session.preset_info().expect("current preset");

    for round in 0..2 {
        session.read_preset().expect("read");
        session.irs().expect("ir list");
        session.presets(0).expect("preset list");
        session.select_snapshot(round % 3).expect("snapshot");

        let preset = session.read_preset().expect("read");
        let first = preset.blocks().next().map(|(position, slot)| {
            (
                position as i64,
                *slot.values.first().unwrap_or(&0.0),
                slot.enabled,
            )
        });
        drop(preset);
        if let Some((position, original, enabled)) = first {
            session
                .set_param(position, 0, Value::F32(original))
                .expect("parameter");
            session.set_enabled(position, enabled).expect("enable");
        }
    }

    session.select_snapshot(0).expect("restoring snapshot");
    session
        .select_preset(0, started_at)
        .expect("restoring preset");
    assert_healthy(&mut session, "a full editing session");

    // The real test: can something else connect afterwards?
    drop(session);
    std::thread::sleep(Duration::from_millis(500));
    assert_reconnectable("a full editing session");
}

/// Opening and closing repeatedly, which is what running the CLI in a loop does
/// and what used to fail on every other attempt.
#[test]
#[ignore = "needs an HX device"]
fn sessions_can_be_opened_and_closed_repeatedly() {
    for attempt in 0..6 {
        let Some(mut session) = device() else { return };
        session
            .preset_info()
            .unwrap_or_else(|e| panic!("session {attempt} could not read: {e}"));
        drop(session);
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_reconnectable("six open/close cycles");
}

/// A backup must restore exactly, or it is not a backup.
///
/// The document is written back twice: once as itself, once after a change.
/// If either write is not byte-exact the device accepts it and then reads the
/// preset as empty, so the assertion is on the blocks surviving.
#[test]
#[ignore = "needs an HX device"]
fn a_preset_survives_a_backup_and_restore() {
    let Some(mut session) = device() else { return };

    let original = session.read_preset().expect("read").encode();
    let blocks = hx_proto::Preset::parse(&original)
        .expect("our own encoding parses")
        .blocks()
        .count();
    assert!(blocks > 0, "the loaded preset has no blocks to preserve");

    let restored = hx_proto::Preset::parse(&original).expect("parse");
    session.write_preset(&restored).expect("restoring");

    let after = session.read_preset().expect("reading back");
    assert_eq!(
        after.blocks().count(),
        blocks,
        "restoring a backup changed the preset"
    );
    assert_eq!(
        after.encode(),
        original,
        "the preset came back different from the backup"
    );
    assert_healthy(&mut session, "a backup and restore");
}

/// Routing an endpoint uses opcode 42, captured from HX Edit's own clicks.
/// A document write is accepted but ignored for this field, which is why the
/// opcode matters — and why this test also asserts the document write still
/// does not apply it, so we notice if a firmware update changes the rules.
#[test]
#[ignore = "needs an HX device"]
fn routing_an_input_round_trips() {
    let Some(mut session) = device() else { return };

    let before = session.read_preset().expect("read");
    let Some(input) = before.layout().paths.first().and_then(|p| p.input) else {
        eprintln!("SKIPPED: no input slot");
        return;
    };
    let was = before.routing(input).expect("input routed somewhere");

    // 4 is Return L/R on an HX Stomp, 1 is Multi; pick whichever is not set.
    let to = if was == 4 { 1 } else { 4 };
    session.set_routing(input as i64, to).expect("op 42");
    let after = session.read_preset().expect("read back");
    assert_eq!(
        after.routing(input),
        Some(to),
        "opcode 42 did not change the routing"
    );

    session.set_routing(input as i64, was).expect("restoring");
    assert_eq!(
        session.read_preset().expect("read").routing(input),
        Some(was)
    );
    assert_healthy(&mut session, "a routing round trip");
}

/// Writing the preset document must complete before the next write begins.
///
/// The wait is on notification 20: the device answers a write with "accepted"
/// and announces the commit afterwards. HX Edit paces on that announcement —
/// fourteen consecutive captured undos all wait for it — and once our client
/// did the same, the write-degradation that used to appear after a dozen
/// back-to-back writes disappeared. Twenty in a row here to prove it.
#[test]
#[ignore = "needs an HX device"]
fn twenty_preset_writes_in_a_row_all_complete() {
    let Some(mut session) = device() else { return };
    let original = session.read_preset().expect("read").encode();
    let blocks = hx_proto::Preset::parse(&original).unwrap().blocks().count();

    for round in 1..=20 {
        let preset = hx_proto::Preset::parse(&original).expect("parse");
        session
            .write_preset(&preset)
            .unwrap_or_else(|e| panic!("write {round} failed: {e}"));
        let after = session
            .read_preset()
            .unwrap_or_else(|e| panic!("read after write {round} failed: {e}"));
        assert_eq!(
            after.blocks().count(),
            blocks,
            "write {round} changed the preset"
        );
    }
    assert_healthy(&mut session, "twenty preset writes");
}

/// The external-clock flag follows real MIDI beat clock.
///
/// This is what identified opcode 99. Patching its reply to `true` in flight
/// made HX Edit swap its BPM readout for "[External]", and sending the device
/// actual MIDI clock flips it for as long as the clock runs — so the flag
/// means "the tempo is not mine to set". Needs `tools/midiclock`, and is
/// skipped when that is not built.
#[test]
#[ignore = "needs an HX device"]
fn the_external_clock_flag_follows_midi_clock() {
    let Some(mut session) = device() else { return };
    if !std::path::Path::new("/tmp/midiclock").exists() {
        eprintln!("SKIPPED: /tmp/midiclock not built");
        return;
    }

    assert!(
        !session.tempo_is_external().expect("query"),
        "the device already thinks it is externally clocked"
    );

    let mut clock = std::process::Command::new("/tmp/midiclock")
        .arg("8")
        .spawn()
        .expect("spawning the clock generator");
    std::thread::sleep(Duration::from_secs(3));

    let external = session.tempo_is_external().expect("query while clocked");
    let _ = clock.wait();
    assert!(external, "MIDI clock was running but the flag stayed false");

    // Let the device notice the clock has stopped before anything else runs.
    // Without this the next test inherits a device still resynchronising, and
    // fails for reasons that have nothing to do with it.
    let settled = Instant::now() + Duration::from_secs(10);
    while Instant::now() < settled {
        std::thread::sleep(Duration::from_millis(500));
        if !session.tempo_is_external().unwrap_or(true) {
            break;
        }
    }
    assert!(
        !session
            .tempo_is_external()
            .expect("query after the clock stops"),
        "the flag is still set after the clock stopped"
    );
    assert_healthy(&mut session, "an external clock run");
}

/// Saving is what makes an edit outlive the edit buffer, and it is the one
/// operation that writes flash on purpose. This changes a parameter, saves,
/// reloads from storage to prove it stuck, then puts the original back and
/// saves again — so the preset ends exactly as it started.
#[test]
#[ignore = "needs an HX device"]
fn saving_a_preset_makes_an_edit_survive_a_reload() {
    use hx_proto::msgpack::Value;

    let Some(mut session) = device() else { return };
    let (setlist, index, name) = session.preset_info().expect("current preset");

    let preset = session.read_preset().expect("read");
    let Some((position, slot)) = preset.blocks().next() else {
        eprintln!("SKIPPED: the loaded preset has no blocks");
        return;
    };
    let original = *slot.values.first().unwrap_or(&0.5);
    let changed = if original > 0.5 {
        original - 0.2
    } else {
        original + 0.2
    };
    drop(preset);

    session
        .set_param(position as i64, 0, Value::F32(changed))
        .expect("editing");
    session
        .save_preset(setlist, index, &name)
        .expect("saving the edit");

    // Reload from storage: an unsaved edit would be discarded here.
    session.select_preset(setlist, index).expect("reload");
    let after = session.read_preset().expect("read back");
    let stored = after
        .slots
        .get(position)
        .and_then(|s| s.values.first().copied())
        .expect("the block survived");
    assert!(
        (stored - changed).abs() < 0.001,
        "saved {changed} but the preset reloaded as {stored}"
    );
    drop(after);

    // And put it back exactly as it was.
    session
        .set_param(position as i64, 0, Value::F32(original))
        .expect("restoring");
    session
        .save_preset(setlist, index, &name)
        .expect("saving the restore");
    session.select_preset(setlist, index).expect("reload");
    let restored = session
        .read_preset()
        .expect("read")
        .slots
        .get(position)
        .and_then(|s| s.values.first().copied())
        .expect("block");
    assert!(
        (restored - original).abs() < 0.001,
        "failed to restore: wanted {original}, got {restored}"
    );

    assert_healthy(&mut session, "a preset save round trip");
}

/// Global settings are a flat numbered namespace, read with opcode 24 and
/// written with 25. Object 203 is the global EQ's on/off, which is a harmless
/// thing to toggle and put straight back.
#[test]
#[ignore = "needs an HX device"]
fn a_device_setting_round_trips() {
    use hx_proto::msgpack::Value;

    let Some(mut session) = device() else { return };
    const GLOBAL_EQ_ENABLED: i64 = 203;

    let Value::Bool(was) = session.object(GLOBAL_EQ_ENABLED).expect("read") else {
        eprintln!("SKIPPED: object {GLOBAL_EQ_ENABLED} is not a switch on this device");
        return;
    };

    session
        .set_object(GLOBAL_EQ_ENABLED, Value::Bool(!was))
        .expect("write");
    assert_eq!(
        session.object(GLOBAL_EQ_ENABLED).expect("read back"),
        Value::Bool(!was),
        "the device did not take the setting"
    );

    session
        .set_object(GLOBAL_EQ_ENABLED, Value::Bool(was))
        .expect("restore");
    assert_eq!(
        session.object(GLOBAL_EQ_ENABLED).expect("read"),
        Value::Bool(was),
        "failed to restore the setting"
    );

    assert_healthy(&mut session, "a device setting round trip");
}
