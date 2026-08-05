//! A cross-platform editor for HX hardware, laid out the way HX Edit is so the
//! muscle memory carries over: presets down the left, the signal chain across
//! the top, and the selected block's model browser and parameters below.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use egui::RichText;
use hx_catalog::{Catalog, Kind};

mod session;
mod theme;
mod wav;

pub use session::{spawn, Cmd, Evt};

pub struct App {
    to_device: Sender<Cmd>,
    from_device: Receiver<Evt>,

    catalog: Option<Catalog>,
    connection: Connection,

    device: String,
    firmware: String,
    presets: Vec<String>,
    preset_count: u16,
    preset_index: i64,
    preset_name: String,

    tempo: Option<f32>,
    snapshots: Vec<String>,
    chain: Vec<session::Block>,
    layout: hx_proto::preset::Layout,
    /// Filter for the model browser. Empty means "show the chosen category".
    search: String,
    /// A copied preset: its name, and the document verbatim. Held in the app
    /// rather than the system clipboard because it is binary, and because
    /// pasting it into a text field would only produce noise.
    clipboard: Option<(String, Vec<u8>)>,
    /// Where the bytes should go once `Cmd::CopyPreset` answers.
    pending_copy: CopyTarget,
    selected: usize,
    /// Category chosen in the browser, or none to follow the current block.
    browsing: Option<u32>,

    irs: Vec<(i64, String)>,
    setlists: Vec<String>,
    /// Which setlist the preset list is showing. Only reachable through the
    /// picker, which appears when a device has more than one — an HX Stomp has
    /// a single list, so on that hardware this stays at zero.
    #[allow(dead_code)]
    setlist: i64,
    /// Editable tempo, so typing does not fight the device's value.
    tempo_draft: Option<String>,
    /// Snapshot being renamed, with its draft name.
    snapshot_draft: Option<(usize, String)>,
    /// MIDI CC to bind to the selected block's bypass.
    assign_cc: i64,
    /// Editable copy of the preset name, so typing does not fight the device.
    renaming: Option<String>,
    log: Vec<String>,
    /// The activity log is a debugging aid, not something to look at while
    /// playing, so it stays out of the way until asked for.
    show_activity: bool,
    status: String,
    current_snapshot: usize,
}

/// Where a copied preset should end up. Reading it is the same round trip
/// either way, so the destination is remembered until the bytes come back.
enum CopyTarget {
    Clipboard,
    File(std::path::PathBuf),
}

#[derive(Debug, PartialEq)]
enum Connection {
    Offline,
    Connecting,
    Online,
}

impl App {
    /// Styling is applied once here rather than per frame: it clones and
    /// rewrites the whole `Style`, which is pure waste sixty times a second.
    pub fn new(ctx: &egui::Context, to_device: Sender<Cmd>, from_device: Receiver<Evt>) -> Self {
        theme::apply(ctx);
        let mut app = App {
            to_device,
            from_device,
            // Without HX Edit installed everything still works, just with
            // numbers where names would be.
            catalog: Catalog::load().ok(),
            connection: Connection::Offline,
            device: String::new(),
            firmware: String::new(),
            presets: Vec::new(),
            preset_count: 0,
            preset_index: -1,
            preset_name: String::new(),
            tempo: None,
            snapshots: Vec::new(),
            chain: Vec::new(),
            layout: hx_proto::preset::Layout::default(),
            search: String::new(),
            clipboard: None,
            pending_copy: CopyTarget::Clipboard,
            selected: 0,
            browsing: None,
            irs: Vec::new(),
            setlists: Vec::new(),
            setlist: 0,
            tempo_draft: None,
            snapshot_draft: None,
            assign_cc: 1,
            renaming: None,
            log: Vec::new(),
            show_activity: false,
            status: "Looking for a device…".into(),
            current_snapshot: 0,
        };
        // Connect straight away. Anyone opening this has a pedal plugged in;
        // making them press a button first is ceremony.
        let _ = app.to_device.send(Cmd::Connect);
        app.connection = Connection::Connecting;
        app
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.to_device.send(cmd);
    }

    fn drain_events(&mut self) {
        loop {
            match self.from_device.try_recv() {
                Ok(Evt::Connected { device, presets }) => {
                    self.connection = Connection::Online;
                    self.device = device;
                    self.preset_count = presets;
                    self.status = "Connected".into();
                    let _ = self.to_device.send(Cmd::ListIrs);
                    let _ = self.to_device.send(Cmd::ListSetlists);
                }
                Ok(Evt::Disconnected) => {
                    self.connection = Connection::Offline;
                    self.chain.clear();
                    self.presets.clear();
                    self.status = "Disconnected".into();
                }
                Ok(Evt::Presets(names)) => self.presets = names,
                Ok(Evt::Loaded {
                    index,
                    name,
                    firmware,
                    tempo,
                    snapshots,
                    chain,
                    layout,
                }) => {
                    self.layout = layout;
                    self.preset_index = index;
                    self.preset_name = name;
                    self.firmware = firmware;
                    self.tempo = tempo;
                    self.snapshots = snapshots;
                    self.chain = chain;
                    // Land on something editable rather than the input, which
                    // has nothing to show.
                    if !self
                        .chain
                        .get(self.selected)
                        .is_some_and(|b| self.is_effect(b))
                    {
                        self.selected = self
                            .chain
                            .iter()
                            .position(|b| self.is_effect(b))
                            .unwrap_or(0);
                    }
                    self.selected = self.selected.min(self.chain.len().saturating_sub(1));
                    self.browsing = None;
                    self.renaming = None;
                    self.tempo_draft = None;
                    self.snapshot_draft = None;
                }
                Ok(Evt::Copied { name, blob }) => {
                    let size = blob.len();
                    match std::mem::replace(&mut self.pending_copy, CopyTarget::Clipboard) {
                        CopyTarget::File(path) => match std::fs::write(&path, &blob) {
                            Ok(()) => self.note(format!("exported {name} to {}", path.display())),
                            Err(e) => self.note(format!("could not write {}: {e}", path.display())),
                        },
                        CopyTarget::Clipboard => {
                            self.clipboard = Some((name.clone(), blob));
                            self.note(format!("copied {name} ({size} bytes)"));
                        }
                    }
                }
                Ok(Evt::Irs(slots)) => self.irs = slots,
                Ok(Evt::Setlists(names)) => self.setlists = names,
                Ok(Evt::Activity(line)) => self.note(line),
                Ok(Evt::Failed(e)) => {
                    self.status = e.clone();
                    self.note(e);
                    if self.connection == Connection::Connecting {
                        self.connection = Connection::Offline;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.status = "Device thread stopped".into();
                    break;
                }
            }
        }
    }

    fn note(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > 300 {
            self.log.remove(0);
        }
    }

    /// A `file://` URI for a model's artwork, which egui loads and caches.
    /// The picture for a slot's tile.
    ///
    /// Endpoints have no model of their own — they report 0, which is a real
    /// entry in the symbol table, so asking for its artwork used to put an amp
    /// on the input tile. What they do have is a routing destination, and HX
    /// Edit draws that: a guitar for an instrument input, a jack for a 1/4"
    /// output. Those live as frames of one strip rather than separate files.
    fn artwork(&self, block: &session::Block) -> Option<theme::Art> {
        use hx_proto::preset::Kind;
        let catalog = self.catalog.as_ref()?;

        if matches!(block.kind, Kind::Input | Kind::Output) {
            let (path, frames) = catalog.endpoint_icons(block.kind == Kind::Input)?;
            // Frame 0 is a placeholder, so the destinations start at 1.
            let frame = block.routing.unwrap_or(0).max(0) as usize + 1;
            return Some(theme::Art::strip(
                format!("file://{}", path.display()),
                frame,
                frames,
            ));
        }

        let path = catalog.artwork(catalog.model_number(block.model)?)?;
        Some(theme::Art::whole(format!("file://{}", path.display())))
    }

    fn model_name(&self, model: u32) -> String {
        self.catalog
            .as_ref()
            .and_then(|c| c.model_number(model))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| format!("model {model}"))
    }

    /// What to write on a chain tile. Inputs and outputs have no model to name.
    fn slot_label(&self, block: &session::Block) -> String {
        use hx_proto::preset::Kind;
        match block.kind {
            Kind::Input => "Input".into(),
            Kind::Output => "Output".into(),
            Kind::Split => "Split".into(),
            Kind::Join => "Join".into(),
            _ => self.model_name(block.model),
        }
    }

    /// Only effects can have their model swapped from the browser.
    fn is_effect(&self, block: &session::Block) -> bool {
        block.kind == hx_proto::preset::Kind::Block
    }

    /// The catalog entry describing a slot's controls.
    ///
    /// Effects, splits and joins carry a model number the symbol table
    /// resolves. Inputs and outputs do not — the device knows what they are
    /// from their position — so they are looked up by symbolic id instead.
    /// They still have real controls: an input has a noise gate, an output has
    /// level and pan.
    fn slot_model(&self, block: &session::Block) -> Option<&hx_catalog::Model> {
        use hx_proto::preset::Kind;
        let catalog = self.catalog.as_ref()?;
        match block.kind {
            Kind::Input => ["HelixStomp_AppDSPFlowInput", "HD2_AppDSPFlow1Input"]
                .into_iter()
                .find_map(|id| catalog.model(id)),
            Kind::Output => ["HelixStomp_AppDSPFlowOutput", "HD2_AppDSPFlowOutput"]
                .into_iter()
                .find_map(|id| catalog.model(id)),
            _ => catalog.model_number(block.model),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        // The device pushes front-panel activity asynchronously, so repaint on
        // a timer rather than only on input.
        ctx.request_repaint_after(Duration::from_millis(150));

        self.dropped_files(ctx);
        self.top_bar(ctx);
        self.preset_list(ctx);
        self.impulse_responses(ctx);
        self.activity(ctx);
        self.signal_chain(ctx);
        self.editor(ctx);
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(48.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(6.0);
                    self.connection_button(ui);
                    ui.add_space(12.0);
                    self.preset_title(ui);
                    ui.add_space(10.0);
                    self.tempo_control(ui);
                    ui.add_space(10.0);
                    self.snapshot_bar(ui);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        self.preset_menu(ui);
                        ui.toggle_value(&mut self.show_activity, "log")
                            .on_hover_text("show what the device is reporting");
                        ui.label(RichText::new(&self.status).color(theme::DIM));
                        if !self.firmware.is_empty() {
                            ui.separator();
                            ui.label(
                                RichText::new(format!("{}  fw {}", self.device, self.firmware))
                                    .color(theme::DIM),
                            );
                        }
                    });
                });
            });
    }

    /// Copy, paste, import and export for whole presets.
    ///
    /// A preset travels as the device's own document, byte for byte, so what
    /// comes back is what was there — including the parts this editor does not
    /// model yet. Pasting a rebuilt-from-the-UI preset would quietly drop them.
    fn preset_menu(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.connection, Connection::Online) && self.preset_index >= 0;

        ui.menu_button("Preset", |ui| {
            ui.set_min_width(190.0);

            if ui.add_enabled(live, egui::Button::new("Copy")).clicked() {
                self.pending_copy = CopyTarget::Clipboard;
                self.send(Cmd::CopyPreset);
                ui.close_menu();
            }

            let paste = match &self.clipboard {
                Some((name, _)) => format!("Paste “{name}”"),
                None => "Paste".to_owned(),
            };
            let can_paste = live && self.clipboard.is_some();
            if ui
                .add_enabled(can_paste, egui::Button::new(paste))
                .clicked()
            {
                if let Some((name, blob)) = self.clipboard.clone() {
                    self.note(format!("pasting {name} over {}", self.preset_name));
                    self.send(Cmd::PastePreset(blob));
                }
                ui.close_menu();
            }

            ui.separator();

            if ui.add_enabled(live, egui::Button::new("Export…")).clicked() {
                let suggested = format!("{}.hxpreset", sanitise(&self.preset_name));
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(suggested)
                    .add_filter("HX preset", &["hxpreset"])
                    .save_file()
                {
                    self.pending_copy = CopyTarget::File(path);
                    self.send(Cmd::CopyPreset);
                }
                ui.close_menu();
            }

            if ui.add_enabled(live, egui::Button::new("Import…")).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("HX preset", &["hxpreset"])
                    .pick_file()
                {
                    self.import(&path);
                }
                ui.close_menu();
            }
        });
    }

    /// Read a preset file and write it over the loaded preset.
    fn import(&mut self, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(blob) => {
                self.note(format!("importing {}", path.display()));
                self.send(Cmd::PastePreset(blob));
            }
            Err(e) => self.note(format!("could not read {}: {e}", path.display())),
        }
    }

    fn connection_button(&mut self, ui: &mut egui::Ui) {
        match self.connection {
            Connection::Online => {
                if ui.button("Disconnect").clicked() {
                    self.send(Cmd::Disconnect);
                }
            }
            Connection::Connecting => {
                ui.spinner();
            }
            Connection::Offline => {
                if ui.button("Connect").clicked() {
                    self.connection = Connection::Connecting;
                    self.status = "Connecting…".into();
                    self.send(Cmd::Connect);
                }
            }
        }
    }

    /// The loaded preset, click-to-rename.
    fn preset_title(&mut self, ui: &mut egui::Ui) {
        if self.preset_index < 0 {
            return;
        }
        ui.label(
            RichText::new(hx_proto::rpc::slot_label(self.preset_index))
                .strong()
                .color(theme::DIM),
        );

        match &mut self.renaming {
            Some(draft) => {
                let edit = ui.add(
                    egui::TextEdit::singleline(draft)
                        .desired_width(170.0)
                        .hint_text("preset name"),
                );
                // Without asking for focus the field never holds it, so it
                // never loses it either and the edit never commits — which is
                // exactly how renaming came to look broken.
                if !edit.has_focus() && !edit.lost_focus() {
                    edit.request_focus();
                }
                if edit.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let name = draft.clone();
                        let index = self.preset_index;
                        self.renaming = None;
                        let _ = self.to_device.send(Cmd::Rename { index, name });
                    } else {
                        self.renaming = None;
                    }
                }
            }
            None => {
                let label = ui.add(
                    egui::Label::new(RichText::new(&self.preset_name).size(16.0).strong())
                        .sense(egui::Sense::click()),
                );
                if label.on_hover_text("click to rename").clicked() {
                    self.renaming = Some(self.preset_name.clone());
                }
            }
        }
    }

    fn tempo_control(&mut self, ui: &mut egui::Ui) {
        let Some(tempo) = self.tempo else { return };
        match &mut self.tempo_draft {
            Some(draft) => {
                let edit = ui.add(egui::TextEdit::singleline(draft).desired_width(52.0));
                if !edit.has_focus() && !edit.lost_focus() {
                    edit.request_focus();
                }
                if edit.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Ok(bpm) = draft.trim().parse::<f32>() {
                            let _ = self.to_device.send(Cmd::SetTempo(bpm));
                        }
                    }
                    self.tempo_draft = None;
                }
            }
            None => {
                let label = ui.add(
                    egui::Label::new(RichText::new(format!("{tempo:.1} BPM")).color(theme::ACCENT))
                        .sense(egui::Sense::click()),
                );
                if label.on_hover_text("click to change tempo").clicked() {
                    self.tempo_draft = Some(format!("{tempo:.1}"));
                }
            }
        }
    }

    /// Snapshots are three saved states of the same preset. The active one is
    /// highlighted, clicking switches, and right-clicking renames — none of
    /// which was discoverable when they were plain buttons.
    fn snapshot_bar(&mut self, ui: &mut egui::Ui) {
        if self.snapshots.is_empty() {
            return;
        }
        let mut pick = None;
        let mut rename = None;

        for (i, name) in self.snapshots.iter().enumerate() {
            match &mut self.snapshot_draft {
                Some((editing, draft)) if *editing == i => {
                    let edit = ui.add(egui::TextEdit::singleline(draft).desired_width(96.0));
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    if edit.lost_focus() {
                        if ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                            rename = Some((i, draft.clone()));
                        }
                        self.snapshot_draft = None;
                    }
                }
                _ => {
                    let active = i == self.current_snapshot;
                    let text = if active {
                        RichText::new(name).color(theme::ACCENT).strong()
                    } else {
                        RichText::new(name).color(theme::DIM)
                    };
                    let button = ui.selectable_label(active, text);
                    if button.clicked() {
                        pick = Some(i);
                    }
                    if button.secondary_clicked() {
                        self.snapshot_draft = Some((i, name.clone()));
                    }
                    button.on_hover_text(
                        "snapshots are saved states of this preset\nclick to switch, right-click to rename",
                    );
                }
            }
        }

        if let Some(index) = pick {
            self.current_snapshot = index;
            self.send(Cmd::SelectSnapshot(index as i64));
        }
        if let Some((index, name)) = rename {
            self.send(Cmd::RenameSnapshot { index, name });
        }
    }

    fn preset_list(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("presets")
            .exact_width(216.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("PRESETS").small().color(theme::DIM));
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Show slot labels alone until the names arrive.
                    let total = if self.presets.is_empty() {
                        self.preset_count as i64
                    } else {
                        self.presets.len() as i64
                    };
                    let mut load = None;
                    for index in 0..total {
                        let name = self
                            .presets
                            .get(index as usize)
                            .cloned()
                            .unwrap_or_default();
                        let selected = index == self.preset_index;
                        let label = format!("{}  {}", hx_proto::rpc::slot_label(index), name);
                        let text = if selected {
                            RichText::new(label).color(theme::ACCENT).strong()
                        } else {
                            RichText::new(label)
                        };
                        if ui.selectable_label(selected, text).clicked() {
                            load = Some(index);
                        }
                    }
                    if let Some(index) = load {
                        self.send(Cmd::SelectPreset(index));
                    }
                });
            });
    }

    /// The impulse response slots, mirroring HX Edit's IRs tab.
    fn impulse_responses(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("irs")
            .exact_width(210.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("IMPULSE RESPONSES").small().color(theme::DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("refresh").clicked() {
                            self.send(Cmd::ListIrs);
                        }
                    });
                });
                ui.separator();

                if self.connection != Connection::Online {
                    ui.label(RichText::new("Connect to manage IRs").color(theme::DIM));
                    return;
                }

                let mut clear = None;
                egui::ScrollArea::vertical().id_salt("irs").show(ui, |ui| {
                    for (slot, name) in &self.irs {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{:>3}", slot + 1))
                                    .monospace()
                                    .color(theme::DIM),
                            );
                            ui.label(if name.is_empty() {
                                "—"
                            } else {
                                name.as_str()
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if !name.is_empty() && ui.small_button("clear").clicked() {
                                        clear = Some(*slot);
                                    }
                                },
                            );
                        });
                    }
                    if self.irs.is_empty() {
                        ui.label(RichText::new("no impulse responses loaded").color(theme::DIM));
                    }
                });
                if let Some(slot) = clear {
                    self.send(Cmd::ClearIr(slot));
                }

                ui.add_space(6.0);
                ui.separator();
                // Uploading takes a few seconds because the device writes to flash
                // and we wait for it to confirm; saying so avoids it looking hung.
                ui.label(
                    RichText::new(
                        "Drop a mono WAV on the window to load it into the first free slot. \
                               Uploads take a few seconds.",
                    )
                    .small()
                    .color(theme::DIM),
                );
            });
    }

    /// Accept a WAV dropped anywhere on the window.
    fn dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for path in dropped {
            // A preset and an impulse response are both "a file you drop on the
            // window", so the extension decides which one you meant.
            if path.extension().is_some_and(|e| e == "hxpreset") {
                self.import(&path);
                continue;
            }
            let free =
                (0..128).find(|s| !self.irs.iter().any(|(slot, n)| slot == s && !n.is_empty()));
            match free {
                Some(slot) => {
                    self.note(format!(
                        "loading {} into IR slot {}",
                        path.display(),
                        slot + 1
                    ));
                    self.send(Cmd::LoadIr { slot, file: path });
                }
                None => self.note("no free impulse response slot".into()),
            }
        }
    }

    fn activity(&mut self, ctx: &egui::Context) {
        if !self.show_activity {
            return;
        }
        egui::TopBottomPanel::bottom("activity")
            .exact_height(100.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("DEVICE ACTIVITY").small().color(theme::DIM));
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.label(RichText::new(line).small().monospace().color(theme::DIM));
                        }
                    });
            });
    }

    /// The signal path, drawn the way it is wired.
    ///
    /// The slot array is a fixed topology rather than a running order: the
    /// split sits after the output in it even though the signal reaches it
    /// first. Read as a list it puts the split and join on the end of the
    /// chain, which is where they used to be drawn.
    ///
    /// They are not blocks in the line either. A split is where the wiring
    /// divides and a join is where it comes back, so they are drawn as the
    /// lines branching and merging around the lanes — still clickable, since a
    /// split has a mode and a join has levels, but not sitting in the signal
    /// path pretending to be pedals.
    ///
    /// The number of lanes is not fixed at two: Helix and Helix LT carry two
    /// independent signal paths, so a preset that splits both has four.
    fn signal_chain(&mut self, ctx: &egui::Context) {
        let rows: usize = self.layout.paths.iter().map(|p| p.lanes.len().max(1)).sum();
        let height = 34.0 + rows.max(1) as f32 * theme::LANE_HEIGHT;

        egui::TopBottomPanel::top("chain")
            .exact_height(height.min(ctx.screen_rect().height() * 0.55))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                if self.chain.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("No preset loaded").color(theme::DIM));
                    });
                    return;
                }

                let mut pick = None;
                egui::ScrollArea::both().show(ui, |ui| {
                    let paths = self.layout.paths.clone();
                    ui.vertical(|ui| {
                        for (n, path) in paths.iter().enumerate() {
                            if paths.len() > 1 {
                                ui.label(
                                    RichText::new(format!("Path {}", n + 1))
                                        .small()
                                        .color(theme::DIM),
                                );
                            }
                            pick = self.path_row(ui, path).or(pick);
                        }
                    });
                });

                if let Some(i) = pick {
                    // Purely a local view change. Mirroring the selection onto
                    // the device's own screen meant every click was a round
                    // trip, and clicking through a chain quickly wedged it.
                    self.selected = i;
                    self.browsing = None;
                }
            });
    }

    /// One signal path: input, whatever the signal passes through, output.
    ///
    /// A split divides a *stretch* of the path, not all of it — the split
    /// records the slot it attaches before, and the blocks on either side of
    /// that stretch carry the undivided signal. Drawing every block as though
    /// it were on a branch was wrong, and obviously so next to HX Edit.
    fn path_row(&self, ui: &mut egui::Ui, path: &hx_proto::preset::Path) -> Option<usize> {
        let mut pick = None;
        let lanes = path.lanes.len();
        let tall = if lanes > 1 {
            theme::LANE_HEIGHT * lanes as f32
        } else {
            theme::BLOCK_HEIGHT
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(6.0);

            if let Some(input) = path.input {
                pick = self.endpoint(ui, input, tall).or(pick);
            }
            // Everything ahead of the split, on the centre line.
            for slot in &path.head {
                theme::wire_run(ui, theme::WIRE_WIDTH, tall);
                pick = self.block_at(ui, *slot, tall).or(pick);
            }

            if lanes > 1 {
                let longest = path.lanes.iter().map(|l| l.blocks.len()).max().unwrap_or(0);
                if let Some(split) = path.split {
                    pick = self.junction(ui, split, lanes, true).or(pick);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for lane in &path.lanes {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            pick = self.lane_row(ui, &lane.blocks, longest).or(pick);
                        });
                        ui.add_space(theme::LANE_HEIGHT - theme::BLOCK_HEIGHT);
                    }
                });
                if let Some(join) = path.join {
                    pick = self.junction(ui, join, lanes, false).or(pick);
                }
            }

            // And everything the recombined signal passes through.
            for slot in &path.tail {
                pick = self.block_at(ui, *slot, tall).or(pick);
                theme::wire_run(ui, theme::WIRE_WIDTH, tall);
            }
            if path.tail.is_empty() && lanes > 1 {
                theme::wire_run(ui, theme::WIRE_WIDTH, tall);
            }
            if let Some(output) = path.output {
                pick = self.endpoint(ui, output, tall).or(pick);
            }
        });
        pick
    }

    /// One block on the centre line, vertically centred against `tall`.
    fn block_at(&self, ui: &mut egui::Ui, slot: usize, tall: f32) -> Option<usize> {
        let i = self.index_of(slot)?;
        let block = &self.chain[i];
        let art = self.artwork(block);
        let mut pick = None;
        ui.allocate_ui(egui::vec2(theme::BLOCK_WIDTH, tall), |ui| {
            ui.vertical(|ui| {
                ui.add_space((tall - theme::BLOCK_HEIGHT) / 2.0);
                if theme::block_button(
                    ui,
                    &self.slot_label(block),
                    art.as_ref(),
                    i == self.selected,
                    block.enabled,
                )
                .clicked()
                {
                    pick = Some(i);
                }
            });
        });
        pick
    }

    /// One lane's blocks, padded out to `longest` so lanes stay in step.
    fn lane_row(&self, ui: &mut egui::Ui, blocks: &[usize], longest: usize) -> Option<usize> {
        let mut pick = None;
        for (n, slot) in blocks.iter().enumerate() {
            let Some(i) = self.index_of(*slot) else {
                continue;
            };
            let block = &self.chain[i];
            let art = self.artwork(block);
            if theme::block_button(
                ui,
                &self.slot_label(block),
                art.as_ref(),
                i == self.selected,
                block.enabled,
            )
            .clicked()
            {
                pick = Some(i);
            }
            if n + 1 < blocks.len() {
                theme::wire_run(ui, theme::WIRE_WIDTH, theme::BLOCK_HEIGHT);
            }
        }
        // A short lane runs on as plain wire to where the merge happens.
        let short = longest.saturating_sub(blocks.len()) as f32;
        if short > 0.0 {
            theme::wire_run(ui, short * theme::COLUMN, theme::BLOCK_HEIGHT);
        }
        pick
    }

    /// An input or output, standing alongside the lanes rather than in one.
    fn endpoint(&self, ui: &mut egui::Ui, slot: usize, tall: f32) -> Option<usize> {
        let i = self.index_of(slot)?;
        let block = &self.chain[i];
        let art = self.artwork(block);
        let mut pick = None;
        ui.allocate_ui(egui::vec2(theme::BLOCK_WIDTH, tall), |ui| {
            // Centred by hand. `centered_and_justified` stretches the widget to
            // fill instead, which made the endpoints twice the height of every
            // other tile.
            ui.vertical(|ui| {
                ui.add_space((tall - theme::BLOCK_HEIGHT) / 2.0);
                if theme::block_button(
                    ui,
                    &self.slot_label(block),
                    art.as_ref(),
                    i == self.selected,
                    block.enabled,
                )
                .clicked()
                {
                    pick = Some(i);
                }
            });
        });
        pick
    }

    /// The branch or merge itself, drawn as wiring and clickable for its own
    /// parameters — a split's mode, a join's levels and pans.
    fn junction(
        &self,
        ui: &mut egui::Ui,
        slot: usize,
        lanes: usize,
        opening: bool,
    ) -> Option<usize> {
        let i = self.index_of(slot)?;
        let selected = i == self.selected;
        let hit = theme::junction(ui, lanes, opening, selected)
            .on_hover_text(self.slot_label(&self.chain[i]));
        hit.clicked().then_some(i)
    }

    fn index_of(&self, slot: usize) -> Option<usize> {
        self.chain.iter().position(|b| b.position == slot as i64)
    }

    fn editor(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(block) = self.chain.get(self.selected).cloned() else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Connect a device to begin").color(theme::DIM));
                });
                return;
            };

            ui.horizontal(|ui| {
                ui.heading(self.slot_label(&block));
                if !self.is_effect(&block) {
                    return;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Clear").clicked() {
                        self.send(Cmd::ClearBlock(block.position));
                    }
                    let last = self.chain.len().saturating_sub(1);
                    if ui
                        .add_enabled(self.selected < last, egui::Button::new("»"))
                        .on_hover_text("move later in the chain")
                        .clicked()
                    {
                        self.send(Cmd::MoveBlock {
                            from: block.position as usize,
                            to: self.chain[self.selected + 1].position as usize,
                        });
                    }
                    if ui
                        .add_enabled(self.selected > 0, egui::Button::new("«"))
                        .on_hover_text("move earlier in the chain")
                        .clicked()
                    {
                        self.send(Cmd::MoveBlock {
                            from: block.position as usize,
                            to: self.chain[self.selected - 1].position as usize,
                        });
                    }
                    let mut on = block.enabled;
                    if ui.checkbox(&mut on, "Enabled").changed() {
                        self.send(Cmd::SetEnabled {
                            block: block.position,
                            enabled: on,
                        });
                        self.chain[self.selected].enabled = on;
                    }
                });
            });
            ui.separator();

            // Inputs, outputs and splits carry no model and no knobs.
            if !self.is_effect(&block) {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("part of the signal path, nothing to edit").color(theme::DIM),
                    );
                });
                return;
            }

            // HX Edit puts this on a separate Bypass/Controller Assign tab; at
            // one control it does not warrant one.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Bypass follows").small().color(theme::DIM));
                ui.add(
                    egui::DragValue::new(&mut self.assign_cc)
                        .range(0..=127)
                        .prefix("CC"),
                );
                if ui.small_button("assign").clicked() {
                    let cc = self.assign_cc;
                    self.send(Cmd::AssignCc {
                        block: block.position,
                        cc,
                    });
                }
            });
            ui.add_space(4.0);
            ui.separator();

            let model = self.slot_model(&block).cloned();
            let Some(model) = model else {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("no controls for this slot").color(theme::DIM));
                });
                return;
            };

            // Only an effect can have its model swapped, so only an effect gets
            // the browser; everything else uses the full width for its knobs.
            if self.is_effect(&block) {
                ui.columns(2, |columns| {
                    let width = columns[0].available_width();
                    columns[0].set_max_width(width);
                    self.model_browser(&mut columns[0], &block);

                    let ui = &mut columns[1];
                    let width = ui.available_width();
                    ui.set_max_width(width);
                    self.pedal(ui, &model, &block.values, block.position, false);

                    if let Some(cab) = block.paired {
                        if let Some(cab) = self
                            .catalog
                            .as_ref()
                            .and_then(|c| c.model_number(cab))
                            .cloned()
                        {
                            ui.add_space(10.0);
                            ui.separator();
                            let values = block.paired_values.clone();
                            self.pedal(ui, &cab, &values, block.position, true);
                        }
                    }
                });
            } else {
                self.pedal(ui, &model, &block.values, block.position, false);
            }
        });
    }

    /// The model shelf: categories down the side, thumbnails in a grid.
    ///
    /// Modelled on Logic's Pedalboard rather than HX Edit's list, because with
    /// several hundred models the picture is what you recognise, not the name.
    fn model_browser(&mut self, ui: &mut egui::Ui, block: &session::Block) {
        let Some(catalog) = self.catalog.as_ref() else {
            ui.label(RichText::new("Install HX Edit for model names").color(theme::DIM));
            return;
        };

        // Default to the category the current block belongs to, which is what
        // HX Edit shows when you select a block.
        let showing = self.browsing.or_else(|| {
            catalog
                .model_number(block.model)
                .and_then(|m| catalog.category_of(&m.id))
        });
        let current_id = catalog.model_number(block.model).map(|m| m.id.clone());

        let mut chosen = None;
        let mut pick = None;

        ui.horizontal(|ui| {
            ui.label(RichText::new("MODEL").small().color(theme::DIM));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search")
                    .desired_width(150.0),
            );
            if !self.search.is_empty() && ui.small_button("✕").clicked() {
                self.search.clear();
            }
        });

        // Searching looks across every category at once — otherwise you have to
        // know where a model lives before you can find it.
        let needle = self.search.to_lowercase();
        let models: Vec<&hx_catalog::Model> = if needle.is_empty() {
            showing.map(|c| catalog.models_in(c)).unwrap_or_default()
        } else {
            catalog
                .models()
                .filter(|m| m.name.to_lowercase().contains(&needle))
                .collect()
        };

        egui::ScrollArea::vertical()
            .id_salt("browser")
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(116.0);
                        for category in catalog.categories() {
                            if catalog.models_in(category.id).is_empty() {
                                continue;
                            }
                            let on = needle.is_empty() && Some(category.id) == showing;
                            if ui.selectable_label(on, &category.name).clicked() {
                                chosen = Some(category.id);
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        if models.is_empty() {
                            ui.label(RichText::new("Nothing matches").color(theme::DIM));
                        }
                        ui.horizontal_wrapped(|ui| {
                            for model in models {
                                let selected = current_id.as_deref() == Some(model.id.as_str());
                                let art = catalog
                                    .artwork(model)
                                    .map(|p| format!("file://{}", p.display()));
                                if theme::model_tile(ui, &model.name, art.as_deref(), selected)
                                    .clicked()
                                {
                                    // The wire wants a model *number*, so only
                                    // models in the symbol table can be chosen.
                                    pick = catalog
                                        .symbols()
                                        .iter()
                                        .find(|s| s.model.as_deref() == Some(model.id.as_str()))
                                        .map(|s| s.number);
                                }
                            }
                        });
                    });
                });
            });

        if let Some(id) = chosen {
            self.browsing = Some(id);
            self.search.clear();
        }
        if let Some(model) = pick {
            self.send(Cmd::SetModel {
                block: block.position,
                model,
            });
        }
    }

    /// Where an Input or Main L/R block is routed.
    ///
    /// Editable via opcode 42, which was captured from HX Edit's own routing
    /// clicks — a document write is accepted but ignored for this field, which
    /// is why this control was read-only for a while.
    fn routing_menu(&self, ui: &mut egui::Ui, model: &hx_catalog::Model, position: i64) {
        let Some(current) = self
            .chain
            .iter()
            .find(|b| b.position == position)
            .and_then(|b| b.routing)
        else {
            return;
        };
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        // The routing setting is the one the device keeps outside the value
        // array, so it is found by name rather than by index.
        let Some(param) = model
            .params
            .iter()
            .find(|p| p.id == "@input" || p.id == "@output")
        else {
            return;
        };
        let Some(choices) = catalog.choices(param) else {
            return;
        };

        let mut chosen = None;
        let showing = choices
            .get(current.max(0) as usize)
            .cloned()
            .unwrap_or_else(|| current.to_string());

        ui.horizontal(|ui| {
            ui.label(RichText::new(&param.name).small().color(theme::DIM));
            egui::ComboBox::from_id_salt(("routing", position))
                .selected_text(RichText::new(showing).color(theme::ACCENT))
                .width(230.0)
                .show_ui(ui, |ui| {
                    for (index, label) in choices.iter().enumerate() {
                        if ui
                            .selectable_label(index as i64 == current, label)
                            .clicked()
                        {
                            chosen = Some(index as i64);
                        }
                    }
                });
        });
        ui.add_space(4.0);

        if let Some(to) = chosen {
            if to != current {
                self.send(Cmd::SetRouting {
                    block: position,
                    to,
                });
            }
        }
    }

    /// Draw one model's controls. Used for both halves of an Amp+Cab block, so
    /// the model number is passed in rather than read off the block.
    /// The selected block drawn as a pedal: its artwork, then its controls as
    /// knobs beneath, the way Logic's Pedalboard and the hardware itself do.
    fn pedal(
        &mut self,
        ui: &mut egui::Ui,
        model: &hx_catalog::Model,
        values: &[f32],
        position: i64,
        paired: bool,
    ) {
        let Some(catalog) = self.catalog.as_ref() else {
            for (i, value) in values.iter().enumerate() {
                ui.label(format!("{i}: {value}"));
            }
            return;
        };

        let mut edit = None;
        ui.vertical_centered(|ui| {
            if let Some(path) = catalog.artwork(model) {
                ui.add(
                    egui::Image::new(format!("file://{}", path.display()))
                        .max_height(96.0)
                        .maintain_aspect_ratio(true),
                );
            }
            ui.label(RichText::new(&model.name).strong());
        });
        ui.add_space(6.0);
        self.routing_menu(ui, model, position);

        // Values arrive in the order the device indexes them, which the catalog
        // knows how to reproduce — it is not simply the model's parameter list.
        // An input's list starts with `@input`, which carries no value, and
        // using it directly shifted every knob by one.
        let params = catalog.ordered_params(model);

        // Knobs wrap onto as many rows as the width allows, like a pedal's face.
        ui.horizontal_wrapped(|ui| {
            for (index, value) in values.iter().enumerate() {
                let Some(param) = params.get(index).copied() else {
                    continue;
                };
                let mut current = *value;

                ui.allocate_ui(egui::vec2(76.0, 92.0), |ui| {
                    ui.vertical_centered(|ui| {
                        let changed = match param.kind {
                            Kind::Switch => {
                                let mut on = current >= 0.5;
                                let hit = ui.add(theme::switch(&mut on)).changed();
                                current = on as u8 as f32;
                                hit
                            }
                            _ => theme::knob(ui, &mut current, param.min..=param.max).changed(),
                        };
                        ui.label(
                            RichText::new(catalog.format(param, current))
                                .monospace()
                                .color(theme::ACCENT),
                        );
                        ui.label(RichText::new(&param.name).small().color(theme::DIM));
                        if changed {
                            edit = Some((index as i64, current, param.kind == Kind::Switch));
                        }
                    });
                });
            }
        });

        if let Some((index, value, switch)) = edit {
            let slot = &mut self.chain[self.selected];
            let target = if paired {
                &mut slot.paired_values
            } else {
                &mut slot.values
            };
            target[index as usize] = value;
            // The cab's parameters are addressed on the same block; only which
            // half they belong to differs, and the device infers that from the
            // index range.
            self.send(Cmd::SetParam {
                block: position,
                index,
                value,
                switch,
            });
        }
    }
}

/// Make a preset name safe to use as a filename.
fn sanitise(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim_matches('_').is_empty() {
        "preset".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// An app with channels that go nowhere, for testing state handling.
    fn app() -> (App, mpsc::Sender<Evt>, mpsc::Receiver<Cmd>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        (
            App::new(&egui::Context::default(), cmd_tx, evt_rx),
            evt_tx,
            cmd_rx,
        )
    }

    /// The app reaches for the device on startup, so it begins in Connecting
    /// rather than waiting to be told.
    #[test]
    fn connecting_populates_the_device_and_preset_count() {
        let (mut app, events, _cmds) = app();
        assert_eq!(app.connection, Connection::Connecting);

        events
            .send(Evt::Connected {
                device: "HX Stomp".into(),
                presets: 126,
            })
            .unwrap();
        app.drain_events();

        assert_eq!(app.connection, Connection::Online);
        assert_eq!(app.device, "HX Stomp");
        assert_eq!(app.preset_count, 126);
    }

    /// A dropped session must not leave the last preset on screen, or the UI
    /// shows a chain the device is no longer holding.
    #[test]
    fn disconnecting_clears_what_was_on_screen() {
        let (mut app, events, _cmds) = app();
        events
            .send(Evt::Connected {
                device: "HX Stomp".into(),
                presets: 126,
            })
            .unwrap();
        events
            .send(Evt::Presets(vec!["One".into(), "Two".into()]))
            .unwrap();
        events
            .send(Evt::Loaded {
                index: 7,
                name: "CT-Sad".into(),
                firmware: "3.80".into(),
                tempo: Some(120.0),
                snapshots: vec!["SNAPSHOT 1".into()],
                layout: hx_proto::preset::Layout::default(),
                chain: vec![session::Block {
                    position: 1,
                    routing: None,
                    kind: hx_proto::preset::Kind::Block,
                    model: 101,
                    enabled: true,
                    values: vec![0.5],
                    paired: None,
                    paired_values: vec![],
                }],
            })
            .unwrap();
        app.drain_events();
        assert_eq!(app.chain.len(), 1);
        assert_eq!(app.preset_name, "CT-Sad");

        events.send(Evt::Disconnected).unwrap();
        app.drain_events();

        assert_eq!(app.connection, Connection::Offline);
        assert!(app.chain.is_empty());
        assert!(app.presets.is_empty());
    }

    #[test]
    fn a_failure_while_connecting_returns_to_offline() {
        let (mut app, events, _cmds) = app();
        app.connection = Connection::Connecting;
        events.send(Evt::Failed("no device".into())).unwrap();
        app.drain_events();

        assert_eq!(app.connection, Connection::Offline);
        assert_eq!(app.status, "no device");
    }

    /// The log is unbounded input from the device, so it must not grow forever.
    #[test]
    fn the_activity_log_is_bounded() {
        let (mut app, events, _cmds) = app();
        for i in 0..400 {
            events.send(Evt::Activity(format!("event {i}"))).unwrap();
        }
        app.drain_events();

        assert!(app.log.len() <= 300, "log grew to {}", app.log.len());
        assert_eq!(app.log.last().unwrap(), "event 399");
    }

    #[test]
    fn an_unknown_model_still_gets_a_label() {
        let (app, _events, _cmds) = app();
        assert_eq!(app.model_name(u32::MAX), format!("model {}", u32::MAX));
    }

    /// Copying keeps the device's own bytes, not a rebuild of what is on
    /// screen: a preset carries more than this editor models, and pasting a
    /// reconstruction would silently drop the rest.
    #[test]
    fn copying_a_preset_keeps_the_bytes_verbatim() {
        let (mut app, events, _cmds) = app();
        let blob = vec![0xde, 0xad, 0xbe, 0xef];

        events
            .send(Evt::Copied {
                name: "Crunch".into(),
                blob: blob.clone(),
            })
            .unwrap();
        app.drain_events();

        assert_eq!(app.clipboard, Some(("Crunch".into(), blob)));
    }

    /// An export writes the file itself rather than putting it on the
    /// clipboard, and the two use the same round trip to the device.
    #[test]
    fn exporting_writes_the_preset_to_the_chosen_file() {
        let (mut app, events, _cmds) = app();
        let file = std::env::temp_dir().join("stompchain-export-test.hxpreset");
        let _ = std::fs::remove_file(&file);

        app.pending_copy = CopyTarget::File(file.clone());
        events
            .send(Evt::Copied {
                name: "Clean".into(),
                blob: b"l6-helix".to_vec(),
            })
            .unwrap();
        app.drain_events();

        assert_eq!(std::fs::read(&file).unwrap(), b"l6-helix");
        assert!(
            app.clipboard.is_none(),
            "an export should not also occupy the clipboard"
        );
        let _ = std::fs::remove_file(&file);
    }

    /// Importing a file that is not a preset must not reach the device: it
    /// would be accepted and then read back as an empty slot.
    #[test]
    fn importing_a_missing_file_reports_instead_of_sending() {
        let (mut app, _events, cmds) = app();
        app.import(std::path::Path::new("/nonexistent/nope.hxpreset"));

        // The app connects on startup, so the queue is not empty — but nothing
        // that would write to the device may be in it.
        assert!(
            !cmds.try_iter().any(|c| matches!(c, Cmd::PastePreset(_))),
            "a file that could not be read must not reach the device"
        );
        assert!(app.log.iter().any(|l| l.contains("could not read")));
    }

    #[test]
    fn a_preset_name_becomes_a_usable_filename() {
        assert_eq!(sanitise("Brit / Clean"), "Brit___Clean");
        assert_eq!(sanitise("  "), "preset");
        assert_eq!(sanitise("03A"), "03A");
    }
}
