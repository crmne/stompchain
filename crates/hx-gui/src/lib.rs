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
    /// A copied block: which slot it came from. The block itself stays on the
    /// device — a copy is a document operation there, so the app only has to
    /// remember what to copy from.
    copied_block: Option<usize>,
    /// A copied preset: its name, and the document verbatim. Held in the app
    /// rather than the system clipboard because it is binary, and because
    /// pasting it into a text field would only produce noise.
    clipboard: Option<(String, Vec<u8>)>,
    /// Where the bytes should go once `Cmd::CopyPreset` answers.
    pending_copy: CopyTarget,
    /// Whether the device window is open.
    show_device: bool,
    /// The device's global EQ switch, as last read.
    global_eq: bool,
    /// How many steps the worker can undo and redo, for enabling the buttons.
    undo_depth: usize,
    redo_depth: usize,
    /// Where a click on a `+` in the chain wants to add a block.
    inserting_at: Option<usize>,
    /// Whether the edit buffer has changes the preset does not.
    ///
    /// The device edits a scratch copy: a changed parameter is audible at once
    /// but vanishes on reload unless it is saved. An editor that does not say
    /// so loses people's work quietly, so this drives a dot in the title.
    dirty: bool,
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
    /// Which MIDI CC the "assign bypass" control offers.
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
            copied_block: None,
            clipboard: None,
            pending_copy: CopyTarget::Clipboard,
            dirty: false,
            show_device: false,
            global_eq: false,
            undo_depth: 0,
            redo_depth: 0,
            inserting_at: None,
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

    /// Send something that changes the edit buffer, and remember that it did.
    fn edit(&mut self, cmd: Cmd) {
        self.dirty = true;
        self.send(cmd);
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
                    self.dirty = false;
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
                Ok(Evt::Saved) => self.dirty = false,
                Ok(Evt::History { undo, redo }) => {
                    self.undo_depth = undo;
                    self.redo_depth = redo;
                }
                Ok(Evt::Settings { global_eq }) => self.global_eq = global_eq,
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

    /// A model's display name.
    ///
    /// Model 0 is treated as "no model": the endpoints report it because they
    /// carry no model reference, and the symbol table's entry 0 is a real amp,
    /// so resolving it names the wrong thing entirely.
    fn model_name(&self, model: u32) -> String {
        if model == 0 {
            return String::new();
        }
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
            Kind::Output => ["HelixStomp_AppDSPFlowOutputMain", "HD2_AppDSPFlowOutput"]
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
        self.status_bar(ctx);
        self.preset_list(ctx);
        self.activity(ctx);
        self.signal_chain(ctx);
        // The shelf sits beside the pedal being edited rather than under it,
        // so choosing a different model is plainly a secondary action.
        self.shelf(ctx);
        self.editor(ctx);
        self.device_window(ctx);
    }
}

impl App {
    /// One row: the preset you are editing, and what you can do to it.
    ///
    /// This had grown to two rows holding two menus, a connection state and a
    /// log toggle — an inventory of the program rather than of the music. The
    /// preset actions moved to the preset list they act on, the device moved
    /// to a status bar at the bottom, and what is left is the preset itself.
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(46.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    self.preset_title(ui);
                    ui.add_space(12.0);
                    self.tempo_control(ui);
                    ui.add_space(12.0);
                    self.snapshot_bar(ui);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        self.save_button(ui);
                        self.history_buttons(ui);
                    });
                });
            });
    }

    /// The device, along the bottom, where a status bar belongs.
    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(28.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    let colour = match self.connection {
                        Connection::Online => egui::Color32::from_rgb(0x4c, 0xc0, 0x60),
                        _ => theme::DIM,
                    };
                    theme::status_dot(ui, colour);

                    // The device's name is the way in to its settings: that is
                    // where you would look for them.
                    let name = if self.device.is_empty() {
                        "No device".to_owned()
                    } else {
                        self.device.clone()
                    };
                    if ui
                        .add_enabled(
                            matches!(self.connection, Connection::Online),
                            egui::Button::new(RichText::new(name).strong()).frame(false),
                        )
                        .on_hover_text("impulse responses and device settings")
                        .clicked()
                    {
                        self.show_device = !self.show_device;
                    }
                    if !self.firmware.is_empty() {
                        ui.label(
                            RichText::new(format!("firmware {}", self.firmware))
                                .small()
                                .color(theme::DIM),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        match self.connection {
                            Connection::Online => {
                                if ui.small_button("Disconnect").clicked() {
                                    self.send(Cmd::Disconnect);
                                }
                            }
                            Connection::Connecting => {
                                ui.spinner();
                            }
                            Connection::Offline => {
                                if ui.small_button("Connect").clicked() {
                                    self.connection = Connection::Connecting;
                                    self.send(Cmd::Connect);
                                }
                            }
                        }
                        ui.label(RichText::new(&self.status).small().color(theme::DIM));
                    });
                });
            });
    }

    /// Undo and redo, where you can see them.
    fn history_buttons(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.connection, Connection::Online);
        if ui
            .add_enabled(live && self.redo_depth > 0, egui::Button::new("Redo"))
            .on_hover_text("put back what undo took away")
            .clicked()
        {
            self.send(Cmd::Redo);
        }
        if ui
            .add_enabled(live && self.undo_depth > 0, egui::Button::new("Undo"))
            .on_hover_text("step back through changes to the chain")
            .clicked()
        {
            self.send(Cmd::Undo);
        }
    }

    /// Commit the edit buffer, and say when there is something to commit.
    ///
    /// The device edits a scratch copy of the preset: changes are audible
    /// immediately but are lost on reload unless they are saved. HX Edit has
    /// this on File > Save Preset; putting it beside the name makes the state
    /// visible rather than something you have to remember.
    fn save_button(&mut self, ui: &mut egui::Ui) {
        if self.preset_index < 0 {
            return;
        }
        if self.dirty {
            theme::status_dot(ui, theme::ACCENT).on_hover_text("unsaved changes");
        }
        let hit = ui
            .add_enabled(self.dirty, egui::Button::new("Save"))
            .on_hover_text("write these changes into the preset")
            .on_disabled_hover_text("no changes to save");
        if hit.clicked() {
            self.send(Cmd::SavePreset);
        }
    }

    /// Copy, paste, import and export, on the preset list itself.
    ///
    /// A preset travels as the device's own document, byte for byte, so what
    /// comes back is what was there — including the parts this editor does not
    /// model. Rebuilding one from what the UI shows would quietly drop them.
    fn preset_actions(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.connection, Connection::Online) && self.preset_index >= 0;
        let small = |ui: &mut egui::Ui, label: &str, on: bool| {
            ui.add_enabled(
                on,
                egui::Button::new(RichText::new(label).small()).frame(false),
            )
        };

        if small(ui, "EXPORT", live)
            .on_hover_text("save this preset to a file")
            .clicked()
        {
            let suggested = format!("{}.hxpreset", sanitise(&self.preset_name));
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(suggested)
                .add_filter("HX preset", &["hxpreset"])
                .save_file()
            {
                self.pending_copy = CopyTarget::File(path);
                self.send(Cmd::CopyPreset);
            }
        }
        if small(ui, "IMPORT", live)
            .on_hover_text("load a preset file over this one")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("HX preset", &["hxpreset"])
                .pick_file()
            {
                self.import(&path);
            }
        }
        if small(ui, "PASTE", live && self.clipboard.is_some())
            .on_hover_text(match &self.clipboard {
                Some((name, _)) => format!("paste “{name}” over this preset"),
                None => "nothing copied yet".to_owned(),
            })
            .clicked()
        {
            if let Some((name, blob)) = self.clipboard.clone() {
                self.note(format!("pasting {name} over {}", self.preset_name));
                self.send(Cmd::PastePreset(blob));
            }
        }
        if small(ui, "COPY", live)
            .on_hover_text("copy this preset")
            .clicked()
        {
            self.pending_copy = CopyTarget::Clipboard;
            self.send(Cmd::CopyPreset);
        }
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

    /// The loaded preset, click-to-rename.    /// The loaded preset, click-to-rename.
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
                // The actions sit on the list they act on, the way HX Edit
                // puts COPY / PASTE / IMPORT / EXPORT on its preset header. A
                // menu called "Preset" at the top of the window made you go
                // looking somewhere else for something that belongs here.
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PRESETS").small().color(theme::DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.preset_actions(ui);
                    });
                });
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
    /// Everything about the *device* rather than the preset.
    ///
    /// Impulse responses used to sit in a permanent side panel next to a
    /// browser category also called IR, which invited exactly the question of
    /// what the difference was. It is this: the **IR category** puts an IR
    /// *block* in your signal chain, and that block plays whichever of the
    /// device's IR slots you point it at. This window is those slots — the
    /// device's library, shared by every preset. The list refreshes itself
    /// whenever it changes, so there is nothing to press.
    fn device_window(&mut self, ctx: &egui::Context) {
        if !self.show_device {
            return;
        }
        let mut open = true;
        egui::Window::new("Device")
            .open(&mut open)
            .default_width(460.0)
            .default_height(480.0)
            .collapsible(false)
            .show(ctx, |ui| {
                // Explanatory text has to be told how wide it may be, or egui
                // lays it out on one line and the window grows to fit it.
                ui.set_max_width(440.0);
                ui.label(
                    RichText::new(format!("{}  ·  firmware {}", self.device, self.firmware))
                        .color(theme::DIM),
                );
                ui.separator();

                ui.heading("Impulse responses");
                ui.label(
                    RichText::new(
                        "The device's IR library, shared by every preset. Add an IR block                          to a chain from the shelf, then point it at one of these slots.",
                    )
                    .small()
                    .color(theme::DIM),
                );
                ui.add_space(4.0);
                if self.irs.is_empty() {
                    ui.label(RichText::new("no impulse responses loaded").color(theme::DIM));
                }
                let irs = self.irs.clone();
                egui::ScrollArea::vertical()
                    .max_height(170.0)
                    .id_salt("irs")
                    .show(ui, |ui| {
                        for (slot, name) in &irs {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{:>3}", slot + 1)).monospace());
                                ui.label(name);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("clear").clicked() {
                                            self.send(Cmd::ClearIr(*slot));
                                        }
                                    },
                                );
                            });
                        }
                    });
                ui.label(
                    RichText::new("Drop a mono WAV on the window to load one.")
                        .small()
                        .color(theme::DIM),
                );

                ui.add_space(10.0);
                ui.separator();
                ui.heading("Preferences");
                ui.label(
                    RichText::new(
                        "The device's own global settings — the same ones HX Edit's                          preferences write. They belong to the device, not the preset.",
                    )
                    .small()
                    .color(theme::DIM),
                );
                ui.add_space(4.0);
                self.preferences(ui);

                ui.add_space(10.0);
                ui.separator();
                ui.checkbox(&mut self.show_activity, "Show what the device reports");
            });
        self.show_device = open;
    }

    /// The handful of global settings worth exposing by name.
    ///
    /// The namespace is 147 numbered objects with no names anywhere in HX
    /// Edit's data, so naming them is a matter of having identified each one.
    /// These are the ones that have been: guessing at the rest would be worse
    /// than leaving them out.
    fn preferences(&mut self, ui: &mut egui::Ui) {
        const GLOBAL_EQ_ENABLED: i64 = 203;

        if !matches!(self.connection, Connection::Online) {
            ui.label(RichText::new("connect to read the device's settings").color(theme::DIM));
            return;
        }

        ui.horizontal(|ui| {
            let mut on = self.global_eq;
            if ui.checkbox(&mut on, "Global EQ").changed() {
                self.global_eq = on;
                self.send(Cmd::SetSetting {
                    id: GLOBAL_EQ_ENABLED,
                    on,
                });
            }
            ui.label(
                RichText::new("applies to every preset, after the output")
                    .small()
                    .color(theme::DIM),
            );
        });
    }

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
    fn path_row(&mut self, ui: &mut egui::Ui, path: &hx_proto::preset::Path) -> Option<usize> {
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
            // Everything ahead of the split, on the centre line. A gap before
            // each block, so a chain can be built from either end.
            for slot in &path.head {
                self.insert_point_tall(ui, *slot, tall);
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
                self.insert_point_tall(ui, *slot, tall);
                pick = self.block_at(ui, *slot, tall).or(pick);
            }
            if let Some(output) = path.output {
                self.insert_point_tall(ui, output, tall);
            }
            if let Some(output) = path.output {
                pick = self.endpoint(ui, output, tall).or(pick);
            }
        });
        pick
    }

    /// A gap you can add a block to, at `before`.
    ///
    /// Clicking it arms the shelf: the next model chosen there is inserted
    /// here rather than replacing the selected block. Adding a pedal used to
    /// mean finding an empty slot and changing its model, which required
    /// knowing the slot topology — this puts the action where the pedal goes.
    fn insert_point(&mut self, ui: &mut egui::Ui, before: usize) {
        if theme::insert_point(ui, theme::BLOCK_HEIGHT)
            .on_hover_text("add a block here")
            .clicked()
        {
            self.inserting_at = Some(before);
            self.browsing = None;
            self.search.clear();
        }
    }

    /// As `insert_point`, on a row that spans more than one lane.
    fn insert_point_tall(&mut self, ui: &mut egui::Ui, before: usize, tall: f32) {
        if theme::insert_point(ui, tall)
            .on_hover_text("add a block here")
            .clicked()
        {
            self.inserting_at = Some(before);
            self.browsing = None;
            self.search.clear();
        }
    }

    /// One block on the centre line, vertically centred against `tall`.
    fn block_at(&mut self, ui: &mut egui::Ui, slot: usize, tall: f32) -> Option<usize> {
        let i = self.index_of(slot)?;
        let block = self.chain[i].clone();
        let art = self.artwork(&block);
        let colour = self.block_colour(&block);
        let mut pick = None;
        ui.allocate_ui(egui::vec2(theme::BLOCK_WIDTH, tall), |ui| {
            ui.vertical(|ui| {
                ui.add_space((tall - theme::BLOCK_HEIGHT) / 2.0);
                if theme::block_button_tinted(
                    ui,
                    &self.slot_label(&block),
                    art.as_ref(),
                    i == self.selected,
                    block.enabled,
                    colour,
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
    fn lane_row(&mut self, ui: &mut egui::Ui, blocks: &[usize], longest: usize) -> Option<usize> {
        let mut pick = None;
        for (n, slot) in blocks.iter().enumerate() {
            let Some(i) = self.index_of(*slot) else {
                continue;
            };
            let block = self.chain[i].clone();
            let art = self.artwork(&block);
            if theme::block_button_tinted(
                ui,
                &self.slot_label(&block),
                art.as_ref(),
                i == self.selected,
                block.enabled,
                self.block_colour(&block),
            )
            .clicked()
            {
                pick = Some(i);
            }
            if n + 1 < blocks.len() {
                self.insert_point(ui, *slot + 1);
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
    fn endpoint(&mut self, ui: &mut egui::Ui, slot: usize, tall: f32) -> Option<usize> {
        let i = self.index_of(slot)?;
        let block = self.chain[i].clone();
        let art = self.artwork(&block);
        let colour = self.block_colour(&block);
        let mut pick = None;
        ui.allocate_ui(egui::vec2(theme::BLOCK_WIDTH, tall), |ui| {
            // Centred by hand. `centered_and_justified` stretches the widget to
            // fill instead, which made the endpoints twice the height of every
            // other tile.
            ui.vertical(|ui| {
                ui.add_space((tall - theme::BLOCK_HEIGHT) / 2.0);
                if theme::block_button_tinted(
                    ui,
                    &self.slot_label(&block),
                    art.as_ref(),
                    i == self.selected,
                    block.enabled,
                    colour,
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

    /// The block being edited, given the whole middle of the window.
    ///
    /// This used to share a column with the model list, which made choosing a
    /// different pedal look as important as adjusting the one you have. The
    /// pedal is the work; the shelf is a side trip, so it is a panel of its
    /// own beside this one.
    fn editor(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(block) = self.chain.get(self.selected).cloned() else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Connect a device to begin").color(theme::DIM));
                });
                return;
            };

            self.pedal_header(ui, &block);
            ui.separator();

            if !self.is_effect(&block) {
                self.endpoint_editor(ui, &block);
                return;
            }

            self.bypass_assignment(ui, &block);

            let Some(model) = self.slot_model(&block).cloned() else {
                ui.label(RichText::new("Install HX Edit for model names").color(theme::DIM));
                return;
            };
            egui::ScrollArea::vertical()
                .id_salt("pedal")
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    let art = self.artwork(&block);
                    self.pedal(
                        ui,
                        &model,
                        &block.values.clone(),
                        block.position,
                        false,
                        art.as_ref(),
                    );

                    // An Amp+Cab is two models sharing a block; the cab has its own
                    // controls and its own name.
                    if let Some(cab) = block.paired.and_then(|m| {
                        self.catalog
                            .as_ref()
                            .and_then(|c| c.model_number(m))
                            .cloned()
                    }) {
                        ui.add_space(14.0);
                        ui.separator();
                        let cab_art = self
                            .catalog
                            .as_ref()
                            .and_then(|c| c.artwork(&cab))
                            .map(|p| theme::Art::whole(format!("file://{}", p.display())));
                        self.pedal(
                            ui,
                            &cab,
                            &block.paired_values.clone(),
                            block.position,
                            true,
                            cab_art.as_ref(),
                        );
                    }
                });
        });
    }

    /// The pedal's name and the things you do to the block itself.
    fn pedal_header(&mut self, ui: &mut egui::Ui, block: &session::Block) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let colour = self.block_colour(block);
            theme::category_swatch(ui, colour);
            ui.heading(self.slot_label(block));

            if !self.is_effect(block) {
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Named rather than glyphed: "«" and "»" were doing the work of
                // "move this pedal earlier or later in the chain", and "Clear"
                // was doing the work of "take it out".
                if ui
                    .button("Remove")
                    .on_hover_text("take this block out of the chain")
                    .clicked()
                {
                    self.edit(Cmd::ClearBlock(block.position));
                }
                ui.add_space(6.0);
                let can_paste = self.copied_block.is_some();
                if ui
                    .add_enabled(can_paste, egui::Button::new("Paste"))
                    .on_hover_text("put the copied block here")
                    .clicked()
                {
                    if let Some(from) = self.copied_block {
                        self.edit(Cmd::CopyBlock {
                            from,
                            to: block.position as usize,
                        });
                    }
                }
                if ui.button("Copy").on_hover_text("copy this block").clicked() {
                    self.copied_block = Some(block.position as usize);
                    self.note(format!("copied {}", self.slot_label(block)));
                }
                ui.add_space(6.0);
                let last = self.chain.len().saturating_sub(1);
                if ui
                    .add_enabled(self.selected < last, egui::Button::new("Move later"))
                    .clicked()
                {
                    self.edit(Cmd::MoveBlock {
                        from: block.position as usize,
                        to: self.chain[self.selected + 1].position as usize,
                    });
                }
                if ui
                    .add_enabled(self.selected > 0, egui::Button::new("Move earlier"))
                    .clicked()
                {
                    self.edit(Cmd::MoveBlock {
                        from: block.position as usize,
                        to: self.chain[self.selected - 1].position as usize,
                    });
                }
                ui.add_space(10.0);
                let mut on = block.enabled;
                if ui.checkbox(&mut on, "Engaged").changed() {
                    self.edit(Cmd::SetEnabled {
                        block: block.position,
                        enabled: on,
                    });
                    self.chain[self.selected].enabled = on;
                }
            });
        });
    }

    /// Assign this block's bypass to a MIDI CC.
    ///
    /// Named for exactly what it does. HX Edit has a general assignment table
    /// — any block, any parameter, any source, including expression pedals and
    /// footswitches — and this is one corner of it: the bypass, from a MIDI CC.
    /// Calling it "Bypass follows" implied the rest of that table existed.
    fn bypass_assignment(&mut self, ui: &mut egui::Ui, block: &session::Block) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Bypass switched by")
                    .small()
                    .color(theme::DIM),
            );
            let mut cc = self.assign_cc;
            if ui
                .add(
                    egui::DragValue::new(&mut cc)
                        .range(0..=127)
                        .prefix("MIDI CC "),
                )
                .changed()
            {
                self.assign_cc = cc.clamp(0, 127);
            }
            if ui
                .button("Assign")
                .on_hover_text("make this CC toggle the block in and out")
                .clicked()
            {
                self.edit(Cmd::AssignCc {
                    block: block.position,
                    cc: self.assign_cc,
                });
            }
            ui.label(
                RichText::new("expression and footswitch sources are not implemented")
                    .small()
                    .color(theme::DIM),
            );
        });
        ui.add_space(2.0);
    }

    /// Inputs, outputs, splits and joins: routing, and their own parameters.
    ///
    /// Resolved by slot *kind*, never by model number. An endpoint reports
    /// model 0, and 0 is a real entry in the symbol table — a Cali 400 — so
    /// looking it up put an amp's name and knobs on the input block.
    fn endpoint_editor(&mut self, ui: &mut egui::Ui, block: &session::Block) {
        let Some(model) = self.slot_model(block).cloned() else {
            ui.add_space(8.0);
            ui.label(RichText::new("nothing to edit here").color(theme::DIM));
            return;
        };
        ui.add_space(8.0);
        let art = self.artwork(block);
        self.pedal(
            ui,
            &model,
            &block.values.clone(),
            block.position,
            false,
            art.as_ref(),
        );
    }

    /// The colour HX Edit gives this block's category.
    fn block_colour(&self, block: &session::Block) -> egui::Color32 {
        let fallback = theme::DIM;
        let Some(catalog) = self.catalog.as_ref() else {
            return fallback;
        };
        catalog
            .model_number(block.model)
            .and_then(|m| catalog.category_of(&m.id))
            .and_then(|c| catalog.category(c))
            .map(|c| theme::category_colour(c.colour))
            .unwrap_or(fallback)
    }

    /// The shelf of models, beside the pedal rather than under it.
    ///
    /// Only categories you actually choose between appear. Input and Output
    /// are fixed ends of the signal path, Split and Merge are the junctions
    /// between lanes, and Connected Devices is settings for outboard gear —
    /// none is a pedal you swap in, and listing them raised the fair question
    /// of what picking one would even do. The endpoints and junctions are
    /// edited by clicking them in the chain; outboard gear lives in the
    /// device window.
    fn shelf(&mut self, ctx: &egui::Context) {
        let Some(block) = self.chain.get(self.selected).cloned() else {
            return;
        };
        if !self.is_effect(&block) {
            return;
        }
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };

        let showing = self
            .browsing
            .or_else(|| {
                catalog
                    .model_number(block.model)
                    .and_then(|m| catalog.category_of(&m.id))
            })
            .unwrap_or(1);
        let current_id = catalog.model_number(block.model).map(|m| m.id.clone());

        let mut chosen = None;
        let mut pick = None;

        egui::SidePanel::right("shelf")
            .default_width(430.0)
            .width_range(280.0..=620.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("SWAP FOR").small().color(theme::DIM));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search")
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.add_space(4.0);

                // Categories as coloured chips, in HX Edit's own colours.
                ui.horizontal_wrapped(|ui| {
                    for category in catalog.categories() {
                        if !category.is_effect() || catalog.models_in(category.id).is_empty() {
                            continue;
                        }
                        let colour = theme::category_colour(category.colour);
                        let on = self.search.is_empty() && category.id == showing;
                        if theme::category_chip(ui, &category.name, colour, on).clicked() {
                            chosen = Some(category.id);
                        }
                    }
                });
                ui.separator();

                let needle = self.search.to_lowercase();
                let models: Vec<&hx_catalog::Model> = if needle.is_empty() {
                    catalog.models_in(showing)
                } else {
                    catalog
                        .models()
                        .filter(|m| m.name.to_lowercase().contains(&needle))
                        .filter(|m| {
                            catalog
                                .category_of(&m.id)
                                .is_some_and(|c| catalog.category(c).is_some_and(|c| c.is_effect()))
                        })
                        .collect()
                };

                egui::ScrollArea::vertical()
                    .id_salt("shelf-models")
                    .show(ui, |ui| {
                        if models.is_empty() {
                            ui.label(RichText::new("Nothing matches").color(theme::DIM));
                        }
                        ui.horizontal_wrapped(|ui| {
                            for model in models {
                                let selected = current_id.as_deref() == Some(model.id.as_str());
                                let art = catalog
                                    .artwork(model)
                                    .map(|p| theme::Art::whole(format!("file://{}", p.display())));
                                if theme::model_tile(ui, &model.name, art.as_ref(), selected)
                                    .clicked()
                                {
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

        if let Some(id) = chosen {
            self.browsing = Some(id);
            self.search.clear();
        }
        if let Some(model) = pick {
            match self.inserting_at.take() {
                Some(at) => self.edit(Cmd::InsertBlock { at, model }),
                None => self.edit(Cmd::SetModel {
                    block: block.position,
                    model,
                }),
            }
        }
    }

    /// Where an Input or Main L/R block is routed.
    ///
    /// Editable via opcode 42, captured from HX Edit's own routing clicks — a
    /// document write is accepted but ignored for this field. Returns the
    /// chosen destination, so the caller can send it once the catalog borrow
    /// has ended.
    fn routing_menu(
        &self,
        ui: &mut egui::Ui,
        model: &hx_catalog::Model,
        position: i64,
    ) -> Option<i64> {
        let current = self
            .chain
            .iter()
            .find(|b| b.position == position)
            .and_then(|b| b.routing)?;
        let catalog = self.catalog.as_ref()?;
        let param = model
            .params
            .iter()
            .find(|p| p.id == "@input" || p.id == "@output")?;
        let choices = catalog.choices(param)?;

        let mut chosen = None;
        let showing = choices
            .get(current.max(0) as usize)
            .cloned()
            .unwrap_or_else(|| current.to_string());

        ui.horizontal(|ui| {
            ui.label(RichText::new(&param.name).small().color(theme::DIM));
            egui::ComboBox::from_id_salt(("routing", position))
                .selected_text(RichText::new(showing).color(theme::ACCENT))
                .width(240.0)
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
        chosen.filter(|to| *to != current)
    }

    /// Draw one model's controls.    /// Draw one model's controls. Used for both halves of an Amp+Cab block, so
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
        art: Option<&theme::Art>,
    ) {
        let Some(catalog) = self.catalog.as_ref() else {
            for (i, value) in values.iter().enumerate() {
                ui.label(format!("{i}: {value}"));
            }
            return;
        };

        let mut edit = None;
        // The pedal, at a size worth looking at. This is the thing being
        // worked on, so it gets the room; the shelf next door is deliberately
        // smaller.
        ui.vertical_centered(|ui| {
            if let Some(art) = art {
                theme::pedal_image(ui, art, 240.0);
            }
            ui.add_space(4.0);
            ui.label(RichText::new(&model.name).heading());
        });
        ui.add_space(10.0);
        let reroute = self.routing_menu(ui, model, position);

        // Values arrive in the order the device indexes them, which the catalog
        // knows how to reproduce — it is not simply the model's parameter list.
        // An input's list starts with `@input`, which carries no value, and
        // using it directly shifted every knob by one.
        let params = catalog.ordered_params(model);

        // Knobs wrap onto as many rows as the width allows, like a pedal's
        // face, and sit under it rather than off to one side.
        let row_width = (params.len().min(8) as f32) * 88.0;
        let indent = ((ui.available_width() - row_width) / 2.0).max(0.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(indent);
            for (index, value) in values.iter().enumerate() {
                let Some(param) = params.get(index).copied() else {
                    continue;
                };
                let mut current = *value;

                ui.allocate_ui(egui::vec2(84.0, 100.0), |ui| {
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

        if let Some(to) = reroute {
            self.edit(Cmd::SetRouting {
                block: position,
                to,
            });
        }
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
            self.edit(Cmd::SetParam {
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
