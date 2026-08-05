//! Re-encoding a preset must be byte-exact.
//!
//! A preset carries a table of byte offsets into itself, so any change in
//! length — an integer written narrower, a string tag shrunk — leaves those
//! offsets pointing at the wrong places. The device accepts the document and
//! then reads the preset as empty, which is a hard failure to diagnose from the
//! outside. This test turns it into a diff.
//!
//! The fixture is a real preset captured from an HX Stomp.

use hx_proto::msgpack::{Decoder, Encoder};

const PRESET: &[u8] = include_bytes!("preset.bin");

/// Report where two encodings first diverge, with context either side.
fn first_difference(original: &[u8], reencoded: &[u8]) -> Option<String> {
    let at = original
        .iter()
        .zip(reencoded)
        .position(|(a, b)| a != b)
        .or_else(|| {
            (original.len() != reencoded.len()).then_some(original.len().min(reencoded.len()))
        })?;

    let from = at.saturating_sub(8);
    Some(format!(
        "diverges at byte {at} of {} (re-encoded {} bytes)\n  original:   {}\n  re-encoded: {}",
        original.len(),
        reencoded.len(),
        original[from..(at + 8).min(original.len())]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
        reencoded[from..(at + 8).min(reencoded.len())]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
    ))
}

#[test]
fn a_captured_preset_re_encodes_byte_for_byte() {
    let values = Decoder::decode_all(PRESET).expect("the fixture decodes");

    let mut out = Vec::new();
    for value in &values {
        out.extend(Encoder::encode(value));
    }

    if let Some(where_) = first_difference(PRESET, &out) {
        panic!("re-encoding is not byte-exact, so writing a preset back corrupts it\n{where_}");
    }
}
