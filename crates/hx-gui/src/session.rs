//! The device, on its own thread.
//!
//! Talking to the hardware blocks — a preset read is a dozen round trips — so
//! the session lives on a worker and the UI speaks to it through channels. The
//! worker owns it outright: the protocol is a strictly ordered stream, and two
//! callers would interleave transfers and desynchronise it.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// What the UI asks for.
pub enum Cmd {
    Connect,
    Disconnect,
    Rename {
        index: i64,
        name: String,
    },
    MoveBlock {
        from: usize,
        to: usize,
    },
    /// Move a block into the gap just before `before`, shifting the blocks
    /// between to close ranks — what dropping it there means.
    MoveBlockBefore {
        from: usize,
        before: usize,
    },
    ListIrs,
    SetTempo(f32),
    RenameSnapshot {
        index: usize,
        name: String,
    },
    ListSetlists,
    AssignCc {
        block: i64,
        cc: i64,
    },
    /// Put a block's bypass under a footswitch, or take it off one.
    AssignBypassFootswitch {
        block: i64,
        switch: u8,
        on: bool,
    },
    /// Put a parameter under a controller.
    AssignParameter {
        block: i64,
        param: i64,
        source: hx_proto::rpc::Source,
    },
    LoadIr {
        slot: i64,
        file: std::path::PathBuf,
    },
    ClearIr(i64),
    SelectPreset(i64),
    SelectSetlist(i64),
    SelectBlock(i64),
    SetParam {
        block: i64,
        index: i64,
        value: f32,
        switch: bool,
    },
    SetEnabled {
        block: i64,
        enabled: bool,
    },
    SetModel {
        block: i64,
        model: u32,
    },
    SelectSnapshot(i64),
    ClearBlock(i64),
    /// Point an input or output somewhere else — opcode 42, the operation
    /// HX Edit's own routing clicks send.
    SetRouting {
        block: i64,
        to: i64,
    },
    /// Commit the edit buffer to the loaded preset.
    SavePreset,
    /// Copy one block over another slot.
    CopyBlock {
        from: usize,
        to: usize,
    },
    /// Copy a snapshot's settings over another, keeping its name.
    CopySnapshot {
        from: usize,
        to: usize,
    },
    /// Re-attach a split or join: the fork or merge moves to sit just before
    /// `before` in the main line.
    MoveJunction {
        junction: usize,
        before: usize,
    },
    /// Put the preset back as it was before the last document edit.
    Undo,
    /// Add a block at a position, sliding whatever is there along.
    InsertBlock {
        at: usize,
        model: u32,
    },
    /// Put back what the last undo took away.
    Redo,
    /// Flip one of the device's global settings.
    SetSetting {
        id: i64,
        on: bool,
    },
    /// Read the loaded preset and hand back its bytes, for the clipboard or a
    /// file. The document is copied verbatim rather than rebuilt from what the
    /// UI shows, because a preset carries more than the UI models.
    CopyPreset,
    /// Write a whole preset document over the loaded one.
    PastePreset(Vec<u8>),
}

/// What the worker reports.
pub enum Evt {
    Connected {
        device: String,
        presets: u16,
    },
    Disconnected,
    Presets(Vec<String>),
    Loaded {
        index: i64,
        name: String,
        firmware: String,
        tempo: Option<f32>,
        snapshots: Vec<String>,
        chain: Vec<Block>,
        layout: hx_proto::preset::Layout,
        /// Whether the edit buffer differs from the stored preset. The worker
        /// owns this: a reload follows most edits, and a reload that reset the
        /// flag made Save go grey with changes still unsaved.
        dirty: bool,
    },
    /// The edit buffer has been committed to the preset.
    Saved,
    /// Whether the worker is in the middle of a device conversation. Edits
    /// take real round trips — a document write near a second — and a window
    /// that does nothing for a second looks broken.
    Busy(bool),
    /// How many steps can be undone and redone.
    History {
        undo: usize,
        redo: usize,
    },
    /// Global settings worth showing, read when a session opens.
    Settings {
        global_eq: bool,
    },
    /// The loaded preset's bytes, in answer to `Cmd::CopyPreset`.
    Copied {
        name: String,
        blob: Vec<u8>,
    },
    Irs(Vec<(i64, String)>),
    Setlists(Vec<String>),
    Activity(String),
    Failed(String),
}

/// One slot as the UI needs it: enough to draw without holding the preset.
#[derive(Clone)]
pub struct Block {
    pub position: i64,
    /// Where an input or output is routed. `None` on everything else.
    pub routing: Option<i64>,
    /// What the slot is: an effect, or the path's input, output, split or join.
    pub kind: hx_proto::preset::Kind,
    pub model: u32,
    pub enabled: bool,
    pub values: Vec<f32>,
    /// The cab riding along with an amp, if any.
    pub paired: Option<u32>,
    pub paired_values: Vec<f32>,
}

pub fn spawn() -> (Sender<Cmd>, Receiver<Evt>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (evt_tx, evt_rx) = mpsc::channel();
    std::thread::spawn(move || {
        Worker {
            cmds: cmd_rx,
            events: evt_tx,
            device: None,
            setlist: 0,
            history: Vec::new(),
            future: Vec::new(),
            snapshot_taken: false,
            dirty: false,
            shown: (-1, String::new()),
            stumbles: 0,
        }
        .run()
    });
    (cmd_tx, evt_rx)
}

struct Worker {
    cmds: Receiver<Cmd>,
    events: Sender<Evt>,
    device: Option<hx_usb::Session>,
    /// Which setlist preset selections apply to.
    setlist: i64,
    /// Preset documents as they were before each document-level edit, newest
    /// last. Bounded — an undo history, not an archive.
    history: Vec<Vec<u8>>,
    /// States undone and therefore redoable, cleared by any fresh edit.
    future: Vec<Vec<u8>>,
    /// Whether the current burst of edits has already been snapshotted.
    ///
    /// Turning a knob is one edit per pixel of drag; a history entry each
    /// would be useless. One entry is taken for the first change after a load
    /// or a save, so undo steps back to the last known-good state — which is
    /// what "undo" means to someone who has just been turning knobs.
    snapshot_taken: bool,
    /// Whether the edit buffer differs from the stored preset. Kept here
    /// rather than in the UI because only the worker knows which reloads are
    /// fresh presets and which are edits taking effect.
    dirty: bool,
    /// The loaded preset's slot and name, as last read from the device — so a
    /// view built from a document we hold does not need a round trip to say
    /// which preset it is.
    shown: (i64, String),
    /// Keepalives missed in a row. The device goes quiet while it commits a
    /// document write, and dropping the session on the first missed beat is
    /// how a drag came to end in a disconnect.
    stumbles: u32,
}

impl Worker {
    fn run(mut self) {
        let mut last_poll = Instant::now();
        loop {
            match self.cmds.recv_timeout(Duration::from_millis(120)) {
                Ok(cmd) => {
                    // Bracket the work so the UI can say the device is being
                    // spoken to; it only shows the state when it lasts.
                    self.send(Evt::Busy(true));
                    self.handle(cmd);
                    self.send(Evt::Busy(false));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }

            // The device drops idle sessions, and it pushes front-panel
            // activity between our requests, so keep it fed and drain what it
            // sent.
            if last_poll.elapsed() > Duration::from_millis(700) {
                last_poll = Instant::now();
                self.poll();
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Connect => self.connect(),
            Cmd::Disconnect => {
                self.device = None;
                self.send(Evt::Disconnected);
            }
            Cmd::SelectPreset(index) => {
                let setlist = self.setlist;
                if self.run_on_device(|d| d.select_preset(setlist, index)) {
                    // The history belongs to the preset it was recorded on.
                    // Kept across a switch, undo would write the previous
                    // preset's document over the new one.
                    self.forget_history();
                    self.dirty = false;
                    self.reload();
                }
            }
            Cmd::SelectSetlist(index) => {
                self.setlist = index;
                if let Some(names) = self.try_on_device(|d| d.presets(index)) {
                    self.send(Evt::Presets(names));
                }
            }
            Cmd::SelectBlock(block) => {
                self.run_on_device(|d| d.select_block(block));
            }
            Cmd::SetParam {
                block,
                index,
                value,
                switch,
            } => {
                use hx_proto::msgpack::Value;
                self.snapshot();
                let wire = if switch {
                    Value::Bool(value >= 0.5)
                } else {
                    Value::F32(value)
                };
                if self.run_on_device(|d| d.set_param(block, index, wire)) {
                    self.dirty = true;
                }
            }
            Cmd::SetEnabled { block, enabled } => {
                self.snapshot();
                if self.run_on_device(|d| d.set_enabled(block, enabled)) {
                    self.dirty = true;
                }
            }
            Cmd::SetRouting { block, to } => {
                self.snapshot();
                if self.run_on_device(|d| d.set_routing(block, to)) {
                    self.dirty = true;
                    self.reload();
                }
            }
            Cmd::CopyBlock { from, to } => {
                self.edit_document(|p| {
                    let block = p.copy_slot(from).ok_or("no such block")?;
                    p.paste_slot(to, &block)
                        .then_some(())
                        .ok_or("that slot cannot hold a block")
                });
            }
            Cmd::CopySnapshot { from, to } => {
                self.edit_document(|p| {
                    let snap = p.copy_snapshot(from).ok_or("no such snapshot")?;
                    p.paste_snapshot(to, &snap)
                        .then_some(())
                        .ok_or("could not write that snapshot")
                });
            }
            Cmd::Undo => self.step_history(true),
            Cmd::Redo => self.step_history(false),
            Cmd::InsertBlock { at, model } => {
                use hx_proto::preset::Kind;

                // Two device operations, and no more: adjust the document if
                // the slot needs freeing or a branch needs attaching, then set
                // the model. Reloading in between lands a third round trip
                // while the device is still committing the write, which is
                // enough to jam it.
                let Some(mut preset) = self.read_settled() else {
                    return;
                };
                let original = preset.encode();

                // The gap before an endpoint or a junction means "at the end
                // of the lane that finishes here" — you cannot put a pedal on
                // the output itself, which is what asking for that slot did.
                let holds_blocks = preset
                    .slots
                    .get(at)
                    .is_some_and(|s| matches!(s.kind, Kind::Block | Kind::Empty));
                let Some(bounds) =
                    preset.lane_bounds(if holds_blocks { at } else { at.max(1) - 1 })
                else {
                    return self.send(Evt::Failed("nothing can go there".into()));
                };

                let free_in_lane = |p: &hx_proto::Preset| {
                    bounds
                        .clone()
                        .find(|i| p.slots.get(*i).is_some_and(|s| s.model.is_none()))
                };
                let target = if holds_blocks {
                    at
                } else {
                    match free_in_lane(&preset) {
                        Some(slot) => slot,
                        None => {
                            return self.send(Evt::Failed(
                                "this row is full — remove a block first".into(),
                            ))
                        }
                    }
                };

                // The first block on an empty branch should parallel the whole
                // line: fork just after the input, merge just before the
                // output. The device initialises the attach points itself when
                // the model lands — to zero, which parallels nothing — so ours
                // have to be written *after* the model, not before. Verified
                // against the hardware; later inserts leave them alone.
                let layout = preset.layout();
                let claim = layout
                    .paths
                    .iter()
                    .find(|p| {
                        p.split
                            .zip(p.join)
                            .is_some_and(|(s, j)| (s + 1..j).contains(&target))
                    })
                    .filter(|p| {
                        (p.split.unwrap() + 1..p.join.unwrap())
                            .all(|s| preset.slots.get(s).is_none_or(|s| s.model.is_none()))
                    })
                    .and_then(|p| Some((p.split?, p.join?, p.input? + 1, p.output?)));

                if preset.slots.get(target).is_some_and(|s| s.model.is_some()) {
                    if !preset.make_room(target, bounds) {
                        return self.send(Evt::Failed(
                            "this row is full — remove a block first".into(),
                        ));
                    }
                    if !self.run_on_device(|d| d.write_preset(&preset)) {
                        return;
                    }
                }

                if self.run_on_device(|d| d.set_model(target as i64, model)) {
                    if let Some((split, join, fork_at, merge_at)) = claim {
                        self.run_on_device(|d| {
                            let mut p = d.read_preset()?;
                            if p.set_attach(split, fork_at) && p.set_attach(join, merge_at) {
                                d.write_preset(&p)?;
                            }
                            Ok(())
                        });
                    }
                    self.dirty = true;
                    self.history.push(original);
                    if self.history.len() > 32 {
                        self.history.remove(0);
                    }
                    self.future.clear();
                    self.report_history();
                    self.reload();
                }
            }
            Cmd::AssignBypassFootswitch { block, switch, on } => {
                self.snapshot();
                let ok = if on {
                    self.run_on_device(|d| d.assign_bypass_footswitch(block, switch))
                } else {
                    self.run_on_device(|d| d.unassign_bypass_footswitch(block, switch))
                };
                if ok {
                    self.dirty = true;
                    let verb = if on { "assigned to" } else { "taken off" };
                    self.send(Evt::Activity(format!("bypass {verb} footswitch {switch}")));
                }
            }
            Cmd::AssignParameter {
                block,
                param,
                source,
            } => {
                self.snapshot();
                if self.run_on_device(|d| d.assign_parameter(block, param, source)) {
                    self.dirty = true;
                    self.send(Evt::Activity(format!("assigned to {}", source.label())));
                }
            }
            Cmd::SetSetting { id, on } => {
                if self.run_on_device(|d| d.set_object(id, hx_proto::msgpack::Value::Bool(on))) {
                    self.send(Evt::Activity(format!("setting {id} is now {on}")));
                }
            }
            Cmd::SavePreset => {
                let Some((setlist, index, name)) =
                    self.device.as_mut().and_then(|d| d.preset_info().ok())
                else {
                    return self.send(Evt::Failed("no preset loaded".into()));
                };
                if self.run_on_device(|d| d.save_preset(setlist, index, &name)) {
                    self.dirty = false;
                    self.send(Evt::Activity(format!("saved {name}")));
                    self.send(Evt::Saved);
                    // The saved state is the new baseline to undo back to.
                    self.snapshot_taken = false;
                }
            }
            Cmd::CopyPreset => {
                if let Some(blob) = self.preset_bytes() {
                    let name = self.preset_name();
                    self.send(Evt::Copied { name, blob });
                }
            }
            Cmd::PastePreset(blob) => self.paste(&blob),
            Cmd::ClearBlock(block) => {
                // Removing a pedal deserves its own undo step, not a shared
                // one with whatever knob was last turned.
                let recorded = self.record_history();
                if self.run_on_device(|d| d.clear_block(block)) {
                    self.dirty = true;
                    self.reload();
                } else if recorded {
                    self.history.pop();
                    self.report_history();
                }
            }
            Cmd::SelectSnapshot(index) => {
                if self.run_on_device(|d| d.select_snapshot(index)) {
                    self.reload();
                }
            }
            Cmd::Rename { index, name } => {
                if self.run_on_device(|d| d.rename_preset(0, index, &name)) {
                    self.reload();
                }
            }
            Cmd::MoveBlock { from, to } => {
                self.edit_document(|p| {
                    p.move_slot(from, to)
                        .then_some(())
                        .ok_or("that block cannot move there")
                });
            }
            Cmd::MoveBlockBefore { from, before } => {
                self.edit_document(|p| {
                    // A block dropped onto an empty branch claims the whole
                    // line, the same as adding one there — and because this is
                    // a document move rather than a model change, the claim
                    // rides in the same write with nothing to reset it.
                    let layout = p.layout();
                    if let Some(path) = layout.paths.iter().find(|path| {
                        path.split
                            .zip(path.join)
                            .is_some_and(|(s, j)| (s + 1..j).contains(&before))
                    }) {
                        let (split, join) = (path.split.unwrap(), path.join.unwrap());
                        let vacant = (split + 1..join)
                            .all(|s| p.slots.get(s).is_none_or(|x| x.model.is_none()));
                        if vacant {
                            if let (Some(input), Some(output)) = (path.input, path.output) {
                                p.set_attach(split, input + 1);
                                p.set_attach(join, output);
                            }
                        }
                    }
                    p.insert_slot(from, before)
                        .then_some(())
                        .ok_or("there is no room for it there")
                });
            }
            Cmd::MoveJunction { junction, before } => {
                self.edit_document(|p| {
                    p.set_attach(junction, before)
                        .then_some(())
                        .ok_or("only a fork or merge can be moved")
                });
            }
            Cmd::SetTempo(bpm) => {
                self.snapshot();
                if self.run_on_device(|d| d.set_tempo(bpm)) {
                    self.dirty = true;
                    self.reload();
                }
            }
            Cmd::RenameSnapshot { index, name } => {
                self.snapshot();
                if self.run_on_device(|d| d.rename_snapshot(index, &name)) {
                    self.dirty = true;
                    self.reload();
                }
            }
            Cmd::AssignCc { block, cc } => {
                self.snapshot();
                if self.run_on_device(|d| d.assign_bypass_cc(block, cc)) {
                    self.dirty = true;
                }
            }
            Cmd::ListSetlists => {
                if let Some(names) = self.try_on_device(|d| d.setlists()) {
                    self.send(Evt::Setlists(names));
                }
            }
            Cmd::ListIrs => {
                if let Some(slots) = self.try_on_device(|d| d.irs()) {
                    self.send(Evt::Irs(slots));
                }
            }
            Cmd::LoadIr { slot, file } => {
                let loaded = self.try_on_device(|d| {
                    let wav = crate::wav::read(&file)?;
                    let name = file
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("impulse")
                        .chars()
                        .take(20)
                        .collect::<String>();
                    d.upload_ir(slot, &name, &wav.samples)
                });
                if loaded.is_some() {
                    self.handle(Cmd::ListIrs);
                }
            }
            Cmd::ClearIr(slot) => {
                if self.run_on_device(|d| d.clear_ir(slot)) {
                    self.handle(Cmd::ListIrs);
                }
            }
            Cmd::SetModel { block, model } => {
                self.snapshot();
                if self.run_on_device(|d| d.set_model(block, model)) {
                    self.dirty = true;
                    self.reload();
                }
            }
        }
    }

    fn connect(&mut self) {
        let found = match hx_usb::list() {
            Ok(devices) => devices.into_iter().next(),
            Err(e) => return self.send(Evt::Failed(e.to_string())),
        };
        let Some(found) = found else {
            return self.send(Evt::Failed(
                "No HX device found — check the USB cable.".into(),
            ));
        };
        // Retry once: the device ignores a fresh session's opening handshake
        // on roughly every other attempt. The CLI does the same.
        let opened = found.open().or_else(|_| found.open());
        match opened {
            Ok(session) => {
                let profile = session.profile;
                self.device = Some(session);
                // A fresh session starts with a clean slate: whatever history
                // was recorded belongs to whatever was connected before.
                self.forget_history();
                self.dirty = false;
                self.send(Evt::Connected {
                    device: profile.name.to_owned(),
                    presets: profile.presets,
                });
                // Load the chain first: it is the view the user is looking at.
                // The name list is a nicety and rides on the control channel,
                // which is the flakier of the two.
                self.reload();
                if let Some(hx_proto::msgpack::Value::Bool(on)) =
                    self.device.as_mut().and_then(|d| d.object(203).ok())
                {
                    self.send(Evt::Settings { global_eq: on });
                }
                // The name list rides on the flakier control channel, so give
                // it the same second chance the session itself gets.
                let names = self
                    .try_on_device(|d| d.presets(0))
                    .or_else(|| self.try_on_device(|d| d.presets(0)));
                if let Some(names) = names {
                    self.send(Evt::Presets(names));
                }
            }
            Err(e) => self.send(Evt::Failed(e.to_string())),
        }
    }

    /// Remember the preset as it stands, unless this burst already did.
    fn snapshot(&mut self) {
        if self.snapshot_taken {
            return;
        }
        if self.record_history() {
            self.snapshot_taken = true;
        }
    }

    /// Push the preset as it stands onto the undo stack, unconditionally.
    /// Returns whether there was a preset to record.
    fn record_history(&mut self) -> bool {
        let Some(document) = self.device.as_mut().and_then(|d| d.read_preset().ok()) else {
            return false;
        };
        self.history.push(document.encode());
        if self.history.len() > 32 {
            self.history.remove(0);
        }
        // A fresh edit is a new branch; anything undone is unreachable now.
        self.future.clear();
        self.report_history();
        true
    }

    /// Drop the whole history, for when what it records no longer exists.
    fn forget_history(&mut self) {
        self.history.clear();
        self.future.clear();
        self.snapshot_taken = false;
        self.report_history();
    }

    fn report_history(&self) {
        let _ = self.events.send(Evt::History {
            undo: self.history.len(),
            redo: self.future.len(),
        });
    }

    /// Move one step back or forward through the document history.
    fn step_history(&mut self, back: bool) {
        let (from, to) = if back {
            (&mut self.history, &mut self.future)
        } else {
            (&mut self.future, &mut self.history)
        };
        let Some(document) = from.pop() else {
            let what = if back { "undo" } else { "redo" };
            return self.send(Evt::Activity(format!("nothing to {what}")));
        };
        let Some(preset) = hx_proto::Preset::parse(&document) else {
            return self.send(Evt::Failed("the history is corrupt".into()));
        };
        // Keep the current state on the other stack so the step is reversible.
        let current = self.device.as_mut().and_then(|d| d.read_preset().ok());
        if let Some(current) = current {
            to.push(current.encode());
        }
        if self.run_on_device(|d| d.write_preset(&preset)) {
            // The buffer now differs from the stored preset — almost always,
            // and "save available after undo" errs on the side of not losing
            // the state someone deliberately stepped to.
            self.dirty = true;
            self.send(Evt::Activity(if back { "undone" } else { "redone" }.into()));
            self.report_history();
            self.present(&preset);
        }
    }

    /// Apply a change to the whole preset document, keeping an undo step.
    ///
    /// Document edits are all-or-nothing: the device takes the new document or
    /// the preset is lost, so the original is kept first and only a successful
    /// modification is sent.
    fn edit_document<F>(&mut self, change: F)
    where
        F: FnOnce(&mut hx_proto::Preset) -> Result<(), &'static str>,
    {
        // Settled: this read may land while the previous edit's write is
        // still committing, and a quick second drag deserves patience too.
        let Some(mut preset) = self.read_settled() else {
            return;
        };
        let original = preset.encode();
        if let Err(why) = change(&mut preset) {
            return self.send(Evt::Failed(why.into()));
        }
        if self.run_on_device(|d| d.write_preset(&preset)) {
            self.dirty = true;
            // Bounded: this is an undo history, not an archive.
            self.history.push(original);
            if self.history.len() > 32 {
                self.history.remove(0);
            }
            // A fresh edit is a new branch; anything undone is unreachable now.
            self.future.clear();
            self.report_history();
            // What was written is what there is — no read-back to race.
            self.present(&preset);
        }
    }

    /// The loaded preset's name, or an empty string if the device will not say.
    fn preset_name(&mut self) -> String {
        self.device
            .as_mut()
            .and_then(|d| d.preset_info().ok())
            .map(|(_, _, name)| name)
            .unwrap_or_default()
    }

    /// The loaded preset exactly as the device holds it.
    fn preset_bytes(&mut self) -> Option<Vec<u8>> {
        let device = self.device.as_mut()?;
        match device.read_preset() {
            Ok(preset) => Some(preset.encode()),
            Err(e) => {
                self.send(Evt::Failed(format!("reading the preset: {e}")));
                None
            }
        }
    }

    /// Write a whole preset document over the loaded one.
    ///
    /// The bytes are parsed first. A malformed document is accepted by the
    /// device and then reads back as an empty preset, so refusing early is the
    /// difference between "that file is not a preset" and a wiped slot.
    fn paste(&mut self, blob: &[u8]) {
        let Some(preset) = hx_proto::Preset::parse(blob) else {
            self.send(Evt::Failed("that is not a preset file".into()));
            return;
        };
        // A paste replaces the whole document; it is exactly the kind of edit
        // someone reaches for undo after.
        let recorded = self.record_history();
        if self.run_on_device(|d| d.write_preset(&preset)) {
            self.dirty = true;
            self.present(&preset);
        } else if recorded {
            self.history.pop();
            self.report_history();
        }
    }

    /// Read the preset back from the device and show it.
    fn reload(&mut self) {
        let Some(preset) = self.read_settled() else {
            return;
        };
        if let Some(info) = self.device.as_mut().and_then(|d| d.preset_info().ok()) {
            let (_, index, name) = info;
            self.shown = (index, name);
        }
        self.present(&preset);
    }

    /// Read the preset, giving the device time to settle first if it must.
    ///
    /// A document write takes the device a moment to commit, and a read that
    /// lands inside that moment fails. That is a busy device, not a dead one:
    /// only when it stays unreachable is the session dropped.
    fn read_settled(&mut self) -> Option<hx_proto::Preset> {
        let device = self.device.as_mut()?;
        let mut last = None;
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(300));
            }
            match device.read_preset() {
                Ok(preset) => return Some(preset),
                Err(e) => last = Some(e),
            }
        }
        if let Some(e) = last {
            self.send(Evt::Failed(e.to_string()));
        }
        self.device = None;
        self.send(Evt::Disconnected);
        None
    }

    /// Show a preset document the worker already holds.
    ///
    /// Every edit used to be followed by a read-back, and the device commits
    /// writes slowly enough that the read could return the *old* document —
    /// a drag that "didn't take" — or fail outright and drop the session.
    /// For our own writes the written bytes are the truth, so the view is
    /// built from them and the wire stays quiet.
    fn present(&mut self, preset: &hx_proto::Preset) {
        let firmware = preset.firmware().unwrap_or_default();
        // Everything the signal passes through, not just the effects: HX Edit
        // draws the input, output and any split/join, and a chain without them
        // reads as though it starts nowhere.
        let chain = preset
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind != hx_proto::preset::Kind::Empty)
            .map(|(position, slot)| Block {
                position: position as i64,
                routing: preset.routing(position),
                kind: slot.kind,
                model: slot.model.unwrap_or_default(),
                enabled: slot.enabled,
                values: slot.values.clone(),
                paired: slot.paired,
                paired_values: slot.paired_values.clone(),
            })
            .collect();
        self.snapshot_taken = false;
        self.send(Evt::Loaded {
            index: self.shown.0,
            name: self.shown.1.clone(),
            firmware,
            tempo: preset.tempo(),
            snapshots: preset.snapshots(),
            chain,
            layout: preset.layout(),
            dirty: self.dirty,
        });
    }

    fn poll(&mut self) {
        let Some(device) = self.device.as_mut() else {
            return;
        };
        for (event, args) in device.poll_notifications() {
            let _ = self
                .events
                .send(Evt::Activity(format!("event {event}: {args:?}")));
        }
        // The device goes quiet while committing a write; one missed beat is
        // patience, not a dead link.
        match device.keepalive() {
            Ok(()) => self.stumbles = 0,
            Err(e) => {
                self.stumbles += 1;
                if self.stumbles >= 3 {
                    self.send(Evt::Failed(e.to_string()));
                    self.device = None;
                    self.send(Evt::Disconnected);
                }
            }
        }
    }

    /// Run something on the device, reporting failure. Returns whether it worked.
    fn run_on_device(
        &mut self,
        f: impl FnOnce(&mut hx_usb::Session) -> hx_usb::Result<()>,
    ) -> bool {
        self.try_on_device(f).is_some()
    }

    fn try_on_device<T>(
        &mut self,
        f: impl FnOnce(&mut hx_usb::Session) -> hx_usb::Result<T>,
    ) -> Option<T> {
        let device = self.device.as_mut()?;
        match f(device) {
            Ok(value) => Some(value),
            Err(e) => {
                let _ = self.events.send(Evt::Failed(e.to_string()));
                None
            }
        }
    }

    fn send(&self, evt: Evt) {
        let _ = self.events.send(evt);
    }
}
