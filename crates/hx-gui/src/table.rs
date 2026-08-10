//! The library's table, and the only one in the app.
//!
//! Both halves of the library are drawn by this: tones and setlists differ in
//! their columns and in nothing else, which is the point. They are one library
//! with two views of itself, and two tables that behaved differently would say
//! otherwise.
//!
//! Rows are virtualised, so a library of a few thousand tones lays out only the
//! twenty you can see, and the first columns are sticky, so scrolling sideways
//! never takes the name away from the row it belongs to. That is what
//! `egui_extras::TableBuilder` could not do: it builds every row every frame.
//!
//! The cells are prepared as plain values before the table draws. Reading them
//! costs a few short strings per row and it buys the whole thing being a value:
//! sorting, selection and editing all happen on data rather than inside a
//! drawing closure, where borrowing the app twice is a fight.

use egui::{RichText, Ui};

use crate::theme;

/// How far a cell's contents sit from its column's left edge.
const PADDING: f32 = 8.0;

/// How tall one row is, and the header with it. Public because a caller that
/// has to bound the table's height has to know what a row costs.
pub const ROW_HEIGHT: f32 = 22.0;

/// What a cell holds.
pub enum Cell {
    /// Ordinary text.
    Text(String),
    /// Text in the second voice: derived, not typed, and not editable.
    Dim(String),
    /// Where a tone is: one icon per place, each its own button.
    Places(Vec<(theme::Icon, theme::Sync, &'static str)>),
    /// A number, drawn as the pedal draws one. Same widget, same gestures:
    /// drag to turn, click the reading to type it. A row of these needs a
    /// taller row than a row of words, which is what `Grid::row_height` is for.
    Knob {
        value: f32,
        range: std::ops::RangeInclusive<f32>,
        /// The reading, formatted the way that parameter is formatted
        /// everywhere else.
        text: String,
    },
}

impl Cell {
    /// What this cell sorts on. A dot sorts by its state, so clicking that
    /// header gathers everything that needs doing.
    pub fn key(&self) -> String {
        match self {
            Cell::Text(t) | Cell::Dim(t) => t.to_lowercase(),
            // Padded so 9 sorts before 10, which a plain string does not.
            Cell::Knob { value, .. } => format!("{value:020.6}"),
            // Sorted so everything with something to do gathers at the top.
            Cell::Places(places) => places
                .iter()
                .map(|(_, state, _)| match state {
                    theme::Sync::Differs => '0',
                    theme::Sync::Absent => '1',
                    theme::Sync::Same => '2',
                    theme::Sync::Unknown => '3',
                })
                .collect(),
        }
    }
}

/// A column: what it is called, how wide, and whether its cells can be typed
/// into.
pub struct Column {
    pub title: &'static str,
    pub width: f32,
    pub editable: bool,
    /// Fills whatever is left. At most one column should say yes.
    pub fills: bool,
}

impl Column {
    pub fn new(title: &'static str, width: f32) -> Column {
        Column {
            title,
            width,
            editable: false,
            fills: false,
        }
    }

    pub fn editable(mut self) -> Column {
        self.editable = true;
        self
    }

    pub fn fills(mut self) -> Column {
        self.fills = true;
        self
    }
}

/// Everything the table needs to draw, prepared.
#[derive(Default)]
pub struct Grid {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    /// Which rows are picked out, by row index into `rows`.
    pub chosen: Vec<bool>,
    /// The one row whose details the inspector is showing.
    pub selected: Option<usize>,
    /// Which column orders the rows, and which way. Only used for the arrow;
    /// the caller has already sorted.
    pub sort: (usize, bool),
    /// How many columns stay put when the table is scrolled sideways.
    pub sticky: usize,
    /// The cell being typed into, and what has been typed so far.
    pub editing: Option<(usize, usize)>,
    pub draft: String,
    /// What a right-click offers, in order. Data rather than a closure, so the
    /// menu can be drawn inside the cell where egui wants it without the table
    /// having to borrow the app that owns the actions.
    pub menu: Vec<String>,
    /// Said instead of an empty grid of rows. The headers still show, because a
    /// table with no rows should still say what it would hold.
    pub nothing_yet: &'static str,
    /// How tall a row is. Zero means [`ROW_HEIGHT`], which is a line of text; a
    /// table with knobs in it needs the room a knob takes.
    pub row_height: f32,
}

/// What a person did to the table this frame.
#[derive(Default)]
pub struct Did {
    pub clicked: Option<(usize, bool, bool)>,
    pub double_clicked: Option<usize>,
    /// Which row's place icon was pressed, and which of them.
    pub place: Option<(usize, usize)>,
    /// Start typing in this cell.
    pub edit: Option<(usize, usize)>,
    /// Finish typing: keep it, or throw it away.
    pub committed: bool,
    pub cancelled: bool,
    pub sort: Option<usize>,
    /// A right-click, and which item of the menu it ended on.
    pub context: Option<usize>,
    pub chose: Option<(usize, usize)>,
    /// A knob cell was turned: which cell, and what it now reads.
    pub turned: Option<(usize, usize, f32)>,
}

/// Draw it, and answer with what happened.
pub fn show(ui: &mut Ui, id: &str, grid: &mut Grid) -> Did {
    if grid.rows.is_empty() && !grid.nothing_yet.is_empty() {
        // Headers first, then the sentence under them: an empty table that
        // shows nothing at all looks broken, and one that shows only a sentence
        // does not say what it is for.
        draw_headers(ui, grid);
        ui.add_space(10.0);
        ui.label(RichText::new(grid.nothing_yet).color(theme::DIM));
        return Did::default();
    }

    let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
    let available = ui.available_width();
    // Every column is widened by its own padding below, so the filling column
    // has to give that room back for every one of them - including its own.
    // Counting only the fixed widths made the columns add up to wider than the
    // table and put a scrollbar under a table that fitted.
    let fixed: f32 = grid
        .columns
        .iter()
        .filter(|c| !c.fills)
        .map(|c| c.width)
        .sum::<f32>()
        + PADDING * grid.columns.len() as f32;
    let columns: Vec<egui_table::Column> = grid
        .columns
        .iter()
        .map(|c| {
            // The margin is for the scrollbar the table draws down its right,
            // which takes width the columns cannot have.
            let width = if c.fills {
                (available - fixed - 12.0).max(90.0)
            } else {
                c.width
            };
            // The width asked for is a floor, not a suggestion. egui_table
            // sizes a column to its widest cell on the first frame, and a cell
            // that truncates rather than wraps reports almost no width at all,
            // so left to itself every column collapses to a few pixels and the
            // headers read "Nar Cha Ger". A floor costs the ability to drag a
            // column narrower than its default, which nobody has ever wanted.
            let width = width + PADDING;
            egui_table::Column::new(width)
                .range(width..=800.0)
                .resizable(!c.fills)
        })
        .collect();

    let mut delegate = Delegate {
        grid,
        did: Did::default(),
        ctrl,
        shift,
    };
    egui_table::Table::new()
        .id_salt(id)
        .num_rows(delegate.grid.rows.len() as u64)
        .columns(columns)
        .num_sticky_cols(delegate.grid.sticky)
        .headers([egui_table::HeaderRow::new(ROW_HEIGHT)])
        .show(ui, &mut delegate);
    delegate.did
}

/// How tall this table's rows are: what it asked for, or a line of text.
fn row_height(grid: &Grid) -> f32 {
    if grid.row_height > 0.0 {
        grid.row_height
    } else {
        ROW_HEIGHT
    }
}

/// The header row on its own, for when there are no rows to hang it above.
fn draw_headers(ui: &mut Ui, grid: &Grid) {
    ui.horizontal(|ui| {
        for (i, column) in grid.columns.iter().enumerate() {
            let width = if column.fills {
                ui.available_width()
            } else {
                column.width
            };
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::hover());
            if ui.is_rect_visible(rect) && !column.title.is_empty() {
                ui.painter().text(
                    rect.left_center() + egui::vec2(PADDING, 0.0),
                    egui::Align2::LEFT_CENTER,
                    column.title,
                    egui::TextStyle::Body.resolve(ui.style()),
                    if i == grid.sort.0 {
                        theme::ACCENT
                    } else {
                        theme::TEXT
                    },
                );
            }
        }
    });
    ui.separator();
}

struct Delegate<'a> {
    grid: &'a mut Grid,
    did: Did,
    ctrl: bool,
    shift: bool,
}

impl egui_table::TableDelegate for Delegate<'_> {
    fn header_cell_ui(&mut self, ui: &mut Ui, cell: &egui_table::HeaderCellInfo) {
        let Some(column) = self.grid.columns.get(cell.col_range.start) else {
            return;
        };
        // The whole cell sorts, not just the few pixels the word covers. A
        // header is a target the width of its column; anything less is a game
        // of hunt-the-arrow.
        let index = cell.col_range.start;
        let (sorting, ascending) = self.grid.sort;
        let arrow = match (index == sorting, ascending) {
            (true, true) => " ↑",
            (true, false) => " ↓",
            _ => "",
        };
        let size = ui.available_size();
        let (rect, hit) = ui.allocate_exact_size(size, egui::Sense::click());
        if ui.is_rect_visible(rect) {
            if hit.hovered() {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(3.0),
                    egui::Color32::from_rgb(0x25, 0x29, 0x31),
                );
            }
            ui.painter().text(
                rect.left_center() + egui::vec2(PADDING, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{}{arrow}", column.title),
                egui::TextStyle::Body.resolve(ui.style()),
                if index == sorting {
                    theme::ACCENT
                } else {
                    theme::TEXT
                },
            );
        }
        if hit.on_hover_text("sort by this column").clicked() {
            self.did.sort = Some(index);
        }
    }

    fn cell_ui(&mut self, ui: &mut Ui, cell: &egui_table::CellInfo) {
        let row = cell.row_nr as usize;
        let col = cell.col_nr;
        let picked =
            self.grid.chosen.get(row).copied().unwrap_or(false) || self.grid.selected == Some(row);
        // Striping and selection are painted here rather than by the table:
        // egui_table draws cells, and the row is the thing a person sees.
        let background = if picked {
            Some(ui.visuals().selection.bg_fill)
        } else if row % 2 == 1 {
            Some(egui::Color32::from_rgb(0x1e, 0x21, 0x28))
        } else {
            None
        };
        if let Some(fill) = background {
            ui.painter()
                .rect_filled(ui.max_rect(), egui::Rounding::ZERO, fill);
        }

        // Every cell is inset from its column's edge. Without it the text sits
        // hard against the line beside it and the table reads as a spreadsheet
        // somebody forgot to finish.
        ui.add_space(PADDING);
        let editing = self.grid.editing == Some((row, col));
        if editing {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.grid.draft)
                    .desired_width(f32::INFINITY)
                    .frame(false),
            );
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.did.cancelled = true;
            } else if field.lost_focus() {
                self.did.committed = true;
            }
            return;
        }

        let Some(content) = self.grid.rows.get(row).and_then(|r| r.get(col)) else {
            return;
        };
        let response = match content {
            Cell::Places(places) => {
                let places = places.clone();
                // On the row's centre line, like the text beside them.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (n, (icon, state, hover)) in places.iter().enumerate() {
                        let hit = theme::place(ui, *icon, *state);
                        if *state != theme::Sync::Unknown && hit.on_hover_text(*hover).clicked() {
                            self.did.place = Some((row, n));
                        }
                    }
                })
                .response
            }
            Cell::Knob { value, range, text } => {
                // The pedal's own knob, with the pedal's own gestures: drag to
                // turn, click the reading to type it. A number that behaves one
                // way under the knobs and another way in a table is two things
                // to learn for one job.
                let (mut turned, range, text) = (*value, range.clone(), text.clone());
                let mut moved = None;
                // The reading is the cell's click target, exactly as it is
                // under the knobs: the knob takes the drag, the number under it
                // takes the click that starts typing.
                let reading = ui
                    .vertical_centered(|ui| {
                        if theme::knob(ui, &mut turned, range).changed() {
                            moved = Some(turned);
                        }
                        ui.add(
                            egui::Label::new(RichText::new(text).monospace().color(theme::ACCENT))
                                .selectable(false)
                                .sense(egui::Sense::click()),
                        )
                    })
                    .inner;
                if let Some(turned) = moved {
                    self.did.turned = Some((row, col, turned));
                }
                reading.on_hover_text("drag the knob to move this end\nclick to type it")
            }
            Cell::Text(text) | Cell::Dim(text) => {
                let rich = if matches!(content, Cell::Dim(_)) {
                    RichText::new(text).color(theme::DIM)
                } else {
                    RichText::new(text)
                };
                ui.add(
                    // Not selectable. egui makes label text selectable by
                    // default, which puts an I-beam and a highlight on every
                    // cell: it reads as an edit field that refuses to be edited.
                    egui::Label::new(rich)
                        .selectable(false)
                        // Truncated, not wrapped: a wrapped name makes one row
                        // twice the height of its neighbours and the table
                        // ripples.
                        .truncate()
                        .sense(egui::Sense::click()),
                )
            }
        };

        // The whole cell answers, not just the words in it. The widgets above
        // took their own clicks first, so a place icon and a knob still behave;
        // this picks up everything around them. Without it a right-click landed
        // only on the text, which in a row tall enough to hold a knob is a
        // sliver of it, and the row's menu looked broken.
        let response = response.union(ui.interact(
            ui.max_rect(),
            ui.id().with(("cell", row, col)),
            egui::Sense::click(),
        ));

        if response.clicked() {
            // A click on a row that is already selected, in a column that can
            // be typed into, starts typing: the same gesture every file
            // manager uses to rename. Nothing is lost, because the click could
            // not have changed the selection anyway.
            let editable = self.grid.columns.get(col).is_some_and(|c| c.editable);
            if editable && self.grid.selected == Some(row) && !self.ctrl && !self.shift {
                self.did.edit = Some((row, col));
            } else {
                self.did.clicked = Some((row, self.ctrl, self.shift));
            }
        }
        if response.double_clicked() {
            self.did.double_clicked = Some(row);
        }
        if response.secondary_clicked() {
            self.did.context = Some(row);
        }
        if !self.grid.menu.is_empty() {
            let items = self.grid.menu.clone();
            response.context_menu(|ui| {
                for (n, item) in items.iter().enumerate() {
                    if ui.button(item).clicked() {
                        self.did.chose = Some((row, n));
                        ui.close_menu();
                    }
                }
            });
        }
    }

    fn default_row_height(&self) -> f32 {
        row_height(self.grid)
    }
}
