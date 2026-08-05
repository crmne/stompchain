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
    /// Put the preset back as it was before the last document edit.
    Undo,
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
    },
    /// The edit buffer has been committed to the preset.
    Saved,
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
}

impl Worker {
    fn run(mut self) {
        let mut last_poll = Instant::now();
        loop {
            match self.cmds.recv_timeout(Duration::from_millis(120)) {
                Ok(cmd) => self.handle(cmd),
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
                let wire = if switch {
                    Value::Bool(value >= 0.5)
                } else {
                    Value::F32(value)
                };
                self.run_on_device(|d| d.set_param(block, index, wire));
            }
            Cmd::SetEnabled { block, enabled } => {
                self.run_on_device(|d| d.set_enabled(block, enabled));
            }
            Cmd::SetRouting { block, to } => {
                if self.run_on_device(|d| d.set_routing(block, to)) {
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
            Cmd::Undo => {
                let Some(previous) = self.history.pop() else {
                    return self.send(Evt::Activity("nothing to undo".into()));
                };
                let Some(preset) = hx_proto::Preset::parse(&previous) else {
                    return self.send(Evt::Failed("the undo history is corrupt".into()));
                };
                if self.run_on_device(|d| d.write_preset(&preset)) {
                    self.send(Evt::Activity("undone".into()));
                    self.reload();
                }
            }
            Cmd::SavePreset => {
                let Some((setlist, index, name)) =
                    self.device.as_mut().and_then(|d| d.preset_info().ok())
                else {
                    return self.send(Evt::Failed("no preset loaded".into()));
                };
                if self.run_on_device(|d| d.save_preset(setlist, index, &name)) {
                    self.send(Evt::Activity(format!("saved {name}")));
                    self.send(Evt::Saved);
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
                if self.run_on_device(|d| d.clear_block(block)) {
                    self.reload();
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
                let moved = self.try_on_device(|d| {
                    let mut preset = d.read_preset()?;
                    if preset.swap_slots(from, to) {
                        d.write_preset(&preset)?;
                    }
                    Ok(())
                });
                if moved.is_some() {
                    self.reload();
                }
            }
            Cmd::SetTempo(bpm) => {
                if self.run_on_device(|d| d.set_tempo(bpm)) {
                    self.reload();
                }
            }
            Cmd::RenameSnapshot { index, name } => {
                if self.run_on_device(|d| d.rename_snapshot(index, &name)) {
                    self.reload();
                }
            }
            Cmd::AssignCc { block, cc } => {
                self.run_on_device(|d| d.assign_bypass_cc(block, cc));
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
                if self.run_on_device(|d| d.set_model(block, model)) {
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
                self.send(Evt::Connected {
                    device: profile.name.to_owned(),
                    presets: profile.presets,
                });
                // Load the chain first: it is the view the user is looking at.
                // The name list is a nicety and rides on the control channel,
                // which is the flakier of the two.
                self.reload();
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

    /// Apply a change to the whole preset document, keeping an undo step.
    ///
    /// Document edits are all-or-nothing: the device takes the new document or
    /// the preset is lost, so the original is kept first and only a successful
    /// modification is sent.
    fn edit_document<F>(&mut self, change: F)
    where
        F: FnOnce(&mut hx_proto::Preset) -> Result<(), &'static str>,
    {
        let Some(device) = self.device.as_mut() else {
            return;
        };
        let mut preset = match device.read_preset() {
            Ok(p) => p,
            Err(e) => return self.send(Evt::Failed(e.to_string())),
        };
        let original = preset.encode();
        if let Err(why) = change(&mut preset) {
            return self.send(Evt::Failed(why.into()));
        }
        if self.run_on_device(|d| d.write_preset(&preset)) {
            // Bounded: this is an undo history, not an archive.
            self.history.push(original);
            if self.history.len() > 32 {
                self.history.remove(0);
            }
            self.reload();
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
        if self.run_on_device(|d| d.write_preset(&preset)) {
            self.reload();
        }
    }

    fn reload(&mut self) {
        let Some(device) = self.device.as_mut() else {
            return;
        };
        let preset = match device.read_preset() {
            Ok(p) => p,
            // A failed read means the link is not healthy. Keeping the session
            // open and continuing to poll it only adds load to a device that is
            // already struggling, so drop it.
            Err(e) => {
                self.send(Evt::Failed(e.to_string()));
                self.device = None;
                self.send(Evt::Disconnected);
                return;
            }
        };
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
        let layout = preset.layout();
        let tempo = preset.tempo();
        let snapshots = preset.snapshots();
        let (index, name) = device
            .preset_info()
            .map(|(_, i, n)| (i, n))
            .unwrap_or((-1, String::new()));
        self.send(Evt::Loaded {
            index,
            name,
            firmware,
            tempo,
            snapshots,
            chain,
            layout,
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
        if let Err(e) = device.keepalive() {
            self.send(Evt::Failed(e.to_string()));
            self.device = None;
            self.send(Evt::Disconnected);
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
