//! A cross-platform editor for HX hardware, laid out the way HX Edit is so the
//! muscle memory carries over: presets down the left, the signal chain across
//! the top, and the selected block's model browser and parameters below.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use egui::RichText;
use hx_catalog::{Catalog, Kind};

mod config;
mod library;
mod session;
mod theme;
mod wav;

pub use session::{spawn, ApplyBlock, Cmd, Evt};

/// A tone file opened for a look before anything touches the pedal. It is
/// drawn by the same renderer as the loaded chain - one renderer, and the
/// preview simply runs it in display mode - and it carries what Load needs.
struct Preview {
    name: String,
    /// "Full rig, for FRFR or a PA" - what the tone is, in one line.
    line: String,
    chain: Vec<session::Block>,
    layout: hx_proto::preset::Layout,
    skipped: Vec<String>,
    load: LoadKind,
    /// Which preset Load writes into. Defaults to the one open now.
    dest: i64,
    /// The file this preview came from, so Keep knows what to copy.
    source: std::path::PathBuf,
}

/// How a previewed tone reaches the pedal: a byte-exact document is written
/// whole; a symbolic tone clears the chain and builds it back block by block.
enum LoadKind {
    Document(Vec<u8>),
    Steps(Vec<session::ApplyBlock>),
}

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
    /// Where a click on a `+` in the chain wants to add a block, and where on
    /// screen to put the picker.
    inserting_at: Option<usize>,
    insert_pos: Option<egui::Pos2>,
    /// When the picker opened. The click that opens it is still in the input
    /// egui reports, and egui may run several passes for one frame, so a frame
    /// counter is not enough to tell "the opening click" from "a click
    /// somewhere else" — a moment of grace is.
    insert_opened: Option<std::time::Instant>,
    /// Set while the device is fetching a preset. Loading one takes about a
    /// second, and a window that does not change for a second looks broken.
    loading: bool,
    /// When the worker started its current device conversation, if it is in
    /// one. Edits take real round trips; past a moment, the title bar says
    /// so with a spinner rather than letting the window look stuck.
    busy_since: Option<std::time::Instant>,
    /// A resource extraction running in the background, and the last thing
    /// worth telling the user about one. Extracting reads a whole installer,
    /// which is far too slow for the UI thread.
    extracting: Option<std::sync::mpsc::Receiver<Result<usize, String>>>,
    onboarding_status: Option<String>,
    /// Whether the welcome window is up. It opens on first launch without
    /// the model data and closes itself the moment extraction succeeds. It
    /// does not dismiss: without the model data there are no names, no knob
    /// ranges, and no pictures, so there is nothing honest to dismiss *to*.
    show_onboarding: bool,
    /// When taps were registered, for working out a tapped tempo.
    taps: Vec<std::time::Instant>,
    /// The slot being dragged along the chain.
    dragging: Option<usize>,
    /// A fork or merge being dragged along the main line: its slot, and
    /// whether it is the split.
    dragging_junction: Option<(usize, bool)>,
    /// Where each gap in the chain sits this frame, by the slot it inserts
    /// before — where a dragged block or junction can land. Rebuilt every
    /// frame; resolving a drop from a stale frame moved blocks nobody asked
    /// to move.
    gap_rects: Vec<(usize, egui::Rect)>,
    /// Where each block sits this frame, for dropping one onto another.
    block_rects: Vec<(usize, egui::Rect)>,
    /// The offered branch this frame — the slot a drop on it takes, and
    /// where it was drawn. Dragging a block onto the ghost is how a hand
    /// says "run this one in parallel".
    ghost_target: Option<(usize, egui::Rect)>,
    /// Whether the edit buffer has changes the preset does not.
    ///
    /// The device edits a scratch copy: a changed parameter is audible at once
    /// but vanishes on reload unless it is saved. An editor that does not say
    /// so loses people's work quietly, so this drives a dot in the title.
    dirty: bool,
    selected: usize,
    /// Category chosen in the browser, or none to follow the current block.
    browsing: Option<u32>,
    /// Scroll the preset list to the selection on the next frame — set when a
    /// different preset loads, so following along from the pedal's own
    /// front panel keeps the list in view without fighting manual scrolling.
    reveal_preset: bool,

    irs: Vec<(i64, String)>,
    setlists: Vec<String>,
    /// Which setlist the preset list is showing. Only reachable through the
    /// picker, which appears when a device has more than one — an HX Stomp has
    /// a single list, so on that hardware this stays at zero.
    setlist: i64,

    /// Favorites and other settings that persist across runs.
    config: config::Config,
    /// Show only favorited presets in the list.
    show_favorites_only: bool,

    /// A tone file opened for a look, read and shown without touching the device.
    preview: Option<Preview>,
    /// Draw the chain without its editing affordances: no insert gaps, no
    /// ghost branch, no drags. The preview runs the renderer this way.
    display_only: bool,
    /// The kept tones on disk, shown under the preset list when open.
    show_library: bool,
    library: Vec<std::path::PathBuf>,
    /// Editable tempo, so typing does not fight the device's value.
    tempo_draft: Option<String>,
    /// A parameter value being typed, by block position and parameter index,
    /// with the text so far. One at a time, like every other draft here.
    param_draft: Option<(i64, i64, String)>,
    /// Snapshot being renamed, with its draft name.
    snapshot_draft: Option<(usize, String)>,
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
    /// Into the library, as a portable .hlx named after the preset.
    Library,
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
            insert_pos: None,
            insert_opened: None,
            loading: false,
            busy_since: None,
            extracting: None,
            onboarding_status: None,
            show_onboarding: false,
            taps: Vec::new(),
            dragging: None,
            dragging_junction: None,
            gap_rects: Vec::new(),
            block_rects: Vec::new(),
            ghost_target: None,
            selected: 0,
            browsing: None,
            reveal_preset: false,
            irs: Vec::new(),
            setlists: Vec::new(),
            setlist: 0,
            config: config::Config::load(),
            show_favorites_only: false,
            preview: None,
            display_only: false,
            show_library: false,
            library: Vec::new(),
            tempo_draft: None,
            param_draft: None,
            snapshot_draft: None,
            assign_cc: 1,
            renaming: None,
            log: Vec::new(),
            show_activity: false,
            status: "Looking for a device…".into(),
            current_snapshot: 0,
        };
        // Without the model data there is nothing to edit with, so the
        // welcome window opens immediately and stays until the data exists.
        app.show_onboarding = app.catalog.is_none();
        // Machines that already have HX Edit need no ceremony at all: lift
        // the data from the installation while the welcome says so.
        if app.show_onboarding && hx_catalog::extract::installed_resources().is_some() {
            app.extract_installed();
        }
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
                    dirty,
                }) => {
                    self.layout = layout;
                    // The worker's word, not a blanket reset: most reloads are
                    // edits taking effect, and those leave changes to save.
                    self.dirty = dirty;
                    self.reveal_preset = index != self.preset_index;
                    self.loading = false;
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
                Ok(Evt::Busy(on)) => {
                    self.busy_since = if on {
                        self.busy_since.or(Some(std::time::Instant::now()))
                    } else {
                        None
                    };
                }
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
                        CopyTarget::Library => self.keep_document(&name, &blob),
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

    /// What to call a slot. Inputs and outputs have no model to name; splits
    /// and joins do — "Split Y", "Mixer" — and the real name says much more
    /// than the slot kind.
    fn slot_label(&self, block: &session::Block) -> String {
        use hx_proto::preset::Kind;
        let named = |fallback: &str| {
            let name = self.model_name(block.model);
            if name.is_empty() {
                fallback.to_owned()
            } else {
                name
            }
        };
        match block.kind {
            Kind::Input => "Input".into(),
            Kind::Output => "Output".into(),
            Kind::Split => named("Split"),
            Kind::Join => named("Join"),
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

        self.shortcuts(ctx);
        self.finish_extraction();
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
        self.insert_picker(ctx);
        self.device_window(ctx);
        self.preview_window(ctx);
        // Over everything: the one step the app cannot work without.
        self.onboarding_modal(ctx);
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

                        ui.separator();
                        if ui
                            .small_button(RichText::new("♥ Sponsor").small())
                            .on_hover_text("support stompchain's development")
                            .clicked()
                        {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(
                                "https://github.com/sponsors/crmne",
                            ));
                        }
                        if ui
                            .add(
                                egui::Label::new(
                                    RichText::new("made with ♥ by Carmine Paolino")
                                        .small()
                                        .color(theme::DIM),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text("paolino.me")
                            .clicked()
                        {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab("https://paolino.me"));
                        }
                    });
                });
            });
    }

    /// The editing keys every editor answers to. Skipped while something has
    /// keyboard focus: Ctrl+Z inside a text field is the field's own undo.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        use egui::{Key, KeyboardShortcut, Modifiers};
        const REDO: KeyboardShortcut =
            KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
        const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

        let pressed = |shortcut: &KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(shortcut));
        let live = matches!(self.connection, Connection::Online);
        // The shifted variant first, or plain Ctrl+Z would swallow it.
        if pressed(&REDO) {
            if live && self.redo_depth > 0 {
                self.send(Cmd::Redo);
            }
        } else if pressed(&UNDO) && live && self.undo_depth > 0 {
            self.send(Cmd::Undo);
        }
        if pressed(&SAVE) && self.dirty {
            self.send(Cmd::SavePreset);
        }
    }

    /// Undo and redo, where you can see them.
    fn history_buttons(&mut self, ui: &mut egui::Ui) {
        let live = matches!(self.connection, Connection::Online);
        let hint =
            |ui: &egui::Ui, m, k| ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(m, k));
        let redo_hint = hint(
            ui,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        );
        let undo_hint = hint(ui, egui::Modifiers::COMMAND, egui::Key::Z);
        if ui
            .add_enabled(live && self.redo_depth > 0, egui::Button::new("Redo"))
            .on_hover_text(format!("put back what undo took away ({redo_hint})"))
            .clicked()
        {
            self.send(Cmd::Redo);
        }
        if ui
            .add_enabled(live && self.undo_depth > 0, egui::Button::new("Undo"))
            .on_hover_text(format!("step back through changes ({undo_hint})"))
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
        let hint = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::S,
        ));
        let hit = ui
            .add_enabled(self.dirty, egui::Button::new("Save"))
            .on_hover_text(format!("write these changes into the preset ({hint})"))
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
        // Import always shows the tone first - drawn as its chain - and loads
        // only when asked. One door for both file kinds; reading needs the
        // catalog, not a device, so looking works offline too.
        if small(ui, "IMPORT", self.catalog.is_some())
            .on_hover_text("open a tone file; you see it before it goes to the pedal")
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("HX tone", &["hxpreset", "hlx"])
                .pick_file()
            {
                self.open_tone_file(&path);
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

    /// The loaded preset, click-to-rename.
    fn preset_title(&mut self, ui: &mut egui::Ui) {
        if self.preset_index < 0 {
            return;
        }
        if self.loading {
            ui.spinner();
        } else if self
            .busy_since
            .is_some_and(|since| since.elapsed() > Duration::from_millis(150))
        {
            // An edit is on its way to the pedal. Only conversations that
            // last are worth announcing — a flicker per knob tick is noise.
            ui.spinner().on_hover_text("writing to the pedal…");
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

    /// Work out a tempo from the intervals between taps.
    ///
    /// Taps more than two seconds apart start a new measurement rather than
    /// averaging in a stale one, and it waits for two taps before saying
    /// anything, because one tap is not an interval.
    fn tap_tempo(&mut self) -> Option<f32> {
        let now = std::time::Instant::now();
        if let Some(previous) = self.taps.last() {
            if now.duration_since(*previous) > Duration::from_secs(2) {
                self.taps.clear();
            }
        }
        self.taps.push(now);
        // Four intervals is enough to steady it without lagging behind.
        if self.taps.len() > 5 {
            self.taps.remove(0);
        }
        if self.taps.len() < 2 {
            return None;
        }
        let span = self.taps.last()?.duration_since(self.taps[0]).as_secs_f32();
        let intervals = (self.taps.len() - 1) as f32;
        let bpm = 60.0 * intervals / span;
        (20.0..=999.0).contains(&bpm).then_some(bpm)
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

        // Tap it in, which is how anyone actually finds a tempo.
        if ui
            .button("Tap")
            .on_hover_text("tap in time to set the tempo")
            .clicked()
        {
            if let Some(bpm) = self.tap_tempo() {
                self.tempo = Some(bpm);
                self.edit(Cmd::SetTempo(bpm));
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
            .default_width(216.0)
            .width_range(150.0..=340.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                // The actions sit on the list they act on, the way HX Edit
                // puts COPY / PASTE / IMPORT / EXPORT on its preset header. A
                // menu called "Preset" at the top of the window made you go
                // looking somewhere else for something that belongs here.
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PRESETS").small().color(theme::DIM));
                    let mark = if self.show_favorites_only { "★" } else { "☆" };
                    let color = if self.show_favorites_only {
                        theme::ACCENT
                    } else {
                        theme::DIM
                    };
                    if ui
                        .add(egui::Button::new(RichText::new(mark).small().color(color)).frame(false))
                        .on_hover_text("Show favorites only")
                        .clicked()
                    {
                        self.show_favorites_only = !self.show_favorites_only;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.preset_actions(ui);
                    });
                });
                ui.separator();

                self.library_shelf(ui);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Show slot labels alone until the names arrive.
                        let total = if self.presets.is_empty() {
                            self.preset_count as i64
                        } else {
                            self.presets.len() as i64
                        };
                        let setlist = self.setlist;
                        let mut load = None;
                        let mut toggle = None;
                        let mut shown_any = false;
                        for index in 0..total {
                            let fav = self.config.is_favorite(setlist, index);
                            if self.show_favorites_only && !fav {
                                continue;
                            }
                            shown_any = true;
                            let name = self
                                .presets
                                .get(index as usize)
                                .cloned()
                                .unwrap_or_default();
                            let selected = index == self.preset_index;
                            let label = format!("{}  {}", hx_proto::rpc::slot_label(index), name);
                            ui.horizontal(|ui| {
                                // The star leads the row, clear of the scrollbar
                                // that overlaps the right edge and eats the click.
                                let mark = if fav { "★" } else { "☆" };
                                let color = if fav { theme::ACCENT } else { theme::DIM };
                                if ui
                                    .add(egui::Button::new(RichText::new(mark).color(color)).frame(false))
                                    .on_hover_text(if fav { "Remove favorite" } else { "Favorite" })
                                    .clicked()
                                {
                                    toggle = Some(index);
                                }
                                let text = if selected {
                                    RichText::new(&label).color(theme::ACCENT).strong()
                                } else {
                                    RichText::new(&label)
                                };
                                // A plain, content-sized label: left-aligned as
                                // before, and never centered (no forced width).
                                let row = ui.selectable_label(selected, text);
                                if row.clicked() {
                                    load = Some(index);
                                }
                                // A preset picked on the pedal itself should be in
                                // view here too, without fighting manual scrolling.
                                if selected && self.reveal_preset {
                                    row.scroll_to_me(Some(egui::Align::Center));
                                    self.reveal_preset = false;
                                }
                            });
                        }
                        if self.show_favorites_only && !shown_any {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("No favorites yet. Tap a star to add one.")
                                    .small()
                                    .color(theme::DIM),
                            );
                        }
                        if let Some(index) = toggle {
                            self.config.toggle_favorite(setlist, index);
                        }
                        if let Some(index) = load {
                            // Loading takes about a second. Say so, or the window
                            // sits unchanged and looks like it missed the click.
                            self.loading = true;
                            self.preset_index = index;
                            self.send(Cmd::SelectPreset(index));
                        }
                    });
            });
    }

    /// Keep the loaded preset's document in the library: as portable .hlx
    /// when the catalog can name its models, as the exact bytes otherwise.
    fn keep_document(&mut self, name: &str, blob: &[u8]) {
        let stem = sanitise(name);
        let kept = match (hx_proto::preset::Preset::parse(blob), self.catalog.as_ref()) {
            (Some(preset), Some(catalog)) => {
                let written = hx_catalog::to_hlx(&preset, catalog, name);
                for skipped in &written.skipped {
                    self.note(format!("kept without {skipped}"));
                }
                library::keep_bytes(&stem, "hlx", written.to_pretty_string().as_bytes())
            }
            _ => library::keep_bytes(&stem, "hxpreset", blob),
        };
        match kept {
            Ok(_) => {
                self.library = library::entries();
                self.show_library = true;
                self.note(format!("kept {name} in the library"));
            }
            Err(why) => self.note(why),
        }
    }

    /// The kept tones, under the preset list: files, not slots, so they
    /// outlive any pedal. Closed it is one quiet line; open it lists the
    /// library, and a click opens the tone's preview.
    fn library_shelf(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("library")
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.add_space(2.0);
                // The arrow says this opens; the count says it holds something.
                let arrow = if self.show_library { "⏷" } else { "⏵" };
                let title = if self.library.is_empty() {
                    format!("{arrow} LIBRARY")
                } else {
                    format!("{arrow} LIBRARY ({})", self.library.len())
                };
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(title).small().color(theme::DIM))
                                .frame(false),
                        )
                        .on_hover_text("tones kept on this computer")
                        .clicked()
                    {
                        self.show_library = !self.show_library;
                        if self.show_library {
                            self.library = library::entries();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let live = matches!(self.connection, Connection::Online)
                            && self.preset_index >= 0;
                        if ui
                            .add_enabled(
                                live,
                                egui::Button::new(RichText::new("KEEP THIS").small()).frame(false),
                            )
                            .on_hover_text("keep the loaded preset in the library")
                            .clicked()
                        {
                            self.pending_copy = CopyTarget::Library;
                            self.send(Cmd::CopyPreset);
                        }
                    });
                });
                if self.show_library {
                    let mut opened = None;
                    let mut removed = None;
                    egui::ScrollArea::vertical()
                        .id_salt("library-shelf")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for path in &self.library {
                                let name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("tone");
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(false, name)
                                        .on_hover_text("open this tone")
                                        .clicked()
                                    {
                                        opened = Some(path.clone());
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("✕")
                                                            .small()
                                                            .color(theme::DIM),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_text("remove from the library")
                                                .clicked()
                                            {
                                                removed = Some(path.clone());
                                            }
                                        },
                                    );
                                });
                            }
                            if self.library.is_empty() {
                                ui.label(
                                    RichText::new(
                                        "Nothing kept yet. Keep a tone from its preview.",
                                    )
                                    .small()
                                    .color(theme::DIM),
                                );
                            }
                        });
                    if let Some(path) = opened {
                        self.open_tone_file(&path);
                    }
                    if let Some(path) = removed {
                        let name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("tone")
                            .to_owned();
                        match library::remove(&path) {
                            Ok(()) => self.note(format!("removed {name}")),
                            Err(why) => self.note(why),
                        }
                        self.library = library::entries();
                    }
                }
                ui.add_space(2.0);
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
                        "The device's IR library, shared by every preset. Add an IR block \
                         to a chain, then point it at one of these slots.",
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
                        "The device's own global settings — the same ones HX Edit's \
                         preferences write. They belong to the device, not the preset.",
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

    /// Start pulling model data out of an HX Edit installer, off the UI
    /// thread, and say so.
    fn extract_resources(&mut self, installer: std::path::PathBuf) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.onboarding_status = Some(format!(
            "reading {}…",
            installer
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
        self.extracting = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(hx_catalog::extract::from_installer(&installer));
        });
    }

    /// Copy the model data from an HX Edit installed on this machine,
    /// off the UI thread.
    fn extract_installed(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.onboarding_status =
            Some("found HX Edit installed on this machine; copying its data…".into());
        self.extracting = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(hx_catalog::extract::from_installed());
        });
    }

    /// Collect a finished extraction and put the catalog straight to work:
    /// no restart, the names and artwork simply appear.
    fn finish_extraction(&mut self) {
        let Some(rx) = &self.extracting else { return };
        match rx.try_recv() {
            Ok(Ok(count)) => {
                self.extracting = None;
                self.catalog = Catalog::load().ok();
                self.onboarding_status = if self.catalog.is_some() {
                    self.show_onboarding = false;
                    self.note(format!("extracted {count} resource files"));
                    None
                } else {
                    Some("extracted files, but the catalog would not load".into())
                };
            }
            Ok(Err(why)) => {
                self.extracting = None;
                self.onboarding_status = Some(why);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.extracting = None;
                self.onboarding_status = Some("extraction stopped unexpectedly".into());
            }
        }
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
            // A preset, an impulse response and an HX Edit installer are all
            // "a file you drop on the window"; the extension decides.
            if path.extension().is_some_and(|e| {
                e == "hxpreset" || e.eq_ignore_ascii_case("hlx")
            }) {
                self.open_tone_file(&path);
                continue;
            }
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dmg") || e.eq_ignore_ascii_case("exe"))
            {
                self.extract_resources(path);
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

    /// Open a tone file of either kind for a look. The extension decides the
    /// reader; both end at the same preview window.
    fn open_tone_file(&mut self, path: &std::path::Path) {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("hlx")) {
            self.preview_hlx(path);
        } else {
            self.preview_hxpreset(path);
        }
    }

    /// What a tone is, in one line: its makeup and what to play it through.
    fn tone_line(tone: &hx_catalog::Tone) -> String {
        let content = match tone.chain_content {
            hx_catalog::ChainContent::FullRig => "Full rig",
            hx_catalog::ChainContent::AmpAndCab => "Amp and cab",
            hx_catalog::ChainContent::AmpOnly => "Amp, no cab",
            hx_catalog::ChainContent::EffectsOnly => "Effects only",
        };
        let output = match tone.output_target_guess {
            hx_catalog::OutputTarget::FrfrPa => "for FRFR or a PA",
            hx_catalog::OutputTarget::GuitarCabOrDi => "for a real cab or the front of an amp",
        };
        format!("{content}, {output}")
    }

    /// Read a .hlx and show what it is, without touching the device.
    fn preview_hlx(&mut self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.note(format!("could not read {}: {e}", path.display()));
                return;
            }
        };
        let Some(catalog) = self.catalog.as_ref() else {
            self.note("reading a tone needs HX Edit's model data first".into());
            return;
        };
        let json: serde_json::Value = match serde_json::from_str(&text) {
            Ok(j) => j,
            Err(e) => {
                self.note(format!("{} is not a readable .hlx: {e}", path.display()));
                return;
            }
        };
        let tone = hx_catalog::inspect(&json, catalog);

        // The tone's blocks resolved to what applying them needs: parameter
        // names become indexes, switches marked as such. A parameter the
        // catalog cannot place is reported, not dropped.
        let mut skipped = tone.skipped.clone();
        let mut blocks = Vec::new();
        for block in tone.blocks.iter().filter(|b| b.path == 0) {
            let mut params = Vec::new();
            for (name, value) in &block.params {
                match catalog.param_index(block.model_number, name) {
                    Some(index) => {
                        let switch = catalog
                            .param(block.model_number, index)
                            .is_some_and(|p| p.kind == Kind::Switch);
                        params.push((index as i64, *value, switch));
                    }
                    None => skipped.push(format!("{}: no parameter {name:?}", block.model_name)),
                }
            }
            blocks.push(session::ApplyBlock {
                model: block.model_number,
                enabled: block.enabled,
                params,
            });
        }
        if tone.blocks.iter().any(|b| b.path == 1) {
            skipped.push("the second signal path is shown but not loaded yet".into());
        }

        // A chain to draw: input, the blocks, output - one path per DSP,
        // positions synthesised since a .hlx carries no slot array.
        let mut chain = Vec::new();
        let mut layout = hx_proto::preset::Layout::default();
        let mut base = 0usize;
        for path_no in [0u8, 1] {
            let on_path: Vec<&hx_catalog::ToneBlock> =
                tone.blocks.iter().filter(|b| b.path == path_no).collect();
            if on_path.is_empty() {
                continue;
            }
            let input = base;
            chain.push(session::Block {
                position: input as i64,
                routing: Some(0),
                kind: hx_proto::preset::Kind::Input,
                model: 0,
                enabled: true,
                values: Vec::new(),
                paired: None,
                paired_values: Vec::new(),
            });
            let mut head = Vec::new();
            for (i, block) in on_path.iter().enumerate() {
                let position = base + 1 + i;
                chain.push(session::Block {
                    position: position as i64,
                    routing: None,
                    kind: hx_proto::preset::Kind::Block,
                    model: block.model_number,
                    enabled: block.enabled,
                    values: Vec::new(),
                    paired: None,
                    paired_values: Vec::new(),
                });
                head.push(position);
            }
            let output = base + 1 + on_path.len();
            chain.push(session::Block {
                position: output as i64,
                routing: Some(0),
                kind: hx_proto::preset::Kind::Output,
                model: 0,
                enabled: true,
                values: Vec::new(),
                paired: None,
                paired_values: Vec::new(),
            });
            layout.paths.push(hx_proto::preset::Path {
                input: Some(input),
                output: Some(output),
                head,
                ..Default::default()
            });
            base = output + 1;
        }

        self.note(format!("previewing {}", tone.name));
        self.preview = Some(Preview {
            name: tone.name.clone(),
            line: Self::tone_line(&tone),
            chain,
            layout,
            skipped,
            load: LoadKind::Steps(blocks),
            dest: self.preset_index.max(0),
            source: path.to_owned(),
        });
    }

    /// Read a .hxpreset and show what it is, through the same codec the .hlx
    /// path uses, so one window understands either. The original bytes are kept
    /// so Load can write the document exactly.
    fn preview_hxpreset(&mut self, path: &std::path::Path) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.note(format!("could not read {}: {e}", path.display()));
                return;
            }
        };
        let Some(catalog) = self.catalog.as_ref() else {
            self.note("reading a tone needs HX Edit's model data first".into());
            return;
        };
        let Some(preset) = hx_proto::preset::Preset::parse(&bytes) else {
            self.note(format!("{} is not a readable preset", path.display()));
            return;
        };
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        // The document knows its own chain and layout; the codec supplies the
        // one-line reading of what the tone is.
        let written = hx_catalog::to_hlx(&preset, catalog, name);
        let mut tone = hx_catalog::inspect(&written.document, catalog);
        tone.skipped.extend(written.skipped);
        self.note(format!("previewing {name}"));
        self.preview = Some(Preview {
            name: name.to_owned(),
            line: Self::tone_line(&tone),
            chain: session::chain_of(&preset),
            layout: preset.layout(),
            skipped: tone.skipped,
            load: LoadKind::Document(bytes),
            dest: self.preset_index.max(0),
            source: path.to_owned(),
        });
    }

    /// How wide a chain draws, by the renderer's own arithmetic: the space
    /// before the input, a column per block, the junctions and the padded
    /// stretch when a path splits, and the wire into the output.
    fn chain_width(&self, layout: &hx_proto::preset::Layout) -> f32 {
        let mut widest = 0.0f32;
        for path in &layout.paths {
            let mut width = 6.0;
            if path.input.is_some() {
                width += theme::BLOCK_WIDTH;
            }
            width += path.head.len() as f32 * theme::COLUMN;
            if !path.lanes.is_empty() {
                let stretch = path
                    .lanes
                    .iter()
                    .map(|lane| self.lane_width(lane))
                    .fold(0.0, f32::max);
                width += 2.0 * theme::JUNCTION_WIDTH + stretch;
            }
            width += path.tail.len() as f32 * theme::COLUMN;
            if path.output.is_some() {
                width += theme::WIRE_WIDTH + theme::BLOCK_WIDTH;
            }
            widest = widest.max(width);
        }
        widest
    }

    /// A tone file, shown the way the app always shows a tone: by the chain
    /// renderer itself, in display mode. Load asks where it should land, so
    /// nothing is overwritten by surprise; nothing touches the device before.
    fn preview_window(&mut self, ctx: &egui::Context) {
        let Some(mut preview) = self.preview.take() else {
            return;
        };
        let live = matches!(self.connection, Connection::Online);
        // The window opens at the chain's full width, and narrower only when
        // the screen cannot hold it - then the chain scrolls inside.
        let natural = self.chain_width(&preview.layout) + 24.0;
        let width = natural.min(ctx.screen_rect().width() - 48.0);
        let mut open = true;
        let mut load = false;
        let mut cancel = false;

        // The one renderer draws whatever chain and layout the app holds, so
        // for the length of this window it holds the file's. The selection
        // stays with the real chain: a previewed tone has nothing selected.
        std::mem::swap(&mut self.chain, &mut preview.chain);
        std::mem::swap(&mut self.layout, &mut preview.layout);
        let selected = std::mem::replace(&mut self.selected, usize::MAX);
        self.display_only = true;

        let title = preview.name.clone();
        egui::Window::new(title)
            .open(&mut open)
            .resizable(true)
            .default_width(width)
            .show(ctx, |ui| {
                ui.label(RichText::new(&preview.line).color(theme::DIM));
                ui.add_space(6.0);
                egui::ScrollArea::horizontal()
                    .id_salt("tone-preview")
                    .show(ui, |ui| {
                        let paths = self.layout.paths.clone();
                        ui.vertical(|ui| {
                            for (n, path) in paths.iter().enumerate() {
                                if paths.len() > 1 {
                                    ui.label(
                                        RichText::new(format!("PATH {}", n + 1))
                                            .small()
                                            .color(theme::DIM),
                                    );
                                }
                                let _ = self.path_row(ui, path);
                            }
                        });
                    });
                for skipped in &preview.skipped {
                    ui.label(RichText::new(skipped).small().color(theme::DIM));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Load into").color(theme::DIM));
                    let total = if self.presets.is_empty() {
                        self.preset_count as i64
                    } else {
                        self.presets.len() as i64
                    };
                    let labels: Vec<String> = (0..total)
                        .map(|i| {
                            let name =
                                self.presets.get(i as usize).cloned().unwrap_or_default();
                            format!("{}  {}", hx_proto::rpc::slot_label(i), name)
                        })
                        .collect();
                    egui::ComboBox::from_id_salt("load-dest")
                        .selected_text(
                            labels
                                .get(preview.dest as usize)
                                .cloned()
                                .unwrap_or_default(),
                        )
                        .show_ui(ui, |ui| {
                            for (i, label) in labels.iter().enumerate() {
                                ui.selectable_value(&mut preview.dest, i as i64, label);
                            }
                        });
                    if ui
                        .add_enabled(live, egui::Button::new("Load"))
                        .on_hover_text("into that preset's edit buffer; Save keeps it")
                        .clicked()
                    {
                        load = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    // A tone worth keeping goes into the library, unless it
                    // is already there.
                    if !library::holds(&preview.source)
                        && ui
                            .button("Keep")
                            .on_hover_text("copy this file into your library")
                            .clicked()
                    {
                        match library::keep(&preview.source) {
                            Ok(kept) => {
                                preview.source = kept;
                                self.library = library::entries();
                                // Show the keep landing, or it looks like
                                // nothing happened.
                                self.show_library = true;
                                self.note(format!("kept {}", preview.name));
                            }
                            Err(why) => self.note(why),
                        }
                    }
                });
            });

        self.display_only = false;
        self.selected = selected;
        std::mem::swap(&mut self.chain, &mut preview.chain);
        std::mem::swap(&mut self.layout, &mut preview.layout);

        if load {
            self.loading = true;
            self.note(format!(
                "loading {} into {}",
                preview.name,
                hx_proto::rpc::slot_label(preview.dest)
            ));
            match preview.load {
                LoadKind::Document(bytes) => self.send(Cmd::LoadDocument {
                    dest: preview.dest,
                    bytes,
                }),
                LoadKind::Steps(blocks) => self.send(Cmd::LoadSteps {
                    dest: preview.dest,
                    name: preview.name,
                    blocks,
                }),
            }
        } else if open && !cancel {
            self.preview = Some(preview);
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
    /// The main line never changes height: input, output and everything the
    /// undivided signal passes through sit on one row, and a parallel branch
    /// hangs *below* the stretch it parallels, the way HX Edit draws it.
    /// Splits and joins are not blocks in the line either — they are drawn as
    /// the wiring forking and merging, still clickable for their own
    /// parameters.
    ///
    /// The number of lanes is not fixed at two: Helix and Helix LT carry two
    /// independent signal paths, so a preset that splits both has four.
    fn signal_chain(&mut self, ctx: &egui::Context) {
        let mut height = 40.0;
        for path in &self.layout.paths {
            height += theme::BLOCK_HEIGHT;
            height += path.lanes.len().saturating_sub(1) as f32 * theme::LANE_HEIGHT;
            if self.can_offer_branch(path) {
                height += 4.0 + theme::GHOST_HEIGHT;
            }
            if self.layout.paths.len() > 1 {
                height += 20.0;
            }
        }

        egui::TopBottomPanel::top("chain")
            .exact_height(height.clamp(126.0, ctx.screen_rect().height() * 0.55))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                if self.chain.is_empty() {
                    ui.centered_and_justified(|ui| {
                        if self.loading {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("loading…").color(theme::DIM));
                            });
                        } else {
                            ui.label(RichText::new("No preset loaded").color(theme::DIM));
                        }
                    });
                    return;
                }

                let mut pick = None;
                // Drag-to-scroll off: it claims the pointer press, so a
                // click-only widget like an insert point never completes its
                // click — and it would fight dragging a block along the chain,
                // which is the same gesture.
                egui::ScrollArea::both()
                    .drag_to_scroll(false)
                    .show(ui, |ui| {
                        self.gap_rects.clear();
                        self.block_rects.clear();
                        self.ghost_target = None;
                        let paths = self.layout.paths.clone();
                        ui.vertical(|ui| {
                            for (n, path) in paths.iter().enumerate() {
                                if paths.len() > 1 {
                                    ui.label(
                                        RichText::new(format!("PATH {}", n + 1))
                                            .small()
                                            .color(theme::DIM),
                                    );
                                }
                                pick = self.path_row(ui, path).or(pick);
                            }
                        });
                        self.block_drag(ui);
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

    /// One signal path: the main line straight across, branches hanging below.
    ///
    /// A split divides a *stretch* of the path, not all of it — the split
    /// records the slot it attaches before, and the blocks on either side of
    /// that stretch carry the undivided signal. The first lane of the divided
    /// stretch *is* the main line, so it stays in the row; the other branches
    /// are drawn beneath it between the fork and the merge.
    fn path_row(&mut self, ui: &mut egui::Ui, path: &hx_proto::preset::Path) -> Option<usize> {
        let mut pick = None;
        let below = path.lanes.len().saturating_sub(1);
        // Every lane spans the same stretch, so they are padded to the widest
        // and the merge lands where all of them end.
        let stretch = path
            .lanes
            .iter()
            .map(|l| self.lane_width(l))
            .fold(0.0, f32::max);

        // Captured while the main row is drawn, to place what hangs below it:
        // the branch rows start under the fork, and the ghost of an offered
        // branch runs from the input's edge to the output's.
        let mut fork_end = None;
        let mut input_rect = None;
        let mut output_left = None;

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(6.0);

                if let Some(input) = path.input {
                    let (hit, rect) = self.endpoint(ui, input);
                    pick = hit.or(pick);
                    input_rect = Some(rect);
                }
                // A gap before each block, so a chain can be built anywhere.
                for slot in &path.head {
                    self.insert_point(ui, *slot);
                    pick = self.block_at(ui, *slot).or(pick);
                }

                if !path.lanes.is_empty() {
                    if let Some(split) = path.split {
                        let (hit, rect) = self.junction(ui, split, below, true);
                        pick = hit.or(pick);
                        fork_end = Some(rect.right());
                    }
                    pick = self.lane_row(ui, &path.lanes[0], stretch).or(pick);
                    if let Some(join) = path.join {
                        let (hit, _) = self.junction(ui, join, below, false);
                        pick = hit.or(pick);
                    }
                }

                // And everything the recombined signal passes through.
                for slot in &path.tail {
                    self.insert_point(ui, *slot);
                    pick = self.block_at(ui, *slot).or(pick);
                }
                if let Some(output) = path.output {
                    self.insert_point(ui, output);
                    let (hit, rect) = self.endpoint(ui, output);
                    pick = hit.or(pick);
                    output_left = Some(rect.left());
                }
            });

            // The branches, aligned column for column under the stretch they
            // parallel.
            for lane in path.lanes.iter().skip(1) {
                ui.add_space(theme::LANE_HEIGHT - theme::BLOCK_HEIGHT);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if let Some(x) = fork_end {
                        let indent = x - ui.cursor().min.x;
                        ui.add_space(indent.max(0.0));
                    }
                    pick = self.lane_row(ui, lane, stretch).or(pick);
                });
            }

            self.junction_drag(ui, path);

            // The offer of a parallel branch, where it would actually run.
            if self.can_offer_branch(path) {
                if let (Some(input), Some(right)) = (input_rect, output_left) {
                    self.ghost_branch(ui, path, input, right);
                }
            }
        });
        pick
    }

    /// Follow a fork or merge being dragged along the main line.
    ///
    /// Every gap it can land in shows a dot while the drag lasts, the one
    /// nearest the pointer takes the accent, and releasing commits the move.
    /// A fork can go anywhere between the input and the merge; a merge,
    /// anywhere between the fork and the output. Escape lets go.
    fn junction_drag(&mut self, ui: &mut egui::Ui, path: &hx_proto::preset::Path) {
        if self.display_only {
            return;
        }
        let Some((slot, opening)) = self.dragging_junction else {
            return;
        };
        let dragged = if opening { path.split } else { path.join };
        if dragged != Some(slot) {
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dragging_junction = None;
            return;
        }
        let Some((lowest, highest, current)) = attach_range(path, opening) else {
            return;
        };

        let candidates: Vec<(usize, egui::Rect)> = self
            .gap_rects
            .iter()
            .filter(|(before, _)| (lowest..=highest).contains(before))
            .copied()
            .collect();
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let nearest = pointer.and_then(|p| {
            candidates
                .iter()
                .min_by(|a, b| {
                    let da = (a.1.center().x - p.x).abs();
                    let db = (b.1.center().x - p.x).abs();
                    da.total_cmp(&db)
                })
                .copied()
        });

        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        for (before, rect) in &candidates {
            let hot = nearest.is_some_and(|(n, _)| n == *before);
            theme::attach_marker(ui, rect.center(), hot);
        }

        if ui.input(|i| i.pointer.any_released()) {
            if let Some((before, _)) = nearest {
                if before != current {
                    self.edit(Cmd::MoveJunction {
                        junction: slot,
                        before,
                    });
                }
            }
            self.dragging_junction = None;
        }
    }

    /// Whether to offer a parallel branch: the path has somewhere to put one,
    /// something to parallel, and nothing on the branch yet.
    fn can_offer_branch(&self, path: &hx_proto::preset::Path) -> bool {
        !self.display_only
            && path.lanes.is_empty()
            && !path.head.is_empty()
            && self.free_on_branch(path).is_some()
    }

    /// The dashed preview of the branch a click would create: it forks after
    /// the input, runs under the whole line, and merges before the output —
    /// which is exactly where the real one will go.
    fn ghost_branch(
        &mut self,
        ui: &mut egui::Ui,
        path: &hx_proto::preset::Path,
        input: egui::Rect,
        right: f32,
    ) {
        let Some(at) = self.free_on_branch(path) else {
            return;
        };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let indent = input.right() - ui.cursor().min.x;
            ui.add_space(indent.max(0.0));
            let hit = theme::ghost_branch(ui, (right - input.right()).max(90.0), input.center().y)
                .on_hover_text("add a block on a parallel branch\nor drag one down here");
            self.ghost_target = Some((at, hit.rect));
            if hit.clicked() {
                self.inserting_at = Some(at);
                self.insert_pos = Some(hit.rect.center_bottom() + egui::vec2(-260.0, 8.0));
                self.insert_opened = Some(std::time::Instant::now());
                self.browsing = None;
                self.search.clear();
            }
        });
    }

    /// Whether two positions sit in the same lane — the main line between the
    /// input and the output, or the same branch of the same split.
    fn same_lane(&self, a: usize, b: usize) -> bool {
        self.layout.paths.iter().any(|p| {
            let main = p.input.map_or(0, |i| i + 1)..p.output.unwrap_or(usize::MAX);
            if main.contains(&a) && main.contains(&b) {
                return true;
            }
            let Some(split) = p.split else { return false };
            let branch = split + 1..p.join.unwrap_or(usize::MAX);
            branch.contains(&a) && branch.contains(&b)
        })
    }

    /// Follow a block being dragged along the chain, resolved fresh from the
    /// pointer every frame — never from what was hovered some frames ago,
    /// which is how a release over nothing came to move a block anyway.
    ///
    /// A ghost of the block rides the pointer. Every gap shows a dot;
    /// dropping into the nearest one slides the block in there, marked by a
    /// bar filling the gap. Dropping onto a block in *another* lane trades
    /// places with it, marked by outlining that block — the lanes are not
    /// contiguous, so between them a move is a trade. Escape lets go.
    fn block_drag(&mut self, ui: &mut egui::Ui) {
        if self.display_only {
            return;
        }
        let Some(from) = self.dragging else {
            return;
        };
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dragging = None;
            return;
        }
        let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) else {
            return;
        };

        // A drop means one of three things: onto the offered branch to run
        // the block in parallel, onto a block in the other lane to trade
        // places with it, or into the nearest gap within reach to slide in.
        let ghost = self
            .ghost_target
            .filter(|(_, rect)| rect.expand(6.0).contains(pointer));
        let swap = if ghost.is_some() {
            None
        } else {
            self.block_rects
                .iter()
                .find(|(slot, rect)| {
                    *slot != from && rect.contains(pointer) && !self.same_lane(from, *slot)
                })
                .map(|(slot, rect)| (*slot, *rect))
        };
        let gap = if ghost.is_some() || swap.is_some() {
            None
        } else {
            let candidates = self
                .gap_rects
                .iter()
                // The gaps either side of the block itself go nowhere.
                .filter(|(before, _)| *before != from && *before != from + 1);
            // A gap under the pointer wins outright; otherwise the nearest
            // one within reach — half a row vertically, so the main line and
            // a branch never compete for a drop between them, and a column
            // horizontally, so a drop far from any gap means nothing.
            candidates
                .clone()
                .find(|(_, rect)| rect.contains(pointer))
                .or_else(|| {
                    candidates
                        .filter(|(_, rect)| {
                            (pointer.y - rect.center().y).abs() < theme::LANE_HEIGHT * 0.5
                                && (pointer.x - rect.center().x).abs() < theme::COLUMN
                        })
                        .min_by(|a, b| {
                            let da = (a.1.center().x - pointer.x).abs();
                            let db = (b.1.center().x - pointer.x).abs();
                            da.total_cmp(&db)
                        })
                })
                .map(|(before, rect)| (*before, *rect))
        };

        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        for (before, rect) in &self.gap_rects {
            if *before == from || *before == from + 1 {
                continue;
            }
            let hot = gap.is_some_and(|(g, _)| g == *before);
            if hot {
                theme::insert_marker(ui, *rect);
            } else {
                theme::attach_marker(ui, rect.center(), false);
            }
        }
        if let Some((_, rect)) = swap {
            theme::swap_marker(ui, rect);
        }
        if let Some((_, rect)) = ghost {
            // The branch lights the way the + does when it is the drop.
            theme::attach_marker(ui, egui::pos2(rect.center().x, rect.bottom() - 12.0), true);
        }
        if let Some(i) = self.index_of(from) {
            let block = &self.chain[i];
            theme::drag_ghost(
                ui.ctx(),
                pointer,
                &self.slot_label(block),
                self.block_colour(block),
            );
        }

        if ui.input(|i| i.pointer.any_released()) {
            if let Some((before, _)) = ghost {
                self.edit(Cmd::MoveBlockBefore { from, before });
            } else if let Some((slot, _)) = swap {
                self.edit(Cmd::MoveBlock { from, to: slot });
            } else if let Some((before, _)) = gap {
                self.edit(Cmd::MoveBlockBefore { from, before });
            }
            self.dragging = None;
        }
    }

    /// A gap you can add a block to, at `before`.
    ///
    /// One click opens the picker at the gap, and the model chosen there goes
    /// in here. Adding a pedal used to mean finding an empty slot and changing
    /// its model, which required knowing the slot topology — this puts the
    /// action where the pedal goes.
    fn insert_point(&mut self, ui: &mut egui::Ui, before: usize) {
        // In display mode the gap is just wire: same width, nothing to click.
        if self.display_only {
            theme::wire_run(ui, theme::WIRE_WIDTH, theme::BLOCK_HEIGHT);
            return;
        }
        let response =
            theme::insert_point(ui, theme::BLOCK_HEIGHT).on_hover_text("add a block here");
        self.gap_rects.push((before, response.rect));
        if response.clicked() {
            self.inserting_at = Some(before);
            // Anchored under the gap, so the choosing happens where the
            // pedal will go.
            self.insert_pos = Some(response.rect.center_bottom() + egui::vec2(-260.0, 6.0));
            self.insert_opened = Some(std::time::Instant::now());
            self.browsing = None;
            self.search.clear();
        }
    }

    /// The first free slot on a path's branch, if it can carry one.
    fn free_on_branch(&self, path: &hx_proto::preset::Path) -> Option<usize> {
        let split = path.split?;
        let join = path.join.unwrap_or(usize::MAX);
        // A slot between the split and the join that nothing occupies.
        (split + 1..join).find(|p| !self.chain.iter().any(|b| b.position == *p as i64))
    }

    /// One block in the line: clickable to edit, draggable to move.
    fn block_at(&mut self, ui: &mut egui::Ui, slot: usize) -> Option<usize> {
        let i = self.index_of(slot)?;
        let block = self.chain[i].clone();
        let art = self.artwork(&block);
        let colour = self.block_colour(&block);
        let hit = theme::block_button_tinted(
            ui,
            &self.slot_label(&block),
            art.as_ref(),
            i == self.selected,
            block.enabled,
            colour,
        );
        if !self.display_only {
            self.block_rects.push((slot, hit.rect));
            if hit.drag_started() {
                self.dragging = Some(slot);
            }
        }
        hit.clicked().then_some(i)
    }

    /// One lane of the divided stretch: a gap before every block, one after
    /// the last while the lane has room, and plain wire out to the merge so
    /// every lane ends where the branches meet.
    fn lane_row(
        &mut self,
        ui: &mut egui::Ui,
        lane: &hx_proto::preset::Lane,
        stretch: f32,
    ) -> Option<usize> {
        let mut pick = None;
        let mut used = 0.0;
        if lane.blocks.is_empty() && !lane.span.is_empty() {
            // An empty stretch is a plain wire, but it still takes a block.
            self.insert_point(ui, lane.span.start);
            used += theme::WIRE_WIDTH;
        }
        for slot in &lane.blocks {
            self.insert_point(ui, *slot);
            pick = self.block_at(ui, *slot).or(pick);
            used += theme::COLUMN;
        }
        if let Some(last) = lane.blocks.last() {
            if lane.blocks.len() < lane.span.len() {
                self.insert_point(ui, *last + 1);
                used += theme::WIRE_WIDTH;
            }
        }
        if stretch > used {
            theme::wire_run(ui, stretch - used, theme::BLOCK_HEIGHT);
        }
        pick
    }

    /// How much room a lane's blocks and gaps ask for; see [`Self::lane_row`].
    fn lane_width(&self, lane: &hx_proto::preset::Lane) -> f32 {
        if lane.blocks.is_empty() {
            return if lane.span.is_empty() {
                0.0
            } else {
                theme::WIRE_WIDTH
            };
        }
        let mut width = lane.blocks.len() as f32 * theme::COLUMN;
        if lane.blocks.len() < lane.span.len() {
            width += theme::WIRE_WIDTH;
        }
        width
    }

    /// An input or output tile. Not draggable: the endpoints are fixtures of
    /// the topology, not blocks to reorder.
    fn endpoint(&mut self, ui: &mut egui::Ui, slot: usize) -> (Option<usize>, egui::Rect) {
        let Some(i) = self.index_of(slot) else {
            return (None, ui.cursor());
        };
        let block = self.chain[i].clone();
        let art = self.artwork(&block);
        let colour = self.block_colour(&block);
        let hit = theme::block_button_tinted(
            ui,
            &self.slot_label(&block),
            art.as_ref(),
            i == self.selected,
            block.enabled,
            colour,
        );
        (hit.clicked().then_some(i), hit.rect)
    }

    /// The fork or merge itself, drawn as wiring: draggable along the line,
    /// clickable for its own parameters — a split's mode, a join's levels.
    fn junction(
        &mut self,
        ui: &mut egui::Ui,
        slot: usize,
        below: usize,
        opening: bool,
    ) -> (Option<usize>, egui::Rect) {
        let Some(i) = self.index_of(slot) else {
            return (None, ui.cursor());
        };
        let what = if opening {
            "the signal forks here\ndrag to move it, click for how it divides"
        } else {
            "the branches rejoin here\ndrag to move it, click for levels"
        };
        let label = self.slot_label(&self.chain[i]);
        let tag = if opening { split_tag(&label) } else { None };
        let held = self.dragging_junction == Some((slot, opening));
        let hit = theme::junction(ui, below, opening, i == self.selected || held, tag)
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(format!("{label}\n{what}"));
        if hit.drag_started() && !self.display_only {
            self.dragging_junction = Some((slot, opening));
        }
        (hit.clicked().then_some(i), hit.rect)
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

    /// The first-run welcome, as a modal over everything: how to give the
    /// pedals their names and faces.
    ///
    /// The model names, knob ranges, and artwork are Line 6's and cannot
    /// ship inside this app, so the one-time step is the user supplying HX
    /// Edit's own installer. It does not dismiss — without that data there
    /// are no names, no ranges, and no pictures, so there is nothing to
    /// edit with — and the moment extraction finishes it closes itself and
    /// the catalog loads in place: no restart.
    fn onboarding_modal(&mut self, ctx: &egui::Context) {
        if !self.show_onboarding {
            return;
        }
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("onboarding-modal"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // The veil: dims the app and swallows every click, so the
                // step in front is unmistakably the only step.
                let (veil, _) =
                    ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
                ui.painter()
                    .rect_filled(veil, 0.0, egui::Color32::from_black_alpha(170));

                let card = egui::Rect::from_center_size(
                    screen.center(),
                    egui::vec2(560.0_f32.min(screen.width() - 24.0), 560.0),
                );
                let mut card_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(card)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                egui::Frame::popup(&ctx.style())
                    .inner_margin(28.0)
                    .show(&mut card_ui, |ui| {
                        ui.set_width(card.width() - 56.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("🎸").size(30.0));
                            ui.add_space(2.0);
                            ui.heading("Welcome to stompchain");
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "Thanks for downloading! One quick step, and you only \
                                     ever do it once.",
                                )
                                .color(theme::ACCENT),
                            );
                            ui.add_space(14.0);
                            ui.label(
                                "stompchain needs HX Edit's data files: every model's name, \
                                 knob ranges, and artwork.",
                            );
                        });
                        ui.add_space(12.0);

                        // Skimmable, not a wall: one emoji-led line each.
                        let bullets = [
                            (
                                "⚖",
                                "They are Line 6's files, so they cannot ship in this app.",
                            ),
                            (
                                "💻",
                                "Either installer works: the Mac .dmg or the Windows .exe.",
                            ),
                            (
                                "🔒",
                                "Extraction happens here; nothing leaves your machine.",
                            ),
                        ];
                        let block = ((ui.available_width() - 440.0) / 2.0).max(0.0);
                        for (icon, line) in bullets {
                            ui.horizontal(|ui| {
                                ui.add_space(block);
                                ui.label(RichText::new(icon).size(16.0));
                                ui.label(RichText::new(line).color(theme::TEXT));
                            });
                            ui.add_space(4.0);
                        }
                        ui.add_space(12.0);

                        ui.vertical_centered(|ui| {
                            if ui
                                .button(RichText::new("Download HX Edit from line6.com").strong())
                                .on_hover_text("free, but a Line 6 account is required")
                                .clicked()
                            {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(
                                    "https://line6.com/software/",
                                ));
                            }
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("then, once it is downloaded:").color(theme::DIM),
                            );
                            ui.add_space(8.0);
                            let row = button_width(ui, "Check my Downloads folder")
                                + button_width(ui, "Browse…")
                                + ui.spacing().item_spacing.x;
                            ui.horizontal(|ui| {
                                center_row(ui, row, |ui| {
                                    if ui
                                        .button("Check my Downloads folder")
                                        .on_hover_text(
                                            "looks for an HX Edit installer you already downloaded",
                                        )
                                        .clicked()
                                    {
                                        match hx_catalog::extract::installer_in_downloads() {
                                            Some(installer) => self.extract_resources(installer),
                                            None => self.onboarding_status = Some(
                                                "no HX Edit installer in your Downloads folder yet"
                                                    .into(),
                                            ),
                                        }
                                    }
                                    if ui
                                        .button("Browse…")
                                        .on_hover_text("pick the installer wherever it landed")
                                        .clicked()
                                    {
                                        if let Some(installer) = rfd::FileDialog::new()
                                            .add_filter("HX Edit installer", &["dmg", "exe"])
                                            .pick_file()
                                        {
                                            self.extract_resources(installer);
                                        }
                                    }
                                });
                            });
                            // Wayland cannot deliver a file drop to this
                            // window (a windowing-library gap), so the hint
                            // only appears where dropping actually works.
                            if std::env::var_os("WAYLAND_DISPLAY").is_none() {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("…or drop the installer anywhere on this window")
                                        .small()
                                        .color(theme::DIM),
                                );
                            }

                            ui.add_space(10.0);
                            if self.extracting.is_some() {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    if let Some(status) = &self.onboarding_status {
                                        ui.label(RichText::new(status).color(theme::DIM));
                                    }
                                });
                            } else if let Some(status) = &self.onboarding_status {
                                ui.label(RichText::new(status).color(theme::ACCENT));
                            }
                        });

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(
                                    "Free and open source, MIT licensed. Not affiliated with \
                                     Yamaha Guitar Group.",
                                )
                                .small()
                                .color(theme::DIM),
                            );
                            ui.horizontal(|ui| {
                                center_row(ui, credits_width(ui), |ui| {
                                    ui.label(
                                        RichText::new("made with ♥ by").small().color(theme::DIM),
                                    );
                                    ui.hyperlink_to(
                                        RichText::new("Carmine Paolino").small(),
                                        "https://paolino.me",
                                    );
                                    ui.label(RichText::new("·").small().color(theme::DIM));
                                    ui.hyperlink_to(
                                        RichText::new("follow updates").small(),
                                        "https://x.com/paolino",
                                    );
                                    ui.label(RichText::new("·").small().color(theme::DIM));
                                    ui.hyperlink_to(
                                        RichText::new("♥ sponsor").small(),
                                        "https://github.com/sponsors/crmne",
                                    );
                                });
                            });
                        });
                    });
            });
    }

    /// The pedal's name and the things you do to the block itself.
    ///
    /// Wrapping rather than right-aligned: at a narrow window the right-to-left
    /// layout ran these buttons back across the block's own name.
    fn pedal_header(&mut self, ui: &mut egui::Ui, block: &session::Block) {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let colour = self.block_colour(block);
            theme::category_swatch(ui, colour);
            ui.heading(self.slot_label(block));

            if !self.is_effect(block) {
                return;
            }
            ui.add_space(12.0);

            let mut on = block.enabled;
            if ui.checkbox(&mut on, "Engaged").changed() {
                self.edit(Cmd::SetEnabled {
                    block: block.position,
                    enabled: on,
                });
                self.chain[self.selected].enabled = on;
            }
            if ui.button("Copy").on_hover_text("copy this block").clicked() {
                self.copied_block = Some(block.position as usize);
                self.note(format!("copied {}", self.slot_label(block)));
            }
            if ui
                .add_enabled(self.copied_block.is_some(), egui::Button::new("Paste"))
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
            if ui
                .button("Remove")
                .on_hover_text("take this block out of the chain")
                .clicked()
            {
                self.edit(Cmd::ClearBlock(block.position));
            }
        });
    }

    /// What drives this block's bypass.
    ///
    /// HX Edit's assignment page in miniature: bypass is a switch, so a
    /// footswitch or a MIDI CC can drive it but an expression pedal cannot —
    /// HX Edit lists pedals here and then steps over them, so they are simply
    /// not offered. Parameters take the full range of sources; see the knob
    /// context menus.
    fn bypass_assignment(&mut self, ui: &mut egui::Ui, block: &session::Block) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Bypass switched by")
                    .small()
                    .color(theme::DIM),
            );

            for switch in 1..=5u8 {
                if ui
                    .button(format!("FS{switch}"))
                    .on_hover_text(format!("footswitch {switch} toggles this block"))
                    .clicked()
                {
                    self.edit(Cmd::AssignBypassFootswitch {
                        block: block.position,
                        switch,
                        on: true,
                    });
                }
            }
            ui.separator();

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
            if ui.button("Assign").clicked() {
                self.edit(Cmd::AssignCc {
                    block: block.position,
                    cc: self.assign_cc,
                });
            }
            ui.separator();
            if ui
                .button("Clear")
                .on_hover_text("take the bypass off every footswitch")
                .clicked()
            {
                for switch in 1..=5u8 {
                    self.edit(Cmd::AssignBypassFootswitch {
                        block: block.position,
                        switch,
                        on: false,
                    });
                }
            }
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
    ///
    /// Effects only. An endpoint reports model 0, which is a real amp in the
    /// symbol table, so resolving it painted the input and output in the amp
    /// category's red.
    fn block_colour(&self, block: &session::Block) -> egui::Color32 {
        let fallback = theme::DIM;
        if block.kind != hx_proto::preset::Kind::Block || block.model == 0 {
            return fallback;
        }
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

    /// The shelf: swap the selected block for another.
    ///
    /// Swapping only. Adding is done at the gap it goes into — see
    /// [`Self::insert_picker`] — because choosing a pedal in a panel on the
    /// far side of the window, after arming a mode there, was a lot of
    /// ceremony for "put a delay here".
    fn shelf(&mut self, ctx: &egui::Context) {
        let Some(block) = self.chain.get(self.selected).cloned() else {
            return;
        };
        // On a preset with nothing in it there is no block to swap, but the
        // obvious thing to want is a pedal — so the shelf adds instead.
        let empty = !self.chain.iter().any(|b| self.is_effect(b));
        if !(self.is_effect(&block) || empty) {
            return;
        }

        let heading = if empty { "ADD A BLOCK" } else { "SWAP FOR" };
        let current = self
            .catalog
            .as_ref()
            .and_then(|c| c.model_number(block.model))
            .map(|m| m.id.clone());

        let mut picked = None;
        egui::SidePanel::right("shelf")
            .default_width(430.0)
            .width_range(200.0..=620.0)
            .show(ctx, |ui| {
                let App {
                    catalog,
                    search,
                    browsing,
                    ..
                } = self;
                let Some(catalog) = catalog.as_ref() else {
                    return;
                };
                ui.add_space(6.0);
                picked = model_picker(
                    ui,
                    catalog,
                    search,
                    browsing,
                    current.as_deref(),
                    heading,
                    false,
                );
            });

        if let Some(model) = picked {
            if empty {
                // The first slot the signal reaches that is free.
                let at = self
                    .layout
                    .paths
                    .first()
                    .and_then(|p| p.input)
                    .map(|i| i + 1)
                    .unwrap_or(1);
                self.edit(Cmd::InsertBlock { at, model });
            } else {
                self.edit(Cmd::SetModel {
                    block: block.position,
                    model,
                });
            }
        }
    }

    /// Choose a pedal for the gap you clicked, where you clicked it.
    ///
    /// Opens focused with the search field live, so the fastest way to add a
    /// delay is to click the gap and type "del". Escape closes it. Everything
    /// happens in one place: the previous flow put a menu on the gap, a mode
    /// on a panel across the window, and the actual choosing a third place
    /// again, which is why it never felt like it worked.
    fn insert_picker(&mut self, ctx: &egui::Context) {
        let (Some(at), Some(pos)) = (self.inserting_at, self.insert_pos) else {
            return;
        };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_picker();
            return;
        }

        let mut picked = None;
        let area = egui::Area::new(egui::Id::new("insert-picker"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::popup(&ctx.style())
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.set_width(520.0);
                        ui.set_height(430.0);
                        let App {
                            catalog,
                            search,
                            browsing,
                            ..
                        } = self;
                        let Some(catalog) = catalog.as_ref() else {
                            return;
                        };
                        picked =
                            model_picker(ui, catalog, search, browsing, None, "ADD A BLOCK", true);
                    });
            });

        // Clicking anywhere else means "not that after all" — but not the
        // click that opened it, which egui still reports this frame, and which
        // landed on the gap rather than inside the popup. A moment's grace is
        // more reliable than a frame counter here, because egui may run
        // several passes for one frame.
        let settled = self
            .insert_opened
            .is_some_and(|t| t.elapsed() > Duration::from_millis(250));
        let outside = ctx.input(|i| {
            i.pointer.any_click()
                && !i
                    .pointer
                    .interact_pos()
                    .is_some_and(|p| area.response.rect.contains(p))
        });
        if settled && outside {
            self.close_picker();
            return;
        }
        if let Some(model) = picked {
            self.close_picker();
            self.edit(Cmd::InsertBlock { at, model });
        }
    }

    fn close_picker(&mut self) {
        self.inserting_at = None;
        self.insert_pos = None;
        self.insert_opened = None;
        self.search.clear();
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

    /// How a split divides the signal, as a row of chips — the defining
    /// choice for the block, in the same place an endpoint offers its
    /// routing. Returns the model number of a newly chosen type.
    ///
    /// Changing type is an ordinary model change on the split's slot; the
    /// attach points and the branch survive it (verified on hardware), the
    /// knobs below re-render for the new type, and undo steps back through it.
    fn split_type_menu(&self, ui: &mut egui::Ui, position: i64) -> Option<u32> {
        let block = self.chain.iter().find(|b| b.position == position)?;
        if block.kind != hx_proto::preset::Kind::Split {
            return None;
        }
        let catalog = self.catalog.as_ref()?;
        let current = catalog.model_number(block.model)?.id.clone();
        let family = catalog.models_in(catalog.category_of(&current)?);

        let mut picked = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Type").small().color(theme::DIM));
            for model in family {
                let name = model.name.strip_prefix("Split ").unwrap_or(&model.name);
                let on = model.id == current;
                let chip = theme::category_chip(ui, name, theme::ACCENT, on)
                    .on_hover_text(split_type_hint(&model.name));
                if chip.clicked() && !on {
                    // Only models the firmware knows by number can be sent.
                    picked = catalog
                        .symbols()
                        .iter()
                        .find(|s| s.model.as_deref() == Some(model.id.as_str()))
                        .map(|s| s.number);
                }
            }
        });
        ui.add_space(4.0);
        picked
    }

    /// The selected block drawn as a pedal: its artwork, then its controls as
    /// knobs beneath, the way Logic's Pedalboard and the hardware itself do.
    /// Used for both halves of an Amp+Cab block, so the model is passed in
    /// rather than read off the block.
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
        let mut assign: Option<(i64, hx_proto::rpc::Source)> = None;
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
        let retype = self.split_type_menu(ui, position);

        // Values arrive in the order the device indexes them, which the catalog
        // knows how to reproduce — it is not simply the model's parameter list.
        // An input's list starts with `@input`, which carries no value, and
        // using it directly shifted every knob by one.
        let params = catalog.ordered_params(model);

        // Knobs sit in rows under the pedal like the face of one, every row
        // starting at the same left edge so the columns line up — a wrapped
        // row that started at the margin made twelve knobs look scattered.
        let cell = egui::vec2(84.0, 100.0);
        let pitch = cell.x + ui.spacing().item_spacing.x;
        let columns = ((ui.available_width() / pitch).floor() as usize)
            .clamp(1, 8)
            .min(values.len().max(1));
        let indent = ((ui.available_width() - columns as f32 * pitch) / 2.0).max(0.0);
        let draft = self.param_draft.clone();
        let mut set_draft: Option<Option<(i64, i64, String)>> = None;
        for row in values
            .iter()
            .enumerate()
            .collect::<Vec<_>>()
            .chunks(columns)
        {
            ui.horizontal(|ui| {
                ui.add_space(indent);
                for (index, value) in row {
                    let index = *index;
                    let Some(param) = params.get(index).copied() else {
                        continue;
                    };
                    let mut current = **value;

                    ui.allocate_ui(cell, |ui| {
                        ui.vertical_centered(|ui| {
                            let mut changed = false;
                            match param.kind {
                                Kind::Switch => {
                                    let mut on = current >= 0.5;
                                    changed = ui.add(theme::switch(&mut on)).changed();
                                    current = on as u8 as f32;
                                    ui.label(
                                        RichText::new(catalog.format(param, current))
                                            .monospace()
                                            .color(theme::ACCENT),
                                    );
                                }
                                // A menu is a menu: a knob that snaps between
                                // named choices only pretended, arc and all.
                                Kind::Enum => {
                                    let choice = current.round().max(0.0) as usize;
                                    ui.add_space(11.0);
                                    egui::ComboBox::from_id_salt((
                                        "param", position, index, paired,
                                    ))
                                    .width(cell.x)
                                    .selected_text(
                                        RichText::new(catalog.format(param, current))
                                            .monospace()
                                            .color(theme::ACCENT),
                                    )
                                    .show_ui(ui, |ui| {
                                        if let Some(choices) = catalog.choices(param) {
                                            for (n, label) in choices.iter().enumerate() {
                                                if ui
                                                    .selectable_label(n == choice, label)
                                                    .clicked()
                                                    && n != choice
                                                {
                                                    current = n as f32;
                                                    changed = true;
                                                }
                                            }
                                        }
                                    });
                                    ui.add_space(11.0);
                                }
                                _ => {
                                    let hit =
                                        theme::knob(ui, &mut current, param.min..=param.max);
                                    changed = hit.changed();
                                    // Double-click puts the factory default
                                    // back, the way every DAW knob does.
                                    if hit.double_clicked() {
                                        current = param.default;
                                        changed = true;
                                    }
                                    hit.on_hover_text(
                                        "drag to turn; double-click to reset\nclick the value to type it",
                                    );
                                    match &draft {
                                        Some((block, i, text))
                                            if *block == position && *i == index as i64 =>
                                        {
                                            let mut text = text.clone();
                                            let field = ui.add(
                                                egui::TextEdit::singleline(&mut text)
                                                    .desired_width(64.0)
                                                    .font(egui::TextStyle::Monospace),
                                            );
                                            if !field.has_focus() && !field.lost_focus() {
                                                field.request_focus();
                                            }
                                            if field.lost_focus() {
                                                if ui.input(|inp| {
                                                    inp.key_pressed(egui::Key::Enter)
                                                }) {
                                                    if let Some(typed) =
                                                        catalog.parse(param, &text)
                                                    {
                                                        current = typed
                                                            .clamp(param.min, param.max);
                                                        changed = true;
                                                    }
                                                }
                                                set_draft = Some(None);
                                            } else {
                                                set_draft =
                                                    Some(Some((position, index as i64, text)));
                                            }
                                        }
                                        _ => {
                                            let shown = ui.add(
                                                egui::Label::new(
                                                    RichText::new(
                                                        catalog.format(param, current),
                                                    )
                                                    .monospace()
                                                    .color(theme::ACCENT),
                                                )
                                                .sense(egui::Sense::click()),
                                            );
                                            if shown
                                                .on_hover_text("click to type a value")
                                                .clicked()
                                            {
                                                set_draft = Some(Some((
                                                    position,
                                                    index as i64,
                                                    catalog.format(param, current),
                                                )));
                                            }
                                        }
                                    }
                                }
                            }
                            let name = ui.label(RichText::new(&param.name).color(theme::DIM));
                            // Right-click to put the knob under a pedal or
                            // switch, which is where you are already looking
                            // when you decide you want to sweep it with your
                            // foot.
                            name.context_menu(|ui| {
                                ui.label(
                                    RichText::new(format!("Control {} with", param.name))
                                        .small()
                                        .color(theme::DIM),
                                );
                                for source in hx_proto::rpc::Source::all() {
                                    if ui.button(source.label()).clicked() {
                                        assign = Some((index as i64, source));
                                        ui.close_menu();
                                    }
                                }
                            });
                            if changed {
                                edit = Some((index as i64, current, param.kind == Kind::Switch));
                            }
                        });
                    });
                }
            });
        }
        if let Some(update) = set_draft {
            self.param_draft = update;
        }

        if let Some((param, source)) = assign {
            self.edit(Cmd::AssignParameter {
                block: position,
                param,
                source,
            });
        }
        if let Some(to) = reroute {
            self.edit(Cmd::SetRouting {
                block: position,
                to,
            });
        }
        if let Some(model) = retype {
            self.edit(Cmd::SetModel {
                block: position,
                model,
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

/// Centre a fixed-width row of widgets inside a horizontal `ui`.
///
/// egui lays a row out left to right with no idea how wide its content will
/// end up, so centring means telling it: pad by half of what is left over.
fn center_row(ui: &mut egui::Ui, content_width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(((ui.available_width() - content_width) / 2.0).max(0.0));
    add(ui);
}

/// What a button with this label will measure, for centring rows of them.
fn button_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, theme::TEXT));
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

/// The measured width of the credits line, so it can be centred exactly.
fn credits_width(ui: &egui::Ui) -> f32 {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let pieces = [
        "made with ♥ by",
        "Carmine Paolino",
        "·",
        "follow updates",
        "·",
        "♥ sponsor",
    ];
    let text: f32 = ui.fonts(|fonts| {
        pieces
            .iter()
            .map(|piece| {
                fonts
                    .layout_no_wrap((*piece).to_owned(), font.clone(), theme::DIM)
                    .size()
                    .x
            })
            .sum()
    });
    text + ui.spacing().item_spacing.x * (pieces.len() - 1) as f32
}

/// One line on what a split type does with the signal, for its chip's hover.
fn split_type_hint(name: &str) -> &'static str {
    match name {
        "Split Y" => "the signal runs down both branches",
        "Split A/B" => "the signal takes one branch at a time",
        "Split Crossover" => "splits the signal by frequency",
        "Split Dynamic" => "splits the signal by playing level",
        _ => "how the signal divides at the fork",
    }
}

/// The tag worn by a fork in the chain, for types that change how the preset
/// behaves. The default Y is silent — a tag is for the deviations worth
/// noticing at a glance.
fn split_tag(name: &str) -> Option<&'static str> {
    match name {
        "Split A/B" => Some("A/B"),
        "Split Crossover" => Some("XO"),
        "Split Dynamic" => Some("DYN"),
        _ => None,
    }
}

/// Where a dragged fork or merge may go: the lowest and highest slot it can
/// attach before, and where it is attached now.
///
/// A fork ranges from just after the input to the merge; a merge, from the
/// fork to the output. The ends may meet — a stretch of zero width is how the
/// device itself represents a branch that parallels nothing.
fn attach_range(path: &hx_proto::preset::Path, opening: bool) -> Option<(usize, usize, usize)> {
    // The stretch the lanes span *is* the pair of attach points.
    let span = path.lanes.first().map(|l| l.span.clone())?;
    Some(if opening {
        (path.input.map_or(0, |i| i + 1), span.end, span.start)
    } else {
        (span.start, path.output.unwrap_or(span.end), span.end)
    })
}

/// Search, categories and a grid of pedals. Returns the model chosen.
///
/// A free function taking the pieces it needs rather than `&mut self`, so the
/// same widget serves the swap shelf and the insert popup — the two places you
/// choose a pedal should not look or behave differently.
fn model_picker(
    ui: &mut egui::Ui,
    catalog: &hx_catalog::Catalog,
    search: &mut String,
    browsing: &mut Option<u32>,
    current: Option<&str>,
    heading: &str,
    focus_search: bool,
) -> Option<u32> {
    let mut picked = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new(heading).small().color(theme::DIM));
        let field = ui.add(
            egui::TextEdit::singleline(search)
                .hint_text("Search pedals")
                .desired_width(f32::INFINITY),
        );
        // Typing is the fastest way to find one of several hundred, so the
        // popup opens ready for it.
        if focus_search && !field.has_focus() {
            field.request_focus();
        }
    });
    ui.add_space(4.0);

    let searching = !search.is_empty();
    let showing = browsing.unwrap_or(1);
    ui.horizontal_wrapped(|ui| {
        for category in catalog.categories() {
            if !category.is_effect() || catalog.models_in(category.id).is_empty() {
                continue;
            }
            let colour = theme::category_colour(category.colour);
            let on = !searching && category.id == showing;
            if theme::category_chip(ui, &category.name, colour, on).clicked() {
                *browsing = Some(category.id);
                search.clear();
            }
        }
    });
    ui.separator();

    let models: Vec<&hx_catalog::Model> = if searching {
        let needle = search.to_lowercase();
        catalog
            .models()
            .filter(|m| m.name.to_lowercase().contains(&needle))
            .filter(|m| {
                catalog
                    .category_of(&m.id)
                    .and_then(|c| catalog.category(c))
                    .is_some_and(|c| c.is_effect())
            })
            .collect()
    } else {
        catalog.models_in(showing)
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if models.is_empty() {
                ui.label(RichText::new("Nothing matches").color(theme::DIM));
            }
            ui.horizontal_wrapped(|ui| {
                for model in models {
                    let selected = current == Some(model.id.as_str());
                    let art = catalog
                        .artwork(model)
                        .map(|p| theme::Art::whole(format!("file://{}", p.display())));
                    if theme::model_tile(ui, &model.name, art.as_ref(), selected).clicked() {
                        // Only models the firmware knows by number can be sent.
                        picked = catalog
                            .symbols()
                            .iter()
                            .find(|s| s.model.as_deref() == Some(model.id.as_str()))
                            .map(|s| s.number);
                    }
                }
            });
        });

    picked
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
                dirty: false,
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

    /// The fork wears a tag only when its type changes the preset's
    /// behaviour; the default Y stays quiet.
    #[test]
    fn only_the_notable_split_types_wear_a_tag() {
        assert_eq!(split_tag("Split Y"), None);
        assert_eq!(split_tag("Split A/B"), Some("A/B"));
        assert_eq!(split_tag("Split Crossover"), Some("XO"));
        assert_eq!(split_tag("Split Dynamic"), Some("DYN"));
        assert_eq!(split_tag("Mixer"), None, "a merge has no type to announce");
    }

    /// The lookups the type chips run: the split's category holds the whole
    /// family, and every member resolves to a number the firmware accepts.
    /// Needs HX Edit's resources; skips quietly where they are not installed.
    #[test]
    fn the_split_family_resolves_through_the_catalog() {
        let Ok(catalog) = Catalog::load() else {
            return;
        };
        let split_y = catalog.model_number(257).expect("Split Y is model 257");
        let family = catalog.models_in(catalog.category_of(&split_y.id).expect("a category"));
        let names: Vec<&str> = family.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["Split Y", "Split A/B", "Split Crossover", "Split Dynamic"],
            "the family, in the order the chips show"
        );
        for model in family {
            assert!(
                catalog
                    .symbols()
                    .iter()
                    .any(|s| s.model.as_deref() == Some(model.id.as_str())),
                "{} resolves to a firmware number",
                model.name
            );
        }
    }

    /// A fork may travel between the input and the merge, a merge between the
    /// fork and the output — and they may meet, because a zero-width stretch
    /// is how the device represents a branch that parallels nothing.
    #[test]
    fn a_dragged_junction_stays_between_its_neighbours() {
        use hx_proto::preset::{Lane, Path};
        let path = Path {
            input: Some(0),
            output: Some(9),
            split: Some(10),
            join: Some(19),
            head: vec![1],
            lanes: vec![
                Lane {
                    branch: 0,
                    blocks: vec![2, 3],
                    span: 2..4,
                },
                Lane {
                    branch: 1,
                    blocks: vec![11],
                    span: 11..19,
                },
            ],
            tail: vec![4],
        };

        assert_eq!(
            attach_range(&path, true),
            Some((1, 4, 2)),
            "the fork ranges from after the input to the merge"
        );
        assert_eq!(
            attach_range(&path, false),
            Some((2, 9, 4)),
            "the merge ranges from the fork to the output"
        );

        let straight = Path {
            head: vec![1],
            ..Path::default()
        };
        assert_eq!(attach_range(&straight, true), None, "no lanes, no drag");
    }

    /// An app holding a branched chain — a drive on the main line, a delay on
    /// the branch — for tests that drive the chain panel with a pointer.
    fn branched_app() -> (App, mpsc::Receiver<Cmd>) {
        use hx_proto::preset::{Kind, Lane, Layout, Path};
        let (mut app, events, cmds) = app();

        let slot = |position: i64, kind| session::Block {
            position,
            routing: None,
            kind,
            model: 0,
            enabled: true,
            values: vec![],
            paired: None,
            paired_values: vec![],
        };
        events
            .send(Evt::Loaded {
                index: 0,
                name: "Test".into(),
                firmware: String::new(),
                tempo: None,
                snapshots: vec![],
                chain: vec![
                    slot(0, Kind::Input),
                    slot(1, Kind::Block),
                    slot(9, Kind::Output),
                    slot(10, Kind::Split),
                    slot(11, Kind::Block),
                    slot(19, Kind::Join),
                ],
                layout: Layout {
                    paths: vec![Path {
                        input: Some(0),
                        output: Some(9),
                        split: Some(10),
                        join: Some(19),
                        head: vec![],
                        lanes: vec![
                            Lane {
                                branch: 0,
                                blocks: vec![1],
                                span: 1..9,
                            },
                            Lane {
                                branch: 1,
                                blocks: vec![11],
                                span: 11..19,
                            },
                        ],
                        tail: vec![],
                    }],
                },
                dirty: true,
            })
            .unwrap();
        app.drain_events();
        (app, cmds)
    }

    /// Draw the chain once so the frame's gap and block rects are recorded,
    /// then release the pointer at `at` and draw again.
    fn draw_then_release_at(app: &mut App, at: impl FnOnce(&App) -> egui::Pos2) -> egui::Context {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 600.0));
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input.clone(), |ctx| app.signal_chain(ctx));
        let pos = at(app);
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(input, |ctx| app.signal_chain(ctx));
        ctx
    }

    fn gap_centre(app: &App, before: usize) -> egui::Pos2 {
        app.gap_rects
            .iter()
            .find(|(b, _)| *b == before)
            .map(|(_, rect)| rect.center())
            .unwrap_or_else(|| panic!("no gap before slot {before}"))
    }

    /// The whole drag, without a screen: draw the chain once so the gaps are
    /// known, put the merge in hand, release the pointer over the gap before
    /// the drive — and the worker must be asked to re-attach the join there.
    #[test]
    fn releasing_a_dragged_merge_reattaches_it_at_the_nearest_gap() {
        let (mut app, cmds) = branched_app();
        app.dragging_junction = Some((19, false));
        draw_then_release_at(&mut app, |app| gap_centre(app, 1));

        assert!(app.dragging_junction.is_none(), "the drag ended");
        assert!(
            cmds.try_iter().any(|c| matches!(
                c,
                Cmd::MoveJunction {
                    junction: 19,
                    before: 1
                }
            )),
            "the worker was asked to re-attach the join before slot 1"
        );
    }

    /// Dropping a block into a gap slides it in there — the branch's delay
    /// released over the main line's leading gap moves before the drive.
    #[test]
    fn dropping_a_block_into_a_gap_moves_it_before_that_slot() {
        let (mut app, cmds) = branched_app();
        app.dragging = Some(11);
        draw_then_release_at(&mut app, |app| gap_centre(app, 1));

        assert!(app.dragging.is_none(), "the drag ended");
        assert!(
            cmds.try_iter().any(|c| matches!(
                c,
                Cmd::MoveBlockBefore {
                    from: 11,
                    before: 1
                }
            )),
            "the delay was asked into the gap before the drive"
        );
    }

    /// Dropping a block onto a block in the other lane trades their places.
    #[test]
    fn dropping_a_block_onto_the_other_lane_trades_places() {
        let (mut app, cmds) = branched_app();
        app.dragging = Some(1);
        draw_then_release_at(&mut app, |app| {
            app.block_rects
                .iter()
                .find(|(slot, _)| *slot == 11)
                .map(|(_, rect)| rect.center())
                .expect("the branch block was drawn")
        });

        assert!(
            cmds.try_iter()
                .any(|c| matches!(c, Cmd::MoveBlock { from: 1, to: 11 })),
            "the two blocks were asked to trade places"
        );
    }

    /// Dragging a block down onto the offered branch moves it there — the
    /// gesture HX Edit taught everyone for "run this one in parallel".
    #[test]
    fn dropping_a_block_onto_the_offered_branch_moves_it_there() {
        use hx_proto::preset::{Kind, Layout, Path};
        let (mut app, events, cmds) = app();
        let slot = |position: i64, kind| session::Block {
            position,
            routing: None,
            kind,
            model: 0,
            enabled: true,
            values: vec![],
            paired: None,
            paired_values: vec![],
        };
        events
            .send(Evt::Loaded {
                index: 0,
                name: "Test".into(),
                firmware: String::new(),
                tempo: None,
                snapshots: vec![],
                chain: vec![
                    slot(0, Kind::Input),
                    slot(1, Kind::Block),
                    slot(2, Kind::Block),
                    slot(9, Kind::Output),
                    slot(10, Kind::Split),
                    slot(19, Kind::Join),
                ],
                layout: Layout {
                    paths: vec![Path {
                        input: Some(0),
                        output: Some(9),
                        split: Some(10),
                        join: Some(19),
                        head: vec![1, 2],
                        lanes: vec![],
                        tail: vec![],
                    }],
                },
                dirty: true,
            })
            .unwrap();
        app.drain_events();

        app.dragging = Some(2);
        draw_then_release_at(&mut app, |app| {
            app.ghost_target
                .map(|(_, rect)| rect.center())
                .expect("a branch was on offer")
        });

        assert!(
            cmds.try_iter().any(|c| matches!(
                c,
                Cmd::MoveBlockBefore {
                    from: 2,
                    before: 11
                }
            )),
            "the block was asked onto the branch's first free slot"
        );
    }

    /// A release over nothing lets go without moving anything — the drop is
    /// resolved from where the pointer is, never from what it once crossed.
    #[test]
    fn releasing_over_nothing_moves_nothing() {
        let (mut app, cmds) = branched_app();
        app.dragging = Some(1);
        draw_then_release_at(&mut app, |_| egui::pos2(1200.0, 550.0));

        assert!(app.dragging.is_none(), "the drag ended");
        assert!(
            !cmds
                .try_iter()
                .any(|c| matches!(c, Cmd::MoveBlock { .. } | Cmd::MoveBlockBefore { .. })),
            "nothing moved"
        );
    }

    /// A reload that follows an edit must keep Save available: the worker says
    /// whether the buffer is dirty, and the app takes its word rather than
    /// assuming a load means a fresh preset.
    #[test]
    fn a_reload_after_an_edit_keeps_the_unsaved_changes_flag() {
        let (mut app, events, _cmds) = app();
        let loaded = |dirty| Evt::Loaded {
            index: 7,
            name: "CT-Sad".into(),
            firmware: "3.80".into(),
            tempo: None,
            snapshots: vec![],
            layout: hx_proto::preset::Layout::default(),
            chain: vec![],
            dirty,
        };

        events.send(loaded(true)).unwrap();
        app.drain_events();
        assert!(
            app.dirty,
            "an edit-triggered reload still has changes to save"
        );

        events.send(loaded(false)).unwrap();
        app.drain_events();
        assert!(!app.dirty, "a fresh load has nothing to save");
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

    /// Opening a .hlx lands in the preview - drawn, not loaded - and sends
    /// nothing to the device until Load is pressed.
    #[test]
    fn a_tone_file_opens_as_a_preview_not_a_write() {
        let (mut app, _events, cmds) = app();
        if app.catalog.is_none() {
            eprintln!("SKIPPED: HX Edit is not installed, so tones cannot be read");
            return;
        }
        let json = serde_json::json!({
            "data": { "meta": { "name": "Looked At" }, "tone": { "dsp0": {
                "block0": { "@model": "HD2_DistScream808Mono", "@enabled": true }
            }}}
        });
        let file = std::env::temp_dir().join("stompchain-preview-test.hlx");
        std::fs::write(&file, json.to_string()).unwrap();

        app.open_tone_file(&file);
        let preview = app.preview.as_ref().expect("a preview should open");
        assert_eq!(preview.name, "Looked At");
        // Drawn as a real chain: the block plus its input and output.
        assert_eq!(preview.chain.len(), 3);
        assert_eq!(preview.layout.paths.len(), 1);
        assert!(
            matches!(&preview.load, LoadKind::Steps(blocks) if blocks.len() == 1),
            "a .hlx loads by rebuilding its blocks"
        );
        assert!(
            !cmds
                .try_iter()
                .any(|c| matches!(c, Cmd::PastePreset(_) | Cmd::LoadSteps { .. })),
            "looking at a tone must not write it"
        );
        let _ = std::fs::remove_file(&file);
    }

    /// Importing a file that is not a preset must not reach the device: it
    /// would be accepted and then read back as an empty slot.
    #[test]
    fn importing_a_missing_file_reports_instead_of_sending() {
        let (mut app, _events, cmds) = app();
        app.open_tone_file(std::path::Path::new("/nonexistent/nope.hxpreset"));

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
