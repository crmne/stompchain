//! Dark styling and the two custom widgets, following HX Edit's look closely
//! enough that the layout reads as familiar.

use egui::{Color32, Response, Rounding, Sense, Stroke, Ui, Vec2};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x12, 0x14, 0x18);
pub const PANEL: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x23);
pub const TEXT: Color32 = Color32::from_rgb(0xd6, 0xd9, 0xdf);
pub const DIM: Color32 = Color32::from_rgb(0x7d, 0x84, 0x92);
/// The amber HX Edit uses for values and the selected preset.
pub const ACCENT: Color32 = Color32::from_rgb(0xd8, 0xa8, 0x3b);

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
        ui.painter().text(
            rect.center_bottom() - Vec2::new(0.0, 11.0),
            egui::Align2::CENTER_CENTER,
            elide(name, 15),
            egui::FontId::proportional(11.0),
            text_colour,
        );
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

/// The gap a dragged block would land in, filled so there is no mistaking
/// it: a bar the height of the blocks either side, in the accent.
pub fn insert_marker(ui: &Ui, rect: egui::Rect) {
    let bar = egui::Rect::from_center_size(rect.center(), Vec2::new(5.0, BLOCK_HEIGHT * 0.9));
    ui.painter().rect_filled(bar, Rounding::same(2.5), ACCENT);
}

/// The dragged block, riding along under the pointer so the hand knows what
/// it is holding. A plain tile — name and category colour — floating above
/// everything on its own layer.
pub fn drag_ghost(ctx: &egui::Context, at: egui::Pos2, name: &str, colour: Color32) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("drag-ghost"),
    ));
    let rect = egui::Rect::from_center_size(
        at + Vec2::new(6.0, -14.0),
        Vec2::new(BLOCK_WIDTH * 0.8, BLOCK_HEIGHT * 0.55),
    );
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

/// A category, as a chip in that category's own colour.
pub fn category_chip(ui: &mut Ui, name: &str, colour: Color32, on: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.0),
        if on { Color32::BLACK } else { colour },
    );
    let size = Vec2::new(galley.size().x + 16.0, 22.0);
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
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        TEXT,
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
