//! Every real preset must survive a full read/write cycle byte-for-byte.
//!
//! These fixtures are factory presets captured off a real HX Stomp (firmware
//! 3.80) with `stompchain backup-all`, chosen to span the codec's paths: empty
//! slots, bass and keys presets, and guitar presets from the smallest to the
//! largest, across many amp models. Only Line 6 factory presets are here -
//! the kind every unit ships with - never a user's own tones.
//!
//! Why this matters after the pedal is gone: a preset carries a table of byte
//! offsets into itself, so any change in encoded length silently corrupts it,
//! and the device reads a corrupted document back as an empty preset. That
//! failure can only be reproduced against hardware. Capturing real presets as
//! fixtures turns it into an offline diff that guards the Helix codec forever,
//! including through refactors made long after the hardware is sold.

use std::fs;
use std::path::PathBuf;

use hx_proto::msgpack::{Decoder, Encoder};
use hx_proto::Preset;

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {dir:?}: {e}"))
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("hxpreset"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&p).unwrap())
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no .hxpreset fixtures found in {dir:?}");
    out
}

/// Where two byte strings first diverge, with a little context.
fn first_difference(a: &[u8], b: &[u8]) -> Option<String> {
    let at = a
        .iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .or_else(|| (a.len() != b.len()).then_some(a.len().min(b.len())))?;
    let from = at.saturating_sub(8);
    let hex = |s: &[u8]| {
        s[from..(at + 8).min(s.len())]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    Some(format!(
        "diverges at byte {at} of {} (re-encoded {})\n    original:   {}\n    re-encoded: {}",
        a.len(),
        b.len(),
        hex(a),
        hex(b),
    ))
}

/// The msgpack layer alone must re-encode identically: this is where a
/// narrower integer or a shrunk string tag would slip in.
#[test]
fn every_fixture_re_encodes_at_the_msgpack_layer() {
    let mut failures = Vec::new();
    for (name, bytes) in fixtures() {
        let values = Decoder::decode_all(&bytes)
            .unwrap_or_else(|e| panic!("{name} does not decode: {e:?}"));
        let mut out = Vec::new();
        for value in &values {
            out.extend(Encoder::encode(value));
        }
        if let Some(where_) = first_difference(&bytes, &out) {
            failures.push(format!("{name}: {where_}"));
        }
    }
    assert!(failures.is_empty(), "msgpack re-encoding is not byte-exact:\n{}", failures.join("\n\n"));
}

/// The full document layer: parse into a Preset and write it back. This is the
/// exact path `write_preset` takes, so a failure here is a preset the app would
/// corrupt on save.
#[test]
fn every_fixture_parses_and_re_encodes_byte_for_byte() {
    let mut failures = Vec::new();
    for (name, bytes) in fixtures() {
        match Preset::parse(&bytes) {
            Some(preset) => {
                let out = preset.encode();
                if let Some(where_) = first_difference(&bytes, &out) {
                    failures.push(format!("{name}: {where_}"));
                }
            }
            None => failures.push(format!("{name}: failed to parse")),
        }
    }
    assert!(failures.is_empty(), "a real preset does not round-trip:\n{}", failures.join("\n\n"));
}
