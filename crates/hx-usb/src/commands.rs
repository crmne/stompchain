//! The operations an editor performs: presets, blocks, parameters, snapshots
//! and impulse responses.
//!
//! Split from the transport deliberately. Everything here is a thin call onto
//! `Session::request` — the interesting code is the channel protocol next door,
//! and mixing the two made one file read as a protocol engine with a service
//! catalogue stapled on.

use std::time::{Duration, Instant};

use hx_proto::msgpack::Value;
use hx_proto::rpc::Message;
use hx_proto::{rpc, ChannelId, Preset};

use crate::{checksum, Error, Result, Session};

impl Session {
    /// Every preset in a setlist, in order.
    pub fn presets(&mut self, setlist: i64) -> Result<Vec<String>> {
        let result = self.request(
            ChannelId::CONTROL,
            rpc::op::LIST_PRESETS,
            hx_proto::msgmap! {
                rpc::key::SETLIST => Value::Int(setlist),
                rpc::key::ARGS => Value::Int(2),
            },
        )?;
        let Value::Array(entries) = result else {
            return Err(Error::Protocol("preset list was not an array".into()));
        };
        // Each entry is a single-key map from preset index to its details, so
        // the name is one level in regardless of what the index is.
        Ok(entries
            .iter()
            .map(|entry| match entry {
                Value::Map(fields) => fields
                    .first()
                    .and_then(|(_, v)| v.get(rpc::key::NAME))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                _ => String::new(),
            })
            .collect())
    }

    /// Set one parameter on one block.
    ///
    /// The value is in the parameter's own units; `hx-catalog` knows the range
    /// and how to display it.
    pub fn set_param(&mut self, block: i64, index: i64, value: Value) -> Result<()> {
        self.command(
            ChannelId::DATA,
            rpc::op::SET_PARAM,
            hx_proto::msgmap! {
                rpc::key::BLOCK => Value::Int(block),
                rpc::key::COMMIT => Value::Bool(true),
                rpc::key::PATH => Value::Int(0),
                rpc::key::PARAM_INDEX => Value::Int(index),
                rpc::key::VALUE => value,
            },
        )
    }

    /// Impulse response slots, as `(slot, name)`.
    ///
    /// Also the way to tell that an upload has finished: the device reports the
    /// new name here once it has written it.
    pub fn irs(&mut self) -> Result<Vec<(i64, String)>> {
        let result = self.request(
            ChannelId::CONTROL,
            rpc::op::LIST_IRS,
            hx_proto::msgmap! { rpc::key::ARGS => Value::Int(2) },
        )?;
        let Value::Array(entries) = result else {
            return Ok(Vec::new());
        };
        Ok(entries
            .iter()
            .filter_map(|e| {
                Some((
                    e.get(rpc::key::IR_SLOT)?.as_i64()?,
                    e.get(rpc::key::NAME)?.as_str()?.to_owned(),
                ))
            })
            .collect())
    }

    /// Bring the control channel up to the state HX Edit leaves it in.
    ///
    /// Opening the service is not enough: HX Edit follows it with a fixed
    /// sequence — end, setlists, presets, ready, list IRs — and the device
    /// will not service an IR upload without it. Cheap enough to do before any
    /// control-channel work.
    fn bootstrap(&mut self) -> Result<()> {
        let c = ChannelId::CONTROL;
        self.command(c, rpc::op::END, hx_proto::msgmap! {})?;
        self.request(c, rpc::op::LIST_SETLISTS, Value::Nil)?;
        self.request(
            c,
            rpc::op::LIST_PRESETS,
            hx_proto::msgmap! {
                rpc::key::SETLIST => Value::Int(0),
                rpc::key::ARGS => Value::Int(2),
            },
        )?;
        self.request(c, rpc::op::READY, Value::Nil)?;
        self.request(
            c,
            rpc::op::LIST_IRS,
            hx_proto::msgmap! { rpc::key::ARGS => Value::Int(2) },
        )?;
        self.command(c, rpc::op::BEGIN, hx_proto::msgmap! {})
    }

    /// Send an impulse response to a slot.
    ///
    /// Samples are mono `f32`. The IR is one RPC message on the control
    /// channel — 4 KB of samples for a 1024-sample file — which the transport
    /// then splits across frames like any other large message.
    ///
    /// The checksum field is reproduced from a capture and its algorithm is
    /// unknown; if the device rejects an upload, that is the first thing to
    /// suspect.
    pub fn upload_ir(&mut self, slot: i64, name: &str, samples: &[f32]) -> Result<()> {
        // The descriptor declares the stored length as 256 × 2^code samples
        // (key 115; key 114 is a multiplier the editor always sends as 1).
        // The device zero-pads shorter data to the declared length — but data
        // *longer* than declared wedges its transfer state machine hard enough
        // to need the 9V adapter pulled, so the length is checked here rather
        // than discovered there.
        let code = match samples.len() {
            0 => return Err(Error::Protocol("an empty impulse response".into())),
            1..=1024 => 2,
            1025..=2048 => 3,
            n => {
                return Err(Error::Protocol(format!(
                    "{n} samples will not fit; the device stores at most 2048"
                )))
            }
        };

        self.bootstrap()?;
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.command(
            ChannelId::CONTROL,
            rpc::op::UPLOAD_IR,
            hx_proto::msgmap! {
                rpc::key::IR_SLOT => Value::Int(slot),
                rpc::key::IR_CHECKSUM => Value::UInt(checksum(&bytes)),
                rpc::key::NAME => Value::Str(name.to_owned()),
                rpc::key::IR_FORMAT_A => Value::Int(1),
                rpc::key::IR_FORMAT_B => Value::Int(code),
                123 => Value::Bool(false),
                124 => Value::Bool(false),
                125 => Value::Int(0),
                rpc::key::IR_SAMPLES => Value::Bin(bytes, 2),
            },
        )?;

        // Opcode 9 answers "accepted", not "done": the device writes the IR to
        // flash afterwards and shows "transferring data" while it does. HX Edit
        // closes the operation with an end marker and then re-reads the slot
        // list, and doing the same is what takes the device out of that state —
        // simply waiting does not.
        self.command(ChannelId::CONTROL, rpc::op::END, hx_proto::msgmap! {})?;

        // Poll the slot list until our name shows up. That is the only honest
        // signal that the write finished rather than merely being accepted, and
        // it is what the device's "transferring data" display is tracking —
        // returning before it appears is what used to leave the unit stuck.
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if self
                .irs()?
                .iter()
                .any(|(s, n)| *s == slot && n.trim() == name.trim())
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        Err(Error::Protocol(format!(
            "the device accepted the impulse response but slot {} still does not show {name:?}; \
             it may still be writing",
            slot + 1
        )))
    }

    /// Empty an impulse response slot.
    pub fn clear_ir(&mut self, slot: i64) -> Result<()> {
        self.bootstrap()?;
        self.command(
            ChannelId::CONTROL,
            rpc::op::CLEAR_IR,
            hx_proto::msgmap! { rpc::key::IR_SLOT => Value::Int(slot) },
        )?;
        self.command(ChannelId::CONTROL, rpc::op::END, hx_proto::msgmap! {})?;
        self.irs()?;
        Ok(())
    }

    /// Empty a slot, removing whatever block is in it.
    ///
    /// The device wants the editing cursor moved to the block first. Sending
    /// the clear alone is answered as though it succeeded and changes nothing,
    /// which is a quietly misleading combination — HX Edit always selects then
    /// clears, and so do we.
    pub fn clear_block(&mut self, block: i64) -> Result<()> {
        self.select_block(block)?;
        self.command(
            ChannelId::DATA,
            rpc::op::CLEAR_BLOCK,
            hx_proto::msgmap! { rpc::key::BLOCK => Value::Int(block) },
        )
    }

    /// Switch to a snapshot, by zero-based index.
    pub fn select_snapshot(&mut self, index: i64) -> Result<()> {
        self.command(
            ChannelId::DATA,
            rpc::op::SELECT_SNAPSHOT,
            hx_proto::msgmap! { rpc::key::SNAPSHOT => Value::Int(index) },
        )
    }

    /// Write a whole preset document back to the device.
    ///
    /// Several operations — reordering blocks, switching snapshots, pasting a
    /// preset — have no dedicated opcode. This is how HX Edit performs its
    /// undo, and it is the general mechanism for anything the opcode table does
    /// not cover.
    ///
    /// Untested against hardware: our captures only ever show the device's own
    /// documents being written back, never a modified one.
    pub fn write_preset(&mut self, preset: &Preset) -> Result<()> {
        self.command(
            ChannelId::DATA,
            rpc::op::WRITE_PRESET,
            hx_proto::msgmap! { rpc::key::DOCUMENT => Value::Bin(preset.encode(), 2) },
        )
    }

    /// Rename a preset.
    pub fn rename_preset(&mut self, setlist: i64, index: i64, name: &str) -> Result<()> {
        self.command(
            ChannelId::DATA,
            rpc::op::RENAME_PRESET,
            hx_proto::msgmap! {
                rpc::key::SETLIST => Value::Int(setlist),
                rpc::key::PRESET_INDEX => Value::Int(index),
                rpc::key::NAME => Value::Str(name.to_owned()),
            },
        )
    }

    /// Change what a block is.
    ///
    /// Like clearing, this needs the editing cursor on the block first — see
    /// [`Session::clear_block`] for why that matters.
    pub fn set_model(&mut self, block: i64, model: u32) -> Result<()> {
        self.select_block(block)?;
        self.command(
            ChannelId::DATA,
            rpc::op::SET_MODEL,
            hx_proto::msgmap! {
                rpc::key::BLOCK => Value::Int(block),
                rpc::key::MODEL_REF => hx_proto::msgmap! {
                    rpc::key::PAIRED => Value::Bool(false),
                    rpc::key::MODEL => Value::Int(model as i64),
                    rpc::key::PAIRED_MODEL => Value::Int(-1),
                },
            },
        )
    }

    /// Move the editing cursor, which the device's own screen follows.
    pub fn select_block(&mut self, block: i64) -> Result<()> {
        self.command(
            ChannelId::DATA,
            rpc::op::SELECT_BLOCK,
            hx_proto::msgmap! {
                rpc::key::BLOCK => Value::Int(block),
                rpc::key::PATH => Value::Int(0),
            },
        )
    }

    /// Assign a MIDI CC to a block's bypass.
    ///
    /// Mirrors HX Edit's Bypass/Controller Assign page. Only the bypass target
    /// has been observed, so that is all this offers rather than pretending to
    /// a generality we cannot test.
    pub fn assign_bypass_cc(&mut self, block: i64, cc: i64) -> Result<()> {
        /// Target 5 is "this block's bypass"; the other two were constant in
        /// every capture.
        const BYPASS_TARGET: i64 = 5;
        const SCOPE: i64 = 300;

        self.command(
            ChannelId::DATA,
            rpc::op::ASSIGN_CONTROLLER,
            hx_proto::msgmap! {
                rpc::key::BLOCK => Value::Int(block),
                rpc::key::ASSIGN_TARGET => Value::Int(BYPASS_TARGET),
                rpc::key::ASSIGN_SCOPE => Value::Int(SCOPE),
                rpc::key::ASSIGN_FLAGS => Value::Int(0),
                rpc::key::CC => Value::Int(cc),
            },
        )
    }

    /// Change the tempo of the loaded preset.
    ///
    /// No opcode carries tempo on its own, so this edits the preset document
    /// and writes it back.
    pub fn set_tempo(&mut self, bpm: f32) -> Result<()> {
        let mut preset = self.read_preset()?;
        if !preset.set_tempo(bpm) {
            return Err(Error::Protocol("this preset has no tempo field".into()));
        }
        self.write_preset(&preset)
    }

    /// Rename a snapshot, by zero-based index.
    pub fn rename_snapshot(&mut self, index: usize, name: &str) -> Result<()> {
        let mut preset = self.read_preset()?;
        if !preset.set_snapshot_name(index, name) {
            return Err(Error::Protocol(format!("no snapshot {}", index + 1)));
        }
        self.write_preset(&preset)
    }

    /// Setlist names.
    pub fn setlists(&mut self) -> Result<Vec<String>> {
        let result = self.request(ChannelId::CONTROL, rpc::op::LIST_SETLISTS, Value::Nil)?;
        let Value::Array(entries) = result else {
            return Ok(Vec::new());
        };
        // A setlist entry is a single-pair map from index to name — `{0:
        // 'PRESETS'}` — rather than the keyed record the preset list uses.
        Ok(entries
            .iter()
            .map(|e| match e {
                Value::Map(fields) => fields
                    .first()
                    .and_then(|(_, v)| v.as_str())
                    .unwrap_or("")
                    .to_owned(),
                other => other.as_str().unwrap_or("").to_owned(),
            })
            .collect())
    }

    /// Switch a block in or out of the signal path.
    pub fn set_enabled(&mut self, block: i64, enabled: bool) -> Result<()> {
        self.command(
            ChannelId::DATA,
            rpc::op::SET_ENABLED,
            hx_proto::msgmap! {
                rpc::key::BLOCK => Value::Int(block),
                rpc::key::ENABLED => Value::Bool(enabled),
            },
        )
    }

    /// Setlist, index and name of the preset currently loaded.
    ///
    /// The name is not part of the preset document itself, so it comes from
    /// this separate metadata call rather than from `read_preset`.
    pub fn preset_info(&mut self) -> Result<(i64, i64, String)> {
        let v = self.request(ChannelId::DATA, rpc::op::PRESET_INFO, Value::Nil)?;
        Ok((
            v.get(rpc::key::SETLIST)
                .and_then(|x| x.as_i64())
                .unwrap_or(-1),
            v.get(rpc::key::PRESET_INDEX)
                .and_then(|x| x.as_i64())
                .unwrap_or(-1),
            v.get(rpc::key::NAME)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_owned(),
        ))
    }

    /// Fetch an object by id.
    pub fn fetch(&mut self, id: i64) -> Result<Value> {
        self.request(
            ChannelId::DATA,
            rpc::op::FETCH_OBJECT,
            hx_proto::msgmap! { rpc::key::OBJECT_ID => Value::Int(id) },
        )
    }

    /// Any notifications the device has pushed since the last call.
    pub fn poll_notifications(&mut self) -> Vec<(i64, Value)> {
        let _ = self.read_once(Duration::from_millis(20));
        let mut out = Vec::new();
        for ch in self.channels.values_mut() {
            for sm in ch.reader.take_messages() {
                if let Message::Notification { event, args } = Message::from_value(sm.body) {
                    out.push((event, args));
                }
            }
        }
        out
    }
}
