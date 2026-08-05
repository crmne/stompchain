//! Layer 3 and 4: the per-channel byte stream and the MessagePack RPC on top.

use crate::msgpack::{Decoder, Encoder, Key, Value};

/// Field keys. The protocol uses bare integers, so naming them is the
/// difference between readable code and a wall of magic numbers.
pub mod key {
    /// Request: opcode.
    pub const OPCODE: i64 = 100;
    /// Request: arguments.
    pub const ARGS: i64 = 101;
    /// Request and response: transaction id.
    pub const TXN: i64 = 102;
    /// Response: how the request completed. Not an error code — a successful
    /// select-preset answers 1 and a preset read answers 0.
    pub const STATUS: i64 = 103;
    /// Response: result payload.
    pub const RESULT: i64 = 104;
    /// Signed error code inside a status-255 reply's result.
    pub const ERROR_CODE: i64 = 111;
    /// Notification: event id.
    pub const EVENT: i64 = 105;
    /// Notification: event arguments.
    pub const EVENT_ARGS: i64 = 106;

    /// Setlist number.
    pub const SETLIST: i64 = 107;
    /// Preset index within the setlist, zero-based and linear — index 7 is the
    /// slot the front panel calls 03B, not "bank 7".
    pub const PRESET_INDEX: i64 = 108;
    pub const NAME: i64 = 109;
    pub const OBJECT_ID: i64 = 118;
    pub const VALUE: i64 = 119;
    /// Slot position in the signal chain.
    pub const BLOCK: i64 = 98;
    /// Parameter position within the block's model, in `Helix.sym` order.
    pub const PARAM_INDEX: i64 = 28;
    /// Signal path the block sits on. Zero on a single-path device.
    pub const PATH: i64 = 26;
    /// Present and true on every parameter write we have seen. Purpose
    /// unknown, so it is replayed rather than omitted.
    pub const COMMIT: i64 = 29;
    /// True when a block is in the signal path. Note the polarity: this is
    /// "enabled", not "bypassed", which is the opposite of how the front panel
    /// describes it.
    pub const ENABLED: i64 = 59;
    /// Model number, inside a model descriptor.
    pub const MODEL: i64 = 25;
    /// The model descriptor itself, on a set-model request. Note this reuses
    /// the number that means "opcode" at the top level of a message.
    pub const MODEL_REF: i64 = 100;
    /// Whether the slot holds a paired model, e.g. an amp with its cab.
    pub const PAIRED: i64 = 23;
    /// The paired model's number, or -1 for none.
    pub const PAIRED_MODEL: i64 = 26;
    /// Preset document on a write-preset request.
    pub const DOCUMENT: i64 = 110;
    /// Zero-based snapshot index.
    pub const SNAPSHOT: i64 = 92;

    /// MIDI CC number on a controller assignment.
    /// Routing destination on opcode 42.
    pub const ROUTING: i64 = 51;
    pub const CC: i64 = 71;
    /// What is being controlled. 5 is a block's bypass.
    pub const ASSIGN_TARGET: i64 = 95;
    /// Two further fields on an assignment whose meaning is not known; they
    /// were constant across every assignment captured.
    pub const ASSIGN_SCOPE: i64 = 96;
    pub const ASSIGN_FLAGS: i64 = 74;

    /// Zero-based impulse response slot.
    pub const IR_SLOT: i64 = 112;
    /// Accompanies an IR upload. Looks like a checksum — it changed between two
    /// uploads of the same length — but the algorithm is not known, so it is
    /// sent as observed and may need to be right. **Unverified.**
    pub const IR_CHECKSUM: i64 = 113;
    /// Sample data: little-endian `f32`, mono.
    pub const IR_SAMPLES: i64 = 110;
    /// Format descriptors, seen as 1 and 3. Meaning not isolated.
    pub const IR_FORMAT_A: i64 = 114;
    pub const IR_FORMAT_B: i64 = 115;
}

/// Operation codes carried in [`key::OPCODE`].
pub mod op {
    /// `{107: setlist, 101: 2}` — every preset name in a setlist.
    pub const LIST_PRESETS: i64 = 1;
    /// `{107: setlist, 108: preset index}`
    pub const SELECT_PRESET: i64 = 20;
    /// Read the currently loaded preset as an `l6-helix` document.
    pub const READ_PRESET: i64 = 22;
    /// Metadata for the current preset: bank, slot and name.
    pub const PRESET_INFO: i64 = 23;
    /// `{118: id}` — fetch an object by id.
    pub const FETCH_OBJECT: i64 = 24;
    /// `{98: block, 29: true, 26: path, 28: index, 119: value}`
    pub const SET_PARAM: i64 = 30;
    /// `{98: block, 59: enabled}`
    pub const SET_ENABLED: i64 = 41;
    /// Route an endpoint: `{98: slot, 51: destination}`.
    pub const SET_ROUTING: i64 = 42;
    /// `{107: setlist, 108: preset index, 109: name}`
    pub const RENAME_PRESET: i64 = 6;
    /// Setlist names. Control channel.
    pub const LIST_SETLISTS: i64 = 0;
    /// `{101: 2}` — impulse response slots. Control channel.
    pub const LIST_IRS: i64 = 13;
    /// Opens and closes each control-channel exchange in HX Edit's traffic.
    pub const BEGIN: i64 = 255;
    pub const END: i64 = 254;
    /// Sent once during control-channel setup; purpose unknown.
    pub const READY: i64 = 112;
    /// `{110: document}` — write a whole preset back.
    pub const WRITE_PRESET: i64 = 21;
    /// Assign a MIDI controller to a block's parameter.
    ///
    /// `{98: block, 95: target, 96: ?, 74: ?, 71: cc}`. Captured by stepping
    /// HX Edit's MIDI In field on a block's Bypass, where only key 71 moved —
    /// so that is the CC number and the rest describe what is being controlled.
    /// Keys 95, 96 and 74 are replayed as observed. **[inferred]**
    pub const ASSIGN_CONTROLLER: i64 = 37;
    /// `{98: block}` — empty a slot.
    pub const CLEAR_BLOCK: i64 = 28;
    /// Upload an impulse response. Control channel.
    pub const UPLOAD_IR: i64 = 9;
    /// `{112: slot}` — empty an impulse response slot. Control channel.
    pub const CLEAR_IR: i64 = 15;
    /// `{98: block, 100: {23: paired, 25: model, 26: second model or -1}}`
    pub const SET_MODEL: i64 = 40;
    /// `{98: block, 26: path}` — move the editing cursor, which the front panel
    /// follows.
    pub const SELECT_BLOCK: i64 = 78;
    /// `{92: index}` — switch snapshot.
    pub const SELECT_SNAPSHOT: i64 = 88;
}

/// Transaction ids start here on each channel and count up.
pub const FIRST_TXN: i64 = 1000;

/// A decoded application-layer message.
#[derive(Debug, Clone)]
pub enum Message {
    Request {
        txn: i64,
        opcode: i64,
        args: Value,
    },
    Response {
        txn: i64,
        status: i64,
        result: Value,
    },
    Notification {
        event: i64,
        args: Value,
    },
    /// Well-formed MessagePack that matches none of the three shapes.
    Other(Value),
}

impl Message {
    pub fn from_value(v: Value) -> Message {
        let get = |k: i64| v.get(k).cloned().unwrap_or(Value::Nil);
        match (v.get(key::TXN), v.get(key::EVENT)) {
            (Some(t), _) if v.get(key::OPCODE).is_some() => Message::Request {
                txn: t.as_i64().unwrap_or(0),
                opcode: get(key::OPCODE).as_i64().unwrap_or(-1),
                args: get(key::ARGS),
            },
            (Some(t), _) if v.get(key::STATUS).is_some() => Message::Response {
                txn: t.as_i64().unwrap_or(0),
                status: get(key::STATUS).as_i64().unwrap_or(-1),
                result: get(key::RESULT),
            },
            (None, Some(e)) => Message::Notification {
                event: e.as_i64().unwrap_or(-1),
                args: get(key::EVENT_ARGS),
            },
            _ => Message::Other(v),
        }
    }

    /// Field order matches HX Edit's own — transaction id first — so encoded
    /// messages are byte-identical to captured ones.
    pub fn to_value(&self) -> Value {
        let m = match self {
            Message::Request { txn, opcode, args } => vec![
                (Key::Int(key::TXN), Value::Int(*txn)),
                (Key::Int(key::OPCODE), Value::Int(*opcode)),
                (Key::Int(key::ARGS), args.clone()),
            ],
            Message::Response {
                txn,
                status,
                result,
            } => vec![
                (Key::Int(key::TXN), Value::Int(*txn)),
                (Key::Int(key::STATUS), Value::Int(*status)),
                (Key::Int(key::RESULT), result.clone()),
            ],
            Message::Notification { event, args } => vec![
                (Key::Int(key::EVENT), Value::Int(*event)),
                (Key::Int(key::EVENT_ARGS), args.clone()),
            ],
            Message::Other(v) => return v.clone(),
        };
        Value::Map(m)
    }

    pub fn encode(&self) -> Vec<u8> {
        Encoder::encode(&self.to_value())
    }
}

/// Read a preset index from either a bare number or a front-panel label.
///
/// The tools print `03B`, so they should accept it. Requiring the reader to
/// convert back to 7 by hand is a small cruelty.
pub fn parse_slot(text: &str) -> Option<i64> {
    if let Ok(index) = text.trim().parse::<i64>() {
        return (index >= 0).then_some(index);
    }
    let text = text.trim();
    let (bank, slot) = text.split_at(text.len().checked_sub(1)?);
    let letter = slot.chars().next()?.to_ascii_uppercase();
    let position = "ABC".find(letter)? as i64;
    let bank: i64 = bank.parse().ok()?;
    (bank >= 1).then(|| (bank - 1) * 3 + position)
}

/// Render a preset index the way the hardware labels it: `03B` for index 7.
pub fn slot_label(index: i64) -> String {
    format!(
        "{:02}{}",
        index / 3 + 1,
        b"ABC"[(index % 3) as usize] as char
    )
}

// ------------------------------------------------------------ stream framing ---

/// One message in a channel's byte stream, with the 8-byte prefix that
/// delimits it.
#[derive(Debug, Clone)]
pub struct StreamMessage {
    pub flags: u16,
    pub service: u16,
    pub body: Value,
}

/// Reassembles a channel's byte stream and yields whole messages.
///
/// The stream is chunked arbitrarily across USB transfers, so a message can
/// straddle any number of them. Bytes are buffered until a complete
/// length-prefixed message is present.
#[derive(Default)]
pub struct StreamReader {
    buf: Vec<u8>,
}

impl StreamReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the bytes that followed a channel header.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Whole messages that have arrived, consuming them from the buffer.
    pub fn take_messages(&mut self) -> Vec<StreamMessage> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        while self.buf.len() >= pos + 8 {
            // Originator tag: 1 from the host, 0 from the device. Kept for
            // symmetry but never gated on.
            let flags = u16::from_le_bytes([self.buf[pos], self.buf[pos + 1]]);
            // Nominally the service id, but the device sends uninitialised
            // memory here on some replies — the same reply arrives with
            // different values across captures. Only the length can be
            // trusted, so this is carried, not checked.
            let service = u16::from_le_bytes([self.buf[pos + 2], self.buf[pos + 3]]);
            let len = u32::from_le_bytes([
                self.buf[pos + 4],
                self.buf[pos + 5],
                self.buf[pos + 6],
                self.buf[pos + 7],
            ]) as usize;
            if self.buf.len() < pos + 8 + len {
                break; // wait for more bytes
            }
            let body = &self.buf[pos + 8..pos + 8 + len];
            match Decoder::new(body).value() {
                Ok(v) => out.push(StreamMessage {
                    flags,
                    service,
                    body: v,
                }),
                // A body that will not parse means our framing is wrong; stop
                // rather than silently resynchronising on garbage.
                Err(_) => break,
            }
            pos += 8 + len;
        }
        self.buf.drain(..pos);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real select-preset request captured from HX Edit.
    const SELECT_PRESET: &[u8] = &[
        0x83, 0x66, 0xcd, 0x03, 0xf1, 0x64, 0x14, 0x65, 0x82, 0x6b, 0x00, 0x6c, 0x0c,
    ];

    #[test]
    fn recognises_a_request() {
        let v = Decoder::new(SELECT_PRESET).value().unwrap();
        match Message::from_value(v) {
            Message::Request { txn, opcode, args } => {
                assert_eq!((txn, opcode), (1009, op::SELECT_PRESET));
                assert_eq!(args.get(key::PRESET_INDEX).unwrap().as_i64(), Some(12));
            }
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn a_request_round_trips_byte_for_byte() {
        // Encoding must reproduce HX Edit's own field order, or captured
        // traffic and generated traffic stop being comparable.
        let request = Message::Request {
            txn: 1009,
            opcode: op::SELECT_PRESET,
            args: crate::msgmap! {
                key::SETLIST => Value::Int(0),
                key::PRESET_INDEX => Value::Int(12),
            },
        };
        assert_eq!(request.encode(), SELECT_PRESET);

        let back = Message::from_value(Decoder::new(&request.encode()).value().unwrap());
        assert!(matches!(
            back,
            Message::Request {
                txn: 1009,
                opcode: 20,
                ..
            }
        ));
    }

    #[test]
    fn stream_reader_waits_for_a_whole_message() {
        // {102: 1000} preceded by its 8-byte stream prefix.
        let body = Encoder::encode(&crate::msgmap! { key::TXN => Value::Int(1000) });
        let mut framed = vec![0x00, 0x00, 0x06, 0x00];
        framed.extend_from_slice(&(body.len() as u32).to_le_bytes());
        framed.extend_from_slice(&body);

        let mut r = StreamReader::new();
        // Split mid-message: nothing should come out until it is complete.
        r.push(&framed[..6]);
        assert!(r.take_messages().is_empty());
        r.push(&framed[6..]);
        let msgs = r.take_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].service, 6);
        assert_eq!(msgs[0].body.get(key::TXN).unwrap().as_i64(), Some(1000));
    }

    #[test]
    fn slot_labels_parse_back_to_their_index() {
        for index in [0, 7, 12, 125] {
            assert_eq!(parse_slot(&slot_label(index)), Some(index), "index {index}");
        }
        assert_eq!(parse_slot("03b"), Some(7));
        assert_eq!(parse_slot("7"), Some(7));
        assert_eq!(parse_slot(" 03B "), Some(7));
        assert_eq!(parse_slot("00A"), None);
        assert_eq!(parse_slot("03D"), None);
        assert_eq!(parse_slot("nonsense"), None);
        assert_eq!(parse_slot(""), None);
    }

    #[test]
    fn slot_labels_match_the_front_panel() {
        // {107: 0, 108: 7} was captured as "CT-Sad", which the device shows as 03B.
        assert_eq!(slot_label(0), "01A");
        assert_eq!(slot_label(7), "03B");
        assert_eq!(slot_label(125), "42C");
    }
}
