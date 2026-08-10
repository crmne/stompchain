//! Dark styling and the two custom widgets, following HX Edit's look closely
//! enough that the layout reads as familiar.

use egui::{Color32, Response, Rounding, Sense, Stroke, Ui, Vec2};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x12, 0x14, 0x18);
pub const PANEL: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x23);
pub const TEXT: Color32 = Color32::from_rgb(0xd6, 0xd9, 0xdf);
pub const DIM: Color32 = Color32::from_rgb(0x7d, 0x84, 0x92);
/// The amber HX Edit uses for values and the selected preset.
pub const ACCENT: Color32 = Color32::from_rgb(0xd8, 0xa8, 0x3b);

/// The name of the semibold family, for the few places that want real weight
/// rather than egui's `strong()` — which only brightens the colour.
pub const SEMIBOLD: &str = "semibold";

/// A font id in the semibold family.
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(SEMIBOLD.into()))
}

/// The typeface: IBM Plex.
///
/// A tone editor is a panel of numbers that change while you look at them, so
/// the figures matter more than the letters. Plex has proper tabular figures —
/// 0.0 and 8.8 occupy the same width, so a value does not shuffle sideways as
/// a knob turns — a one that cannot be mistaken for an l, and a slashed zero in
/// the mono cut for the slot labels. It was drawn for machinery, which is what
/// this is, and it is OFL, so it ships with the binaries.
///
/// egui's own fonts stay on behind it as the fallback: Plex has no ★, ☆ or ●,
/// and a missing glyph draws as an empty box.
pub fn fonts(ctx: &egui::Context) {
    use egui::{FontData, FontFamily};

    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        ("plex", &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..]),
        ("plex-semibold", &include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")[..]),
        ("plex-mono", &include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")[..]),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes));
    }

    // First in the list is the primary; what follows is the fallback chain, so
    // egui's bundled fonts still answer for the glyphs Plex does not carry.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "plex".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plex-mono".to_owned());
    // The semibold cut is its own family: `RichText::strong()` in egui changes
    // colour, not weight, so anything that wants weight has to ask for it.
    let mut heavy = vec!["plex-semibold".to_owned()];
    heavy.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), heavy);

    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;

    v.dark_mode = true;
    v.panel_fill = PANEL;
    v.window_fill = BACKGROUND;
    v.extreme_bg_color = BACKGROUND;
    v.override_text_color = Some(TEXT);

    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x25, 0x29, 0x31);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x2f, 0x34, 0x3e);
    v.widgets.active.bg_fill = Color32::from_rgb(0x39, 0x3f, 0x4b);
    v.selection.bg_fill = Color32::from_rgb(0x2c, 0x3a, 0x52);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);

    // A deliberate scale rather than egui's defaults. Plex carries a large
    // x-height, so these read a size bigger than the numbers suggest; Small in
    // particular does a lot of work here as the colour-dimmed second voice, and
    // at egui's 9.0 it was a squint.
    use egui::{FontFamily::Monospace, FontFamily::Proportional, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Small, FontId::new(10.0, Proportional)),
        (TextStyle::Body, FontId::new(13.0, Proportional)),
        (TextStyle::Button, FontId::new(13.0, Proportional)),
        (TextStyle::Heading, FontId::new(16.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, Monospace)),
    ]
    .into();

    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.slider_width = 180.0;
    ctx.set_style(style);
}

/// One block in the signal chain: the model's own artwork above its name,
/// tinted and outlined when it is the block being edited, greyed when bypassed.
///
/// The artwork is what makes a chain readable at a glance — the same reason HX
/// Edit draws it. Models without a picture fall back to the name alone rather
/// than leaving a hole.
/// Our own category icons: the category's name, and the SVG drawn for it.
///
/// Bundled rather than read off disk. Names and knob ranges have no substitute
/// but HX Edit's own data; an icon is art we can simply make, so it is one less
/// thing borrowed — and it is there whether or not HX Edit is installed.
macro_rules! category_icons {
    ($($name:literal => $file:literal),* $(,)?) => {
        &[$((
            $name,
            concat!("bytes://category-", $file, ".svg"),
            include_bytes!(concat!("../assets/icons/", $file, ".svg")).as_slice(),
        )),*]
    };
}

const CATEGORY_ICONS: &[(&str, &str, &[u8])] = category_icons! {
    "Distortion" => "distortion",
    "Dynamics" => "dynamics",
    "EQ" => "eq",
    "Modulation" => "modulation",
    "Delay" => "delay",
    "Reverb" => "reverb",
    "Pitch/Synth" => "pitch-synth",
    "Filter" => "filter",
    "Wah" => "wah",
    "Amp+Cab" => "amp-cab",
    "Amp" => "amp",
    "Preamp" => "preamp",
    "Cab" => "cab",
    "IR" => "ir",
    "Volume/Pan" => "volume-pan",
    "Send/Return" => "send-return",
    "Looper" => "looper",
    "Input" => "input",
    "Output" => "output",
    "Split" => "split",
    "Merge" => "merge",
    "Connected Devices" => "connected-devices",
};

/// Hand the icons to egui's loaders, once, so they can be drawn by URI like any
/// other image.
pub fn register_icons(ctx: &egui::Context) {
    for (_, uri, bytes) in CATEGORY_ICONS {
        ctx.include_bytes(*uri, *bytes);
    }
}

/// The icon we have drawn for a category, if we have drawn one. A category we
/// have not falls back to HX Edit's own.
pub fn category_icon(name: &str) -> Option<Art> {
    CATEGORY_ICONS
        .iter()
        .find(|(label, _, _)| *label == name)
        .map(|(_, uri, _)| Art::whole((*uri).to_owned()))
}

/// A colour written as the three bytes it is.
pub fn rgb((r, g, b): (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Turn HX Edit's `0xRRGGBB` category colour into something paintable.
pub fn category_colour(rgb: u32) -> Color32 {
    Color32::from_rgb(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    )
}

/// One block in the chain, tinted with its category's own colour.
///
/// The colours are HX Edit's, read from its catalog rather than invented, so a
/// chain here reads the same as a chain there: amber distortion, yellow EQ and
/// dynamics, red amps, green delay. A bypassed block is drawn dim and its name
/// bracketed, which is also what HX Edit does.
pub fn block_button_tinted(
    ui: &mut Ui,
    name: &str,
    category: Option<&str>,
    artwork: Option<&Art>,
    selected: bool,
    enabled: bool,
    accent: Color32,
) -> Response {
    let size = Vec2::new(BLOCK_WIDTH, BLOCK_HEIGHT);
    // Draggable as well as clickable: a chain is an order, and dragging is how
    // people reorder things.
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        // The category colour carries the meaning; the fill only has to keep
        // it legible. A bypassed block loses its colour, which is the whole
        // point of bypassing it.
        let tint = if enabled { accent } else { DIM };
        let fill = if !enabled {
            Color32::from_rgb(0x22, 0x25, 0x2b)
        } else if selected {
            tint.gamma_multiply(0.30)
        } else {
            Color32::from_rgb(0x2a, 0x2e, 0x36)
        };
        let border = if response.dragged() {
            Stroke::new(2.0_f32, ACCENT)
        } else if selected {
            Stroke::new(2.0_f32, tint)
        } else {
            Stroke::new(1.0_f32, tint.gamma_multiply(0.55))
        };

        let painter = ui.painter();
        painter.rect_filled(rect, Rounding::same(5.0), fill);
        painter.rect_stroke(rect, Rounding::same(5.0), border);

        let text_colour = if enabled { tint } else { DIM };
        if let Some(art) = artwork {
            let box_ = egui::Rect::from_center_size(
                rect.center() - Vec2::new(0.0, 10.0),
                Vec2::new(76.0, 48.0),
            );
            let tint = if enabled {
                Color32::WHITE
            } else {
                Color32::from_gray(110)
            };
            art.paint(ui, box_, tint);
        }

        // Model names routinely exceed the tile, so they are truncated with an
        // ellipsis; the full name is on the hover tooltip.
        //
        // The category goes underneath rather than into the name. An Amp+Cab
        // block holds two models and saying so in the name - "Cali Rectifire +
        // Cab" - only pushed the name itself off the tile. HX Edit puts the
        // category here for the same reason.
        let name_y = if category.is_some() { 20.0 } else { 11.0 };
        ui.painter().text(
            rect.center_bottom() - Vec2::new(0.0, name_y),
            egui::Align2::CENTER_CENTER,
            elide(name, 15),
            egui::FontId::proportional(11.0),
            text_colour,
        );
        if let Some(category) = category {
            ui.painter().text(
                rect.center_bottom() - Vec2::new(0.0, 8.0),
                egui::Align2::CENTER_CENTER,
                elide(category, 16),
                egui::FontId::proportional(9.0),
                if enabled { DIM } else { DIM.gamma_multiply(0.6) },
            );
        }
    }

    response.on_hover_text(name)
}

/// A small filled circle. Painted rather than typed, because the bundled font
/// has no glyph for one and an empty box is worse than no indicator at all.
pub fn status_dot(ui: &mut Ui, colour: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(rect.center(), 5.0, colour);
    }
    response
}

/// The actions that live in the header, drawn rather than typed.
///
/// Typing them was the obvious route and the wrong one: no single font carries
/// ⧉, ⎘ and ⌫, so the set would have come from three fallbacks at three weights
/// on a good day and as empty boxes on a bad one. Drawn from coordinates they
/// are one set, they scale with the window, and they cannot go missing.
///
/// The shapes follow the Feather icon vocabulary, which is what a person has
/// seen ten thousand times and so does not have to read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Save,
    Undo,
    Redo,
    Copy,
    Paste,
    Remove,
    Gear,
    Sliders,
    Keep,
}

impl Icon {
    /// The strokes, as polylines on a 24×24 grid.
    fn strokes(self) -> &'static [&'static [(f32, f32)]] {
        match self {
            // A floppy disk: body with the corner taken off, shutter, label.
            Icon::Save => &[
                &[
                    (3.0, 3.0),
                    (16.0, 3.0),
                    (21.0, 8.0),
                    (21.0, 19.0),
                    (3.0, 19.0),
                    (3.0, 3.0),
                ],
                &[(7.0, 19.0), (7.0, 12.0), (17.0, 12.0), (17.0, 19.0)],
                &[(7.0, 3.0), (7.0, 8.0), (15.0, 8.0)],
            ],
            // An arrow turning back on itself, left for undo and right for redo.
            Icon::Undo => &[
                &[(9.0, 14.0), (4.0, 9.0), (9.0, 4.0)],
                &[(20.0, 20.0), (20.0, 13.0), (16.0, 9.0), (4.0, 9.0)],
            ],
            Icon::Redo => &[
                &[(15.0, 14.0), (20.0, 9.0), (15.0, 4.0)],
                &[(4.0, 20.0), (4.0, 13.0), (8.0, 9.0), (20.0, 9.0)],
            ],
            // Two sheets, one behind the other.
            Icon::Copy => &[
                &[
                    (9.0, 9.0),
                    (21.0, 9.0),
                    (21.0, 21.0),
                    (9.0, 21.0),
                    (9.0, 9.0),
                ],
                &[(5.0, 15.0), (3.0, 15.0), (3.0, 3.0), (15.0, 3.0), (15.0, 5.0)],
            ],
            // A clipboard, with its clip.
            Icon::Paste => &[
                &[
                    (4.0, 5.0),
                    (20.0, 5.0),
                    (20.0, 22.0),
                    (4.0, 22.0),
                    (4.0, 5.0),
                ],
                &[(9.0, 5.0), (9.0, 2.0), (15.0, 2.0), (15.0, 5.0)],
            ],
            // A bin: lid, body, handle.
            Icon::Remove => &[
                &[(3.0, 6.0), (21.0, 6.0)],
                &[(5.0, 6.0), (5.0, 22.0), (19.0, 22.0), (19.0, 6.0)],
                &[(9.0, 6.0), (9.0, 3.0), (15.0, 3.0), (15.0, 6.0)],
            ],
            // The cog is two circles and eight teeth, drawn in `paint` rather
            // than listed here: as a traced rim it came out as a muddy blob at
            // the size it is actually used, because a 26-point outline has no
            // room to be a cog in seventeen pixels.
            Icon::Gear => &[],
            // An arrow going down into a tray: put this away.
            Icon::Keep => &[
                &[(12.0, 3.0), (12.0, 14.0)],
                &[(7.0, 9.0), (12.0, 14.0), (17.0, 9.0)],
                &[(3.0, 16.0), (3.0, 21.0), (21.0, 21.0), (21.0, 16.0)],
            ],
            // Three faders, each with its cap at a different place: an EQ.
            Icon::Sliders => &[
                &[(6.0, 3.0), (6.0, 9.0)],
                &[(6.0, 14.0), (6.0, 21.0)],
                &[(3.0, 11.5), (9.0, 11.5)],
                &[(12.0, 3.0), (12.0, 15.0)],
                &[(9.0, 17.5), (15.0, 17.5)],
                &[(12.0, 20.0), (12.0, 21.0)],
                &[(18.0, 3.0), (18.0, 5.0)],
                &[(15.0, 7.5), (21.0, 7.5)],
                &[(18.0, 10.0), (18.0, 21.0)],
            ],
        }
    }

    /// Paint into a square, at whatever size the square is.
    pub fn paint(self, painter: &egui::Painter, box_: egui::Rect, colour: Color32) {
        let scale = box_.width() / 24.0;
        let stroke = Stroke::new((1.6 * scale).max(1.0), colour);
        let at = |(x, y): (f32, f32)| box_.min + Vec2::new(x * scale, y * scale);
        for line in self.strokes() {
            painter.add(egui::Shape::line(
                line.iter().map(|&p| at(p)).collect(),
                stroke,
            ));
        }
        if self == Icon::Gear {
            // A rim, a hub, and eight teeth standing off the rim. Circles drawn
            // as circles stay round at any size, which a traced outline does
            // not.
            let centre = box_.center();
            painter.circle_stroke(centre, 6.5 * scale, stroke);
            painter.circle_stroke(centre, 2.6 * scale, stroke);
            for i in 0..8 {
                let angle = std::f32::consts::TAU * i as f32 / 8.0;
                let (sin, cos) = angle.sin_cos();
                let dir = Vec2::new(cos, sin);
                painter.line_segment(
                    [centre + dir * 6.0 * scale, centre + dir * 10.0 * scale],
                    stroke,
                );
            }
        }
    }
}

/// One drawn action, as a frameless button.
///
/// Dim at rest and bright under the pointer, so a row of them reads as one
/// quiet group until you go looking — the header should show the preset, not
/// an inventory of the program.
pub fn icon_button(ui: &mut Ui, icon: Icon, enabled: bool) -> Response {
    const BOX: f32 = 24.0;
    // Sensing hover only, when off, is what makes the button unclickable: a
    // hand-allocated widget has no `add_enabled` to fall back on, and a
    // greyed-out icon that still fires is worse than one that is not greyed.
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BOX), sense);
    if ui.is_rect_visible(rect) {
        let colour = if !enabled {
            DIM.gamma_multiply(0.4)
        } else if response.hovered() {
            ACCENT
        } else {
            TEXT
        };
        if enabled && response.hovered() {
            ui.painter().rect_filled(
                rect,
                Rounding::same(4.0),
                Color32::from_rgb(0x25, 0x29, 0x31),
            );
        }
        // The glyph sits inside the hit box, so neighbours do not crowd it.
        let inset = egui::Rect::from_center_size(rect.center(), Vec2::splat(BOX - 7.0));
        icon.paint(ui.painter(), inset, colour);
    }
    response
}

/// The gap a dragged block would land in, filled so there is no mistaking
/// it: a bar the height of the blocks either side, in the accent.
pub fn insert_marker(ui: &Ui, rect: egui::Rect) {
    let bar = egui::Rect::from_center_size(rect.center(), Vec2::new(5.0, BLOCK_HEIGHT * 0.9));
    ui.painter().rect_filled(bar, Rounding::same(2.5), ACCENT);
}

/// The dragged block, riding along under the pointer so the hand knows what
/// it is holding. A plain tile — name and category colour — floating above
/// everything on its own layer.
///
/// Centred on the pointer, deliberately: the tile is what the eye aims with,
/// and an offset tile meant a drop that looked right landed one gap over.
pub fn drag_ghost(ctx: &egui::Context, at: egui::Pos2, name: &str, colour: Color32) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("drag-ghost"),
    ));
    let rect = egui::Rect::from_center_size(at, Vec2::new(BLOCK_WIDTH * 0.8, BLOCK_HEIGHT * 0.55));
    painter.rect_filled(
        rect,
        Rounding::same(5.0),
        Color32::from_rgba_unmultiplied(0x2a, 0x2e, 0x36, 230),
    );
    painter.rect_stroke(rect, Rounding::same(5.0), Stroke::new(2.0_f32, colour));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        elide(name, 13),
        egui::FontId::proportional(11.0),
        TEXT,
    );
}

/// The category's colour, as a bar beside the name it belongs to.
pub fn category_swatch(ui: &mut Ui, colour: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(6.0, 22.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, Rounding::same(3.0_f32), colour);
    }
    response
}

/// A category, as a chip in that category's own colour, with HX Edit's own
/// glyph for it where one is installed.
///
/// The icons are monochrome silhouettes, so they are tinted to match the text
/// rather than drawn as they come — which also means the chip still reads when
/// it is filled and the text goes black.
pub fn category_chip(
    ui: &mut Ui,
    name: &str,
    icon: Option<&Art>,
    colour: Color32,
    on: bool,
) -> Response {
    const ICON: f32 = 14.0;
    const GAP: f32 = 5.0;

    let ink = if on { Color32::BLACK } else { colour };
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.0), ink);
    let art_width = icon.map_or(0.0, |_| ICON + GAP);
    let size = Vec2::new(galley.size().x + art_width + 16.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, Rounding::same(11.0), colour);
    } else {
        painter.rect_stroke(
            rect,
            Rounding::same(11.0),
            Stroke::new(
                1.0_f32,
                colour.gamma_multiply(if response.hovered() { 1.0 } else { 0.5 }),
            ),
        );
    }
    // Icon and label as one group, centred together.
    let left = rect.center().x - (galley.size().x + art_width) / 2.0;
    if let Some(icon) = icon {
        icon.paint(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(left, rect.center().y - ICON / 2.0),
                Vec2::splat(ICON),
            ),
            ink,
        );
    }
    painter.galley(
        egui::pos2(left + art_width, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    response
}

/// A subcategory, as a smaller pill under the category it belongs to.
///
/// Deliberately quieter than [`category_chip`]: no icon, no colour of its own,
/// and shorter. Mono / Stereo / Legacy is a second question you only ask after
/// the first one, and a pill that shouted as loud as Distortion would make the
/// row above it look like a sibling rather than a parent.
pub fn shelf_pill(ui: &mut Ui, name: &str, on: bool) -> Response {
    let ink = if on { Color32::BLACK } else { DIM };
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), egui::FontId::proportional(11.0), ink);
    let size = Vec2::new(galley.size().x + 14.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, Rounding::same(9.0), TEXT);
    } else if response.hovered() {
        painter.rect_filled(
            rect,
            Rounding::same(9.0),
            Color32::from_rgb(0x25, 0x29, 0x31),
        );
    }
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    response
}

/// One model in the browser, as a picture with its name under it.
///
/// A grid of thumbnails the way Logic's Pedalboard shows its shelf: with a few
/// hundred models to choose from, the picture is what you actually recognise.
pub fn model_tile(ui: &mut Ui, name: &str, artwork: Option<&Art>, selected: bool) -> Response {
    let size = Vec2::new(140.0, 136.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter().with_clip_rect(rect);
    if selected {
        painter.rect_filled(rect, Rounding::same(5.0), ui.visuals().selection.bg_fill);
    } else if response.hovered() {
        painter.rect_filled(
            rect,
            Rounding::same(5.0),
            Color32::from_rgb(0x25, 0x29, 0x31),
        );
    }

    let art = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 8.0, rect.top() + 6.0),
        Vec2::new(size.x - 16.0, 94.0),
    );
    match artwork {
        Some(a) => a.paint(ui, art, Color32::WHITE),
        None => {
            painter.rect_filled(
                art,
                Rounding::same(3.0),
                Color32::from_rgb(0x21, 0x25, 0x2c),
            );
        }
    }

    // Two lines of name, wrapped by hand: the tiles must stay the same size or
    // the grid stops being a grid.
    let mut galley = ui.painter().layout(
        name.to_owned(),
        egui::FontId::proportional(12.0),
        if selected { ACCENT } else { TEXT },
        size.x - 8.0,
    );
    if galley.rows.len() > 2 {
        galley = ui.painter().layout_no_wrap(
            format!("{}…", name.chars().take(16).collect::<String>()),
            egui::FontId::proportional(12.0),
            if selected { ACCENT } else { TEXT },
        );
    }
    painter.galley(
        egui::pos2(rect.center().x - galley.size().x / 2.0, art.bottom() + 4.0),
        galley,
        TEXT,
    );

    response.on_hover_text(name)
}

/// Signal-path geometry. Fixed rather than derived so the two lanes of a split
/// line up column for column, which is the whole point of drawing them stacked.
pub const BLOCK_WIDTH: f32 = 104.0;
pub const BLOCK_HEIGHT: f32 = 86.0;
pub const WIRE_WIDTH: f32 = 22.0;
pub const JUNCTION_WIDTH: f32 = 34.0;
/// Height of one lane including the gap under it.
pub const LANE_HEIGHT: f32 = BLOCK_HEIGHT + 10.0;
/// One block and the wire that follows it.
pub const COLUMN: f32 = BLOCK_WIDTH + WIRE_WIDTH;

pub const WIRE: Color32 = Color32::from_rgb(0x4a, 0x50, 0x5c);

/// A picture to draw on a tile: a whole file, or one frame of a strip.
///
/// The endpoints need the second form. HX Edit draws whichever destination an
/// input or output is routed to, and those live as vertical strips of equal
/// frames in one file rather than as separate images.
#[derive(Clone, PartialEq)]
pub struct Art {
    pub uri: String,
    /// `(frame, total)` when the file is a strip; `None` for a plain image.
    pub frame: Option<(usize, usize)>,
}

impl Art {
    pub fn whole(uri: String) -> Art {
        Art { uri, frame: None }
    }

    pub fn strip(uri: String, frame: usize, total: usize) -> Art {
        Art {
            uri,
            frame: Some((frame, total)),
        }
    }

    pub fn paint(&self, ui: &Ui, area: egui::Rect, tint: Color32) {
        match self.frame {
            None => fit(ui, &self.uri, area, tint),
            Some((frame, total)) if total > 0 => {
                // One frame of a vertical strip, kept square: the source cells
                // are square, so the drawn box must be too or the icon skews.
                let side = area.height().min(area.width());
                let box_ = egui::Rect::from_center_size(area.center(), Vec2::splat(side));
                let top = frame.min(total - 1) as f32 / total as f32;
                egui::Image::new(&self.uri)
                    .tint(tint)
                    .uv(egui::Rect::from_min_max(
                        egui::pos2(0.0, top),
                        egui::pos2(1.0, top + 1.0 / total as f32),
                    ))
                    .paint_at(ui, box_);
            }
            _ => {}
        }
    }
}

/// Draw an image centred inside `area`, preserving its proportions.
///
/// `Image::paint_at` stretches to whatever rectangle it is given, which turns
/// wide pedal photographs into squashed ones. Asking the image for its natural
/// size first and scaling by the smaller of the two ratios letterboxes it
/// instead.
fn fit(ui: &Ui, uri: &str, area: egui::Rect, tint: Color32) {
    let image = egui::Image::new(uri).maintain_aspect_ratio(true).tint(tint);

    // Until the texture has loaded its size is unknown; fill the box for that
    // frame and let the next one place it properly.
    let natural = image
        .load_and_calc_size(ui, egui::Vec2::splat(f32::INFINITY))
        .unwrap_or(area.size());

    let scale = (area.width() / natural.x)
        .min(area.height() / natural.y)
        .min(1.0);
    let placed = egui::Rect::from_center_size(area.center(), natural * scale);
    image.paint_at(ui, placed);
}

/// The pedal, drawn as large as it goes without inventing detail.
///
/// HX Edit's artwork is 128 to 256 pixels square. Asking for more than that
/// stretches it, and a stretched pedal looks worse than a small sharp one, so
/// this never scales past 1:1 — it only shrinks to fit `max`.
pub fn pedal_image(ui: &mut Ui, art: &Art, max: f32) -> Response {
    let image = egui::Image::new(&art.uri).maintain_aspect_ratio(true);
    let mut natural = image
        .load_and_calc_size(ui, Vec2::splat(f32::INFINITY))
        .unwrap_or(Vec2::splat(max));
    // One frame of a strip is as tall as it is wide; the file is the whole
    // strip, so its height is the frame count times that.
    if let Some((_, total)) = art.frame {
        if total > 0 {
            natural.y /= total as f32;
        }
    }

    let scale = (max / natural.x).min(max / natural.y).min(1.0);
    let size = natural * scale;
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        art.paint(ui, rect, Color32::WHITE);
    }
    response
}

fn elide(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars().take(max - 1).collect::<String>() + "…"
}

/// A rotary knob, the way a pedal has them.
///
/// Sliders are fine for a mixer but wrong for a stompbox: the whole point of the
/// artwork is that the thing looks like the pedal you already know, and a pedal
/// has knobs. Dragging vertically turns it, which is what every audio
/// application does and what the hand expects.
pub fn knob(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> Response {
    let size = Vec2::splat(44.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    let (min, max) = (*range.start(), *range.end());
    let span = max - min;
    if response.dragged() && span.abs() > f32::EPSILON {
        // A full sweep takes about 200px of travel, which is fine control
        // without being tedious. Up increases, as on a real knob turned right.
        let delta = -response.drag_delta().y / 200.0 * span;
        *value = (*value + delta).clamp(min.min(max), max.max(min));
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let centre = rect.center();
        let radius = rect.width() * 0.42;
        let fraction = if span.abs() > f32::EPSILON {
            ((*value - min) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Knobs sweep 270°, leaving a gap at the bottom so the pointer position
        // is unambiguous.
        let start = std::f32::consts::PI * 0.75;
        let sweep = std::f32::consts::PI * 1.5;
        let angle = start + sweep * fraction;

        let painter = ui.painter();
        painter.circle_filled(centre, radius, Color32::from_rgb(0x2b, 0x2f, 0x38));
        painter.circle_stroke(
            centre,
            radius,
            Stroke::new(1.0_f32, Color32::from_rgb(0x50, 0x56, 0x62)),
        );

        // The travelled arc, drawn as short segments.
        let steps = 24;
        let mut previous = None;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            if t > fraction {
                break;
            }
            let a = start + sweep * t;
            let p = centre + Vec2::new(a.cos(), a.sin()) * (radius + 3.0);
            if let Some(prev) = previous {
                painter.line_segment([prev, p], Stroke::new(2.5_f32, ACCENT));
            }
            previous = Some(p);
        }

        let pointer = centre + Vec2::new(angle.cos(), angle.sin()) * (radius - 5.0);
        painter.line_segment([centre, pointer], Stroke::new(2.0_f32, TEXT));
        painter.circle_filled(centre, 2.5, Color32::from_rgb(0x8a, 0x90, 0x9c));
    }

    response
}

/// A footswitch-style toggle, for the parameters a pedal exposes as a switch.
///
/// Sized to sit in the same grid cell as a knob so a row of controls lines up
/// whatever mix of the two a model happens to have.
pub fn switch(on: &mut bool) -> impl egui::Widget + '_ {
    move |ui: &mut Ui| {
        let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
        if response.clicked() {
            *on = !*on;
            response.mark_changed();
        }
        if ui.is_rect_visible(rect) {
            let body = egui::Rect::from_center_size(rect.center(), Vec2::new(30.0, 30.0));
            let painter = ui.painter();
            painter.rect_filled(
                body,
                Rounding::same(5.0),
                Color32::from_rgb(0x2b, 0x2f, 0x38),
            );
            painter.rect_stroke(
                body,
                Rounding::same(5.0),
                Stroke::new(1.0_f32, Color32::from_rgb(0x50, 0x56, 0x62)),
            );
            painter.circle_filled(
                body.center(),
                7.0,
                if *on {
                    ACCENT
                } else {
                    Color32::from_rgb(0x3a, 0x3f, 0x49)
                },
            );
        }
        response
    }
}

/// Where the signal forks into a parallel branch, or comes back together.
///
/// Drawn as the wiring itself rather than as a box in the line, which is what
/// it is. The main line runs straight through — a branch is an addition below
/// the line, not a detour of it — and a curve drops away to each branch lane.
/// HX Edit draws it the same way, and it is what makes the moment the path
/// divides legible at a glance. It stays clickable, since a split still has a
/// mode and a join still has levels.
///
/// `opening` curves out to the branches; the merge is the same figure
/// mirrored. `below` is how many branch lanes hang under the main line.
/// `tag` is worn under the dot — "A/B", "XO" — for split types that change
/// how the preset behaves; the default Y goes untagged.
pub fn junction(
    ui: &mut Ui,
    below: usize,
    opening: bool,
    selected: bool,
    tag: Option<&str>,
) -> Response {
    let size = Vec2::new(JUNCTION_WIDTH, BLOCK_HEIGHT);
    // Draggable as well as clickable: the attach point is a position on the
    // line, and positions are things you drag.
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let colour = if selected {
        ACCENT
    } else if response.hovered() {
        TEXT
    } else {
        WIRE
    };
    let stroke = Stroke::new(if selected { 2.0_f32 } else { 1.5_f32 }, colour);
    let painter = ui.painter();
    let cy = rect.center().y;
    painter.hline(rect.x_range(), cy, stroke);

    // One curve per branch, horizontal at both ends so the wiring reads as
    // wiring: it leaves the line level and arrives at the lane level. Painted
    // past the widget's own rect — the lanes below are still this figure's
    // to meet.
    for n in 1..=below {
        let ty = cy + LANE_HEIGHT * n as f32;
        let (from, to) = if opening {
            (egui::pos2(rect.left(), cy), egui::pos2(rect.right(), ty))
        } else {
            (egui::pos2(rect.left(), ty), egui::pos2(rect.right(), cy))
        };
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            [
                from,
                egui::pos2(from.x + JUNCTION_WIDTH, from.y),
                egui::pos2(to.x - JUNCTION_WIDTH, to.y),
                to,
            ],
            false,
            Color32::TRANSPARENT,
            stroke,
        ));
    }
    // A dot on the fork, so it reads as something you can click.
    painter.circle_filled(rect.center(), 4.0, colour);
    if let Some(tag) = tag {
        painter.text(
            rect.center() + Vec2::new(0.0, 13.0),
            egui::Align2::CENTER_CENTER,
            tag,
            egui::FontId::proportional(9.0),
            colour,
        );
    }

    response
}

/// How tall the offer of a parallel branch is; see [`ghost_branch`].
pub const GHOST_HEIGHT: f32 = 40.0;

/// The offer of a parallel branch, dashed because it does not exist yet:
/// where the line would fork, the lane the blocks would sit on, and where it
/// would merge back — with a `+` where the first block goes.
///
/// This replaced a label reading "parallel branch" floating in the signal
/// path, which looked like a thing in the chain rather than an action.
/// `from_y` is the main line's height, where the fork will leave it.
pub fn ghost_branch(ui: &mut Ui, width: f32, from_y: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, GHOST_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let colour = if response.hovered() {
        ACCENT
    } else {
        WIRE.gamma_multiply(0.9)
    };
    let stroke = Stroke::new(1.5_f32, colour);
    let painter = ui.painter();
    let y = rect.bottom() - 12.0;
    let reach = 30.0_f32.min(width * 0.25);

    let dashed_curve = |from: egui::Pos2, to: egui::Pos2| {
        let points = egui::epaint::CubicBezierShape::from_points_stroke(
            [
                from,
                egui::pos2(from.x + reach, from.y),
                egui::pos2(to.x - reach, to.y),
                to,
            ],
            false,
            Color32::TRANSPARENT,
            Stroke::NONE,
        )
        .flatten(Some(0.5));
        egui::Shape::dashed_line(&points, stroke, 4.0, 4.0)
    };
    // Fork out of the main line, and merge back into it.
    painter.extend(dashed_curve(
        egui::pos2(rect.left(), from_y),
        egui::pos2(rect.left() + reach, y),
    ));
    painter.extend(dashed_curve(
        egui::pos2(rect.right() - reach, y),
        egui::pos2(rect.right(), from_y),
    ));

    // The lane itself, leaving room for the `+` at its middle.
    let centre = egui::pos2(rect.center().x, y);
    painter.extend(egui::Shape::dashed_line(
        &[
            egui::pos2(rect.left() + reach, y),
            egui::pos2(centre.x - 14.0, y),
        ],
        stroke,
        4.0,
        4.0,
    ));
    painter.extend(egui::Shape::dashed_line(
        &[
            egui::pos2(centre.x + 14.0, y),
            egui::pos2(rect.right() - reach, y),
        ],
        stroke,
        4.0,
        4.0,
    ));

    // The `+`, always visible: this is the affordance, not a hover surprise.
    if response.hovered() {
        painter.circle_filled(centre, 8.0, ACCENT);
    } else {
        painter.circle_stroke(centre, 8.0, Stroke::new(1.5_f32, colour));
    }
    let mark = if response.hovered() {
        Color32::BLACK
    } else {
        colour
    };
    painter.line_segment(
        [centre - Vec2::new(4.0, 0.0), centre + Vec2::new(4.0, 0.0)],
        Stroke::new(2.0_f32, mark),
    );
    painter.line_segment(
        [centre - Vec2::new(0.0, 4.0), centre + Vec2::new(0.0, 4.0)],
        Stroke::new(2.0_f32, mark),
    );

    response
}

/// One place a dragged fork or merge can land: a dot on the wire, grown and
/// lit when it is the one the pointer would choose.
pub fn attach_marker(ui: &Ui, at: egui::Pos2, hot: bool) {
    let painter = ui.painter();
    if hot {
        painter.circle_filled(at, 5.0, ACCENT);
    } else {
        painter.circle_filled(at, 3.0, WIRE);
        painter.circle_stroke(at, 3.0, Stroke::new(1.0_f32, DIM));
    }
}

/// Mark a drop that would trade places with the block under the pointer.
pub fn swap_marker(ui: &Ui, rect: egui::Rect) {
    ui.painter().rect_stroke(
        rect.expand(2.0),
        Rounding::same(6.0),
        Stroke::new(3.0_f32, ACCENT),
    );
}

/// A gap in the chain you can add something to.
///
/// Drawn as ordinary wire until the pointer is over it, when it offers a `+`.
/// Adding a block was previously only possible by finding an empty slot and
/// changing its model, which meant knowing the slot topology — this puts the
/// action where the thing goes.
pub fn insert_point(ui: &mut Ui, height: f32) -> Response {
    // Click *and drag*: a click-only widget here never completed its click,
    // while the blocks either side — which sense drags — always did. Sensing
    // the drag makes this widget the one that owns the press.
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(WIRE_WIDTH, height), Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let y = rect.center().y;
    painter.hline(rect.x_range(), y, Stroke::new(1.5_f32, WIRE));

    if response.hovered() {
        let centre = egui::pos2(rect.center().x, y);
        painter.circle_filled(centre, 8.0, ACCENT);
        painter.line_segment(
            [egui::pos2(centre.x - 4.0, y), egui::pos2(centre.x + 4.0, y)],
            Stroke::new(2.0_f32, Color32::BLACK),
        );
        painter.line_segment(
            [egui::pos2(centre.x, y - 4.0), egui::pos2(centre.x, y + 4.0)],
            Stroke::new(2.0_f32, Color32::BLACK),
        );
    }
    response
}

/// Blank wire, for padding a short lane out to the merge point.
pub fn wire_run(ui: &mut Ui, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .hline(rect.x_range(), rect.center().y, Stroke::new(1.5_f32, WIRE));
    }
}
