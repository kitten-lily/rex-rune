//! One terminal pane: its stream, local view state (selection, preedit, retained-frame offset)
//! and the cell grid, drawn on a fixed column grid.
use super::{
    keys,
    theme::{ACCENT, TERMINAL_SELECTION, mix, rgb},
};
use rex_linux::{
    connection::{Command, Connection},
    links::Link,
    terminal::{Cell, CursorShape, Frame},
    terminal_theme::{TerminalTheme, is_light},
};
use rune::{
    El, LineCap, Mods, Path, Shape, Span, TextEdit, TextStyle, canvas, el, memo, rich, text,
};
use serde_json::json;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

pub const PADDING: f32 = 8.;

pub struct Term {
    pub connection: Connection,
    pub frame: Option<Arc<Frame>>,
    pub last_size: Option<(u16, u16)>,
    /// The grid's rect in window points (inside the padding).
    pub rect: [f32; 4],
    /// The first live frame stays hidden until the server acknowledges this grid's size.
    initial_size: InitialSize,
    pub selection: Option<(usize, usize)>,
    selection_mode: SelectionMode,
    selection_anchor: Option<(usize, usize)>,
    pub search: Option<TextEdit>,
    pub preedit: String,
    pub notice: Option<String>,
    pub scroll: f32,
    /// A button press forwarded to the program, until its release: the gesture keeps going to
    /// the program even if the program stops tracking or Shift goes down mid-drag.
    pub mouse: Option<MouseGesture>,
    /// The last cell a motion report named, so moves within a cell send nothing.
    pub mouse_cell: Option<(u16, u16)>,
    /// The link under the pointer while Ctrl is held, with the cell the pointer is on.
    pub link: Option<(usize, Link)>,
    /// The link a Ctrl+press went down on; releasing over the same link opens it.
    pub pressed_link: Option<String>,
}

#[derive(Default)]
struct InitialSize {
    expected: Option<(u16, u16)>,
    ready: bool,
}

impl InitialSize {
    fn layout(&mut self, size: (u16, u16), frame: Option<(u16, u16)>) {
        if self.ready {
            return;
        }
        self.expected = Some(size);
        self.observe(frame);
    }

    fn observe(&mut self, frame: Option<(u16, u16)>) {
        if self.expected.is_some() && self.expected == frame {
            self.expected = None;
            self.ready = true;
        }
    }

    fn renders(&self, live: bool) -> bool {
        self.ready || !live
    }
}

#[derive(Clone, Copy)]
pub struct MouseGesture {
    pub button: &'static str,
}

#[derive(Clone, Copy, Default)]
enum SelectionMode {
    #[default]
    Cell,
    Word,
    Line,
}
#[derive(Clone, Copy, PartialEq)]
pub struct Metrics {
    pub cell: (f32, f32),
    pub style: TextStyle,
}

const FEATURES: &str = "'liga' 0, 'calt' 0, 'clig' 0, 'kern' 0";

pub fn metrics(size: f32) -> Metrics {
    let style = TextStyle::new(size, 400.)
        .lh((size * 1.4).ceil())
        .features(FEATURES)
        .mono();
    let width = rune::text::advance("M", style).max(1.);
    Metrics {
        cell: (width, (size * 1.4).ceil()),
        style,
    }
}

pub struct Status {
    pub live: bool,
    pub owner: bool,
    pub status: String,
}

impl Term {
    pub fn new(connection: Connection) -> Self {
        let frame = connection
            .model
            .lock()
            .expect("terminal model")
            .frame
            .clone();
        Self {
            connection,
            frame,
            last_size: None,
            rect: [0.; 4],
            initial_size: InitialSize::default(),
            selection: None,
            selection_mode: SelectionMode::default(),
            selection_anchor: None,
            search: None,
            preedit: String::new(),
            notice: None,
            scroll: 0.,
            mouse: None,
            mouse_cell: None,
            link: None,
            pressed_link: None,
        }
    }

    pub fn refresh(&mut self) {
        let frame = self
            .connection
            .model
            .lock()
            .expect("terminal model")
            .frame
            .clone();
        self.initial_size
            .observe(frame.as_ref().map(|frame| (frame.cols, frame.rows)));
        // A selection is cell indices into a grid of one size; in another size (a resize,
        // or the first frame at this client's size) they name other text.
        let size = |f: &Option<Arc<Frame>>| f.as_ref().map(|f| (f.cols, f.rows));
        if size(&self.frame).is_some() && size(&self.frame) != size(&frame) {
            self.selection = None;
            self.selection_anchor = None;
        }
        self.frame = frame;
        // The same cell may hold a different link, or none, after output.
        if let Some((index, _)) = self.link.take() {
            self.link = self
                .frame
                .as_ref()
                .and_then(|f| f.link_at(index))
                .map(|l| (index, l));
        }
    }

    pub fn status(&self) -> Status {
        let m = self.connection.model.lock().expect("terminal model");
        Status {
            live: m.connected && m.control_connected,
            owner: m.owner,
            status: m.status.clone(),
        }
    }

    fn display_frame(&self) -> Option<&Arc<Frame>> {
        if self.initial_size.renders(self.status().live) {
            self.frame.as_ref()
        } else {
            None
        }
    }

    pub fn send(&mut self, command: Command) {
        if let Err(error) = self.connection.commands.try_send(command) {
            self.notice = Some(format!("Input not sent: {error}"));
        }
    }

    pub fn visible_rows(&self, cell: (f32, f32)) -> usize {
        (self.rect[3] / cell.1).floor().max(1.) as usize
    }

    pub fn display_row(&self, cell: (f32, f32)) -> usize {
        self.frame
            .as_ref()
            .map_or(0, |f| f.display_row_start(self.visible_rows(cell)))
    }

    pub fn hidden_rows(&self, cell: (f32, f32)) -> usize {
        if self.rect[3] <= 0. {
            return 0;
        }
        self.frame.as_ref().map_or(0, |f| {
            (f.rows as usize).saturating_sub(self.visible_rows(cell))
        })
    }

    /// A non-owner never resizes; an owner sends its grid size whenever it changes.
    pub fn resize_if_owner(&mut self, font: f32) {
        if self.rect[2] <= 0. || self.rect[3] <= 0. {
            return;
        }
        let Metrics { cell, .. } = metrics(font);
        let status = self.status();
        let cols = (self.rect[2] / cell.0).floor().clamp(1., 65535.) as u16;
        let rows = (self.rect[3] / cell.1).floor().clamp(1., 65535.) as u16;
        self.initial_size.layout(
            (cols, rows),
            self.frame.as_ref().map(|frame| (frame.cols, frame.rows)),
        );
        if !status.owner || !status.live {
            self.last_size = None;
        } else if self.last_size != Some((cols, rows)) {
            self.last_size = Some((cols, rows));
            self.send(Command::Resize(
                cols,
                rows,
                cell.0.ceil() as u16,
                cell.1.ceil() as u16,
            ));
        }
    }

    /// The cell index under a point in the grid's own coordinates.
    pub fn index(&self, x: f32, y: f32, cell: (f32, f32)) -> usize {
        let cols = self.frame.as_ref().map_or(1, |f| f.cols) as usize;
        let rows = self.frame.as_ref().map_or(1, |f| f.rows) as usize;
        let col = (x / cell.0).max(0.) as usize;
        let row = (y / cell.1).max(0.) as usize;
        (row + self.display_row(cell)).min(rows - 1) * cols + col.min(cols - 1)
    }

    /// The program has asked for mouse reports.
    pub fn tracking(&self) -> bool {
        self.frame.as_ref().is_some_and(|f| f.mouse_tracking)
    }

    /// The openable link under a point in the grid's own coordinates, with its cell index.
    pub fn link_at(&self, x: f32, y: f32, cell: (f32, f32)) -> Option<(usize, Link)> {
        let index = self.index(x, y, cell);
        self.frame.as_ref()?.link_at(index).map(|l| (index, l))
    }

    /// The server's cell (column, row) under a point in the grid's own coordinates, clamped
    /// to the grid, so a drag past the edge reports the edge cell.
    pub fn cell_at(&self, x: f32, y: f32, cell: (f32, f32)) -> (u16, u16) {
        let cols = self.frame.as_ref().map_or(1, |f| f.cols).max(1);
        let rows = self.frame.as_ref().map_or(1, |f| f.rows).max(1);
        let col = (x / cell.0).floor().max(0.) as usize;
        let row = (y / cell.1).floor().max(0.) as usize + self.display_row(cell);
        (
            col.min(cols as usize - 1) as u16,
            row.min(rows as usize - 1) as u16,
        )
    }

    /// One mouse report. Shift never goes along: it is the local-selection override, and a
    /// Shift press leaves the server's button reporting stuck for the block.
    pub fn send_mouse(&mut self, action: &str, button: Option<&str>, at: (u16, u16), mods: Mods) {
        let mut payload = json!({
            "action": action,
            "x": at.0,
            "y": at.1,
            "mods": keys::mods(Mods { shift: false, ..mods }),
        });
        if let Some(button) = button {
            payload["button"] = json!(button);
        }
        self.mouse_cell = Some(at);
        self.send(Command::Mouse(payload));
    }

    /// A wheel turn of whole lines at a cell: negative scrolls up, positive down.
    pub fn send_wheel(&mut self, at: (u16, u16), lines: i32) {
        let step = lines.signum();
        for _ in 0..lines.unsigned_abs().min(8) {
            self.send(Command::Wheel(
                json!({"x": at.0, "y": at.1, "delta_x": 0, "delta_y": step}),
            ));
        }
    }

    pub fn copy_text(&self) -> Option<String> {
        let (a, b) = self.selection?;
        Some(self.frame.as_ref()?.selected_text(a, b))
    }

    pub fn begin_selection(&mut self, index: usize, clicks: u32) {
        let Some(frame) = &self.frame else {
            return;
        };
        self.selection_mode = match clicks {
            2 => SelectionMode::Word,
            3.. => SelectionMode::Line,
            _ => SelectionMode::Cell,
        };
        let range = match self.selection_mode {
            SelectionMode::Cell => (index, index),
            SelectionMode::Word => frame.word_range(index),
            SelectionMode::Line => frame.line_range(index),
        };
        self.selection_anchor = Some(range);
        self.selection = Some(range);
    }

    pub fn extend_selection(&mut self, index: usize) {
        let (Some(frame), Some(anchor)) = (&self.frame, self.selection_anchor) else {
            return;
        };
        let target = match self.selection_mode {
            SelectionMode::Cell => (index, index),
            SelectionMode::Word => frame.word_range(index),
            SelectionMode::Line => frame.line_range(index),
        };
        self.selection = Some(if target.1 < anchor.0 {
            (anchor.1, target.0)
        } else {
            (anchor.0, target.1)
        });
    }
}

/// How the cursor draws: owning the keyboard (filled), visible but not (hollow), or not at all.
#[derive(Clone, Copy, Hash, PartialEq)]
pub enum CursorLook {
    Block,
    Hollow,
}

fn hash_cell(cell: &Cell, h: &mut DefaultHasher) {
    cell.text.hash(h);
    (cell.fg, cell.bg, cell.underline_color, cell.spacer_tail).hash(h);
    let s = &cell.style;
    (
        s.bold,
        s.italic,
        s.faint,
        s.strikethrough,
        s.underline,
        s.overline,
    )
        .hash(h);
}

#[derive(Clone, Copy, PartialEq)]
struct RunStyle {
    fg: u32,
    bold: bool,
    italic: bool,
    faint: bool,
    underline: ghostty_vt::style::Underline,
    underline_color: u32,
    overline: bool,
    strike: bool,
}
impl RunStyle {
    /// `default_bg` is the server's default background, which draws as `body` wherever it
    /// shows (inverse video's foreground included).
    fn of(cell: &Cell, default_bg: u32, body: u32) -> Self {
        let s = &cell.style;
        let tint = |c: u32| if c == default_bg { body } else { c };
        Self {
            // Faint text is pre-mixed toward its cell in sRGB.
            fg: if s.faint {
                mix(tint(cell.fg), tint(cell.bg), 0.65)
            } else {
                tint(cell.fg)
            },
            bold: s.bold,
            italic: s.italic,
            faint: s.faint,
            underline: s.underline,
            underline_color: tint(cell.underline_color),
            overline: s.overline,
            strike: s.strikethrough,
        }
    }
}

fn glyph_run(content: String, style: RunStyle, x: f32, y: f32, m: &Metrics) -> El {
    let font = face(m, style.bold, style.italic);
    let color = rgb(style.fg);
    let len = content.len();
    let mut e = text(content);
    if style.strike {
        e = e.strike(0, len, Some(color));
    }
    e.no_wrap().font(font).color(color).absolute(x, y)
}

/// (character, bold, italic, size bits) → advances exactly one cell.
type FitCache = std::collections::HashMap<(char, bool, bool, u32), bool>;

thread_local! {
    /// Whether a character, in a given face, advances exactly one cell: only those join
    /// multi-character segments, so nothing after a fallback or wider glyph drifts.
    static FITS: std::cell::RefCell<FitCache> = Default::default();
}

fn face(m: &Metrics, bold: bool, italic: bool) -> TextStyle {
    let mut font = m.style;
    if bold {
        font.weight = 700.;
    }
    if italic {
        font = font.italic();
    }
    font
}

fn fits_cell(ch: char, bold: bool, italic: bool, m: &Metrics) -> bool {
    if ch.is_ascii_graphic() || ch == ' ' {
        return true;
    }
    let key = (ch, bold, italic, m.style.size.to_bits());
    FITS.with_borrow_mut(|cache| {
        *cache.entry(key).or_insert_with(|| {
            let advance = rune::text::advance(&ch.to_string(), face(m, bold, italic));
            (advance - m.cell.0).abs() < 0.01
        })
    })
}

/// One text element for a run of single-cell characters starting at column `col`, with a span
/// per style change, so colours and weights never add elements.
struct Segment {
    col: usize,
    text: String,
    spans: Vec<Span>,
    strikes: Vec<(usize, usize, u32)>,
    style: Option<RunStyle>,
    start: usize,
}

impl Segment {
    fn new(col: usize) -> Self {
        Self {
            col,
            text: String::new(),
            spans: Vec::new(),
            strikes: Vec::new(),
            style: None,
            start: 0,
        }
    }
    fn close_span(&mut self) {
        if let Some(style) = self.style.take() {
            let end = self.text.len();
            if end > self.start {
                let color = rgb(style.fg);
                let mut span = Span::new(self.start, end);
                span.color = Some(color);
                if style.bold {
                    span.weight = Some(700.);
                }
                span.italic = style.italic;
                self.spans.push(span);
                if style.strike {
                    self.strikes.push((self.start, end, style.fg));
                }
            }
        }
    }
    fn push(&mut self, text: &str, style: RunStyle) {
        if self.style != Some(style) {
            self.close_span();
            self.style = Some(style);
            self.start = self.text.len();
        }
        self.text.push_str(text);
    }
    fn finish(mut self, m: &Metrics) -> Option<El> {
        self.close_span();
        // Trailing blanks need no glyph; underline and overline are separate shapes.
        while self.text.ends_with(' ')
            && !self.strikes.iter().any(|&(_, e, _)| e == self.text.len())
        {
            self.text.pop();
            if let Some(last) = self.spans.last_mut() {
                last.end = last.end.min(self.text.len() as u32);
                if last.end <= last.start {
                    self.spans.pop();
                }
            }
        }
        if self.text.trim().is_empty() && self.strikes.is_empty() {
            return None;
        }
        let mut e = rich(self.text, self.spans)
            // The server owns line breaks. Fractional-DPI rounding must not wrap a run again.
            .no_wrap()
            .font(m.style)
            .absolute(self.col as f32 * m.cell.0, 0.);
        for (a, b, fg) in self.strikes {
            e = e.strike(a, b, Some(rgb(fg)));
        }
        Some(e)
    }
}

fn decoration_shape(style: RunStyle, width: f32, height: f32) -> Shape {
    use ghostty_vt::style::Underline;
    let mut shape = Shape::new();
    let color = rgb(style.underline_color);
    let y = height - 2.;
    match style.underline {
        Underline::Single => {
            shape = shape.line([0., y], [width, y], 1., LineCap::Butt, color);
        }
        Underline::Double => {
            shape = shape
                .line([0., y - 2.], [width, y - 2.], 1., LineCap::Butt, color)
                .line([0., y], [width, y], 1., LineCap::Butt, color);
        }
        Underline::Curly => {
            let mut path = Path::new().move_to([0., y]);
            let mut x = 0.;
            while x < width {
                let end = (x + 4.).min(width);
                path = path.quad_to([(x + end) / 2., y + 1.5], [end, y]);
                x = end;
                if x < width {
                    let end = (x + 4.).min(width);
                    path = path.quad_to([(x + end) / 2., y - 1.5], [end, y]);
                    x = end;
                }
            }
            shape = shape.stroke_path(&path, 1., LineCap::Round, color);
        }
        Underline::Dotted => {
            let mut x = 0.;
            while x < width {
                shape = shape.line([x, y], [x + 0.1, y], 1.5, LineCap::Round, color);
                x += 3.;
            }
        }
        Underline::Dashed => {
            let mut x = 0.;
            while x < width {
                shape = shape.line([x, y], [(x + 4.).min(width), y], 1., LineCap::Butt, color);
                x += 7.;
            }
        }
        _ => {}
    }
    if style.overline {
        shape = shape.line([0., 1.], [width, 1.], 1., LineCap::Butt, rgb(style.fg));
    }
    shape
}

fn rect(shape: Shape, x: f32, y: f32, w: f32, h: f32, color: rune::Color) -> Shape {
    shape.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], color)
}

/// Font-independent terminal sprites for characters whose edges must meet cell boundaries.
fn terminal_shape(glyph: char, width: f32, height: f32, color: rune::Color) -> Option<Shape> {
    let mut shape = Shape::new();
    if ('\u{2580}'..='\u{259f}').contains(&glyph) {
        let eighth = height / 8.;
        shape = match glyph {
            '\u{2580}' => rect(shape, 0., 0., width, height / 2., color),
            '\u{2581}'..='\u{2587}' => {
                let n = glyph as u32 - 0x2580;
                rect(
                    shape,
                    0.,
                    height - n as f32 * eighth,
                    width,
                    n as f32 * eighth,
                    color,
                )
            }
            '\u{2588}' => rect(shape, 0., 0., width, height, color),
            '\u{2589}'..='\u{258f}' => {
                let n = 0x2590 - glyph as u32;
                rect(shape, 0., 0., width * n as f32 / 8., height, color)
            }
            '\u{2590}' => rect(shape, width / 2., 0., width / 2., height, color),
            '\u{2591}' => rect(shape, 0., 0., width, height, color.alpha(0.25)),
            '\u{2592}' => rect(shape, 0., 0., width, height, color.alpha(0.5)),
            '\u{2593}' => rect(shape, 0., 0., width, height, color.alpha(0.75)),
            '\u{2594}' => rect(shape, 0., 0., width, eighth, color),
            '\u{2595}' => rect(shape, width * 7. / 8., 0., width / 8., height, color),
            quadrant => {
                let bits = match quadrant {
                    '\u{2596}' => 0b0100,
                    '\u{2597}' => 0b1000,
                    '\u{2598}' => 0b0001,
                    '\u{2599}' => 0b1101,
                    '\u{259a}' => 0b1001,
                    '\u{259b}' => 0b0111,
                    '\u{259c}' => 0b1011,
                    '\u{259d}' => 0b0010,
                    '\u{259e}' => 0b0110,
                    '\u{259f}' => 0b1110,
                    _ => 0,
                };
                let mut result = shape;
                for (bit, x, y) in [
                    (1, 0., 0.),
                    (2, width / 2., 0.),
                    (4, 0., height / 2.),
                    (8, width / 2., height / 2.),
                ] {
                    if bits & bit != 0 {
                        result = rect(result, x, y, width / 2., height / 2., color);
                    }
                }
                result
            }
        };
        return Some(shape);
    }
    if !('\u{2500}'..='\u{257f}').contains(&glyph) {
        return None;
    }
    if matches!(glyph, '\u{2571}' | '\u{2572}' | '\u{2573}') {
        if matches!(glyph, '\u{2571}' | '\u{2573}') {
            shape = shape.line([width, 0.], [0., height], 1., LineCap::Butt, color);
        }
        if matches!(glyph, '\u{2572}' | '\u{2573}') {
            shape = shape.line([0., 0.], [width, height], 1., LineCap::Butt, color);
        }
        return Some(shape);
    }
    if glyph == '═' {
        return Some(rect(
            rect(shape, 0., height / 2. - 2., width, 1., color),
            0.,
            height / 2. + 1.,
            width,
            1.,
            color,
        ));
    }
    if glyph == '║' {
        return Some(rect(
            rect(shape, width / 2. - 2., 0., 1., height, color),
            width / 2. + 1.,
            0.,
            1.,
            height,
            color,
        ));
    }
    let horizontal = "─━╌╍┄┅┈┉═";
    let vertical = "│┃╎╏┆┇┊┋║";
    let right_down = "┌┍┎┏╒╓╔╭";
    let left_down = "┐┑┒┓╕╖╗╮";
    let right_up = "└┕┖┗╘╙╚╰";
    let left_up = "┘┙┚┛╛╜╝╯";
    let right_up_down = "├┝┞┟┠┡┢┣╞╟╠";
    let left_up_down = "┤┥┦┧┨┩┪┫╡╢╣";
    let left_right_down = "┬┭┮┯┰┱┲┳╤╥╦";
    let left_right_up = "┴┵┶┷┸┹┺┻╧╨╩";
    let all = "┼┽┾┿╀╁╂╃╄╅╆╇╈╉╊╋╪╫╬";
    let left = horizontal.contains(glyph)
        || left_down.contains(glyph)
        || left_up.contains(glyph)
        || left_up_down.contains(glyph)
        || left_right_down.contains(glyph)
        || left_right_up.contains(glyph)
        || all.contains(glyph);
    let right = horizontal.contains(glyph)
        || right_down.contains(glyph)
        || right_up.contains(glyph)
        || right_up_down.contains(glyph)
        || left_right_down.contains(glyph)
        || left_right_up.contains(glyph)
        || all.contains(glyph);
    let up = vertical.contains(glyph)
        || right_up.contains(glyph)
        || left_up.contains(glyph)
        || right_up_down.contains(glyph)
        || left_up_down.contains(glyph)
        || left_right_up.contains(glyph)
        || all.contains(glyph);
    let down = vertical.contains(glyph)
        || right_down.contains(glyph)
        || left_down.contains(glyph)
        || right_up_down.contains(glyph)
        || left_up_down.contains(glyph)
        || left_right_down.contains(glyph)
        || all.contains(glyph);
    if !(left || right || up || down) {
        return None;
    }
    let stroke = if "━┃┅┇┉┋┏┓┗┛┣┫┳┻╋".contains(glyph) {
        2.
    } else {
        1.
    };
    let center = [width / 2., height / 2.];
    let double = "═║╒╓╔╕╖╗╘╙╚╛╜╝╞╟╠╡╢╣╤╥╦╧╨╩╪╫╬".contains(glyph);
    for (draw, end) in [
        (left, [0., center[1]]),
        (right, [width, center[1]]),
        (up, [center[0], 0.]),
        (down, [center[0], height]),
    ] {
        if draw && double {
            if end[1] == center[1] {
                shape = shape
                    .line(
                        [center[0], center[1] - 1.5],
                        [end[0], end[1] - 1.5],
                        1.,
                        LineCap::Butt,
                        color,
                    )
                    .line(
                        [center[0], center[1] + 1.5],
                        [end[0], end[1] + 1.5],
                        1.,
                        LineCap::Butt,
                        color,
                    );
            } else {
                shape = shape
                    .line(
                        [center[0] - 1.5, center[1]],
                        [end[0] - 1.5, end[1]],
                        1.,
                        LineCap::Butt,
                        color,
                    )
                    .line(
                        [center[0] + 1.5, center[1]],
                        [end[0] + 1.5, end[1]],
                        1.,
                        LineCap::Butt,
                        color,
                    );
            }
        } else if draw {
            shape = shape.line(center, end, stroke, LineCap::Butt, color);
        }
    }
    Some(shape)
}

fn row_el(
    cells: &[Cell],
    y: f32,
    frame_bg: u32,
    sel: Option<(usize, usize)>,
    search: &[(usize, usize, bool)],
    m: &Metrics,
    colors: &TermColors,
) -> El {
    let body = colors.body(frame_bg);
    let (cw, ch) = m.cell;
    // Memoized rows are flattened on their own, so each says again that it never animates:
    // otherwise a glyph run that leaves a changed row fades out over its replacement.
    let mut row = el().w_full().h(ch).absolute(0., y).instant_changes();
    // Backgrounds, then selection, then glyphs.
    let mut start = 0;
    while start < cells.len() {
        let bg = cells[start].bg;
        let mut end = start + 1;
        while end < cells.len() && cells[end].bg == bg {
            end += 1;
        }
        if bg != frame_bg {
            row = row.child(
                el().absolute(start as f32 * cw, 0.)
                    .size((end - start) as f32 * cw, ch)
                    .fill(rgb(bg)),
            );
        }
        start = end;
    }
    for &(a, b, selected) in search {
        row = row.child(
            el().absolute(a as f32 * cw, 0.)
                .size((b + 1 - a) as f32 * cw, ch)
                .fill(rgb(ACCENT).alpha(if selected { 0.55 } else { 0.25 })),
        );
    }
    if let Some((a, b)) = sel {
        row = row.child(
            el().absolute(a as f32 * cw, 0.)
                .size((b + 1 - a) as f32 * cw, ch)
                .fill(rgb(colors.selection_bg)),
        );
    }
    let mut segment: Option<Segment> = None;
    for (col, cell) in cells.iter().enumerate() {
        if cell.spacer_tail {
            continue;
        }
        let mut style = RunStyle::of(cell, frame_bg, body);
        if let (Some(fg), Some((a, b))) = (colors.selection_fg, sel)
            && (a..=b).contains(&col)
        {
            style.fg = fg;
        }
        let text = if cell.text.is_empty() {
            " "
        } else {
            cell.text.as_str()
        };
        let mut chars = text.chars();
        let single = chars.next().filter(|_| chars.next().is_none());
        let wide = cells.get(col + 1).is_some_and(|c| c.spacer_tail);
        if let Some(glyph) = single
            && let Some(shape) =
                terminal_shape(glyph, if wide { cw * 2. } else { cw }, ch, rgb(style.fg))
        {
            if let Some(done) = segment.take().and_then(|s| s.finish(m)) {
                row = row.child(done);
            }
            row = row.child(
                canvas(shape)
                    .absolute(col as f32 * cw, 0.)
                    .size(if wide { cw * 2. } else { cw }, ch),
            );
            continue;
        }
        let fits = !wide && single.is_some_and(|c| fits_cell(c, style.bold, style.italic, m));
        if fits {
            if text == " "
                && segment.is_none()
                && style.underline == ghostty_vt::style::Underline::None
                && !style.overline
                && !style.strike
            {
                continue;
            }
            segment
                .get_or_insert_with(|| Segment::new(col))
                .push(text, style);
        } else {
            if let Some(done) = segment.take().and_then(|s| s.finish(m)) {
                row = row.child(done);
            }
            row = row.child(glyph_run(text.to_owned(), style, col as f32 * cw, 0., m));
        }
    }
    if let Some(done) = segment.and_then(|s| s.finish(m)) {
        row = row.child(done);
    }
    // Terminal decorations use their requested style and colour rather than the font's
    // single-underline implementation.
    let mut start = 0;
    while start < cells.len() {
        let style = RunStyle::of(&cells[start], frame_bg, body);
        let mut end = start + 1;
        while end < cells.len() {
            let next = RunStyle::of(&cells[end], frame_bg, body);
            if next.underline != style.underline
                || next.underline_color != style.underline_color
                || next.overline != style.overline
                || next.fg != style.fg
            {
                break;
            }
            end += 1;
        }
        let shape = decoration_shape(style, (end - start) as f32 * cw, ch);
        if !shape.is_empty() {
            row = row.child(
                canvas(shape)
                    .absolute(start as f32 * cw, 0.)
                    .size((end - start) as f32 * cw, ch),
            );
        }
        start = end;
    }
    row
}

pub struct GridLook {
    pub cursor: Option<CursorLook>,
    pub ime: bool,
    pub colors: TermColors,
}

/// The colours the terminal area is drawn in, from the chosen terminal theme. The theme's
/// default colours and palette reach the frame through the server; these are the ones the
/// client draws itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TermColors {
    pub background: u32,
    pub selection_bg: u32,
    pub selection_fg: Option<u32>,
    pub cursor_text: Option<u32>,
}
impl TermColors {
    pub fn of(theme: &TerminalTheme) -> Self {
        Self {
            background: theme.background,
            selection_bg: theme.selection_background.unwrap_or(TERMINAL_SELECTION),
            selection_fg: theme.selection_foreground,
            cursor_text: theme.cursor_text,
        }
    }

    /// What the server's default background `frame_bg` draws as: the theme's background
    /// when both are equally light or dark (the server's built-in black, or another owner's
    /// theme), otherwise the server's own, so default text drawn for it stays readable.
    pub fn body(&self, frame_bg: u32) -> u32 {
        if is_light(frame_bg) == is_light(self.background) {
            self.background
        } else {
            frame_bg
        }
    }
}

impl Term {
    /// The fill behind this pane's grid.
    pub fn body(&self, colors: &TermColors) -> u32 {
        self.display_frame()
            .map_or(colors.background, |f| colors.body(f.bg))
    }
}

/// The visible rows of `term`'s frame, each memoized by its content.
pub fn grid(
    key: &str,
    term: &Term,
    m: &Metrics,
    images: Option<&rune::ImageCache>,
    look: GridLook,
) -> El {
    let Some(frame) = term.display_frame() else {
        return el().w_full().h_full();
    };
    let (cw, ch) = m.cell;
    let visible = term.visible_rows(m.cell);
    let start = frame.display_row_start(visible);
    let cols = frame.cols as usize;
    let body = look.colors.body(frame.bg);
    let selection = term.selection.map(|(a, b)| (a.min(b), a.max(b)));
    // A size another client owns can be wider or taller than this grid: crop it here, not at
    // the card's edge, so it never covers the padding or the card's border.
    let mut grid = el().w_full().h_full().instant_changes().clip_rounded(0.);
    let add_images = |mut grid: El, above: bool| {
        let Some(cache) = images else { return grid };
        for placement in frame.images.iter().filter(|image| (image.z >= 0) == above) {
            let image = cache.rgba(
                placement.source_size.0,
                placement.source_size.1,
                placement.rgba.clone(),
            );
            let cell_px = (
                placement.cell_size_px.0.max(1.),
                placement.cell_size_px.1.max(1.),
            );
            let x = placement.col as f32 * cw + placement.x_offset_px as f32 / cell_px.0 * cw;
            let y = (placement.row as f32 - start as f32) * ch
                + placement.y_offset_px as f32 / cell_px.1 * ch;
            let width = placement.pixel_size.0 as f32 / cell_px.0 * cw;
            let height = placement.pixel_size.1 as f32 / cell_px.1 * ch;
            grid = grid.child(
                el().key(format!("{key}/image/{}-{}", placement.key, placement.z))
                    .absolute(x, y)
                    .size(width, height)
                    .pixels(image.pixels()),
            );
        }
        grid
    };
    grid = add_images(grid, false);
    for (y, cells) in frame.cells.iter().enumerate().skip(start).take(visible) {
        let sel = selection.and_then(|(a, b)| {
            let (row0, row1) = (y * cols, y * cols + cols - 1);
            (a <= row1 && b >= row0).then(|| (a.max(row0) - row0, b.min(row1) - row0))
        });
        let search: Vec<_> = frame
            .search_matches
            .iter()
            .filter_map(|found| {
                let (y0, y1) = (found.start.1 as usize, found.end.1 as usize);
                (y0 <= y && y <= y1).then(|| {
                    (
                        if y == y0 { found.start.0 as usize } else { 0 },
                        if y == y1 {
                            found.end.0 as usize
                        } else {
                            cols - 1
                        },
                        frame.search_selected == Some(*found),
                    )
                })
            })
            .collect();
        let mut h = DefaultHasher::new();
        for cell in cells {
            hash_cell(cell, &mut h);
        }
        (
            sel,
            &search,
            frame.bg,
            cw.to_bits(),
            ch.to_bits(),
            m.style,
            look.colors,
        )
            .hash(&mut h);
        let top = (y - start) as f32 * ch;
        let id = format!("{key}/r{}", y - start);
        grid = grid.child(memo(id, h.finish(), || {
            row_el(cells, top, frame.bg, sel, &search, m, &look.colors)
        }));
    }
    grid = add_images(grid, true);
    // The link Ctrl+click would open, underlined in its own colour, row by row.
    if let Some((_, link)) = &term.link {
        let color = frame
            .cells
            .get(link.start / cols)
            .and_then(|row| row.get(link.start % cols))
            .map_or(frame.fg, |c| RunStyle::of(c, frame.bg, body).fg);
        for y in
            (link.start / cols..=link.end / cols).filter(|y| (start..start + visible).contains(y))
        {
            let c0 = if y == link.start / cols {
                link.start % cols
            } else {
                0
            };
            let c1 = if y == link.end / cols {
                link.end % cols
            } else {
                cols - 1
            };
            grid = grid.child(
                el().key(format!("{key}/link{y}"))
                    .absolute(c0 as f32 * cw, (y - start) as f32 * ch + ch - 2.)
                    .size((c1 + 1 - c0) as f32 * cw, 1.)
                    .fill(rgb(color)),
            );
        }
    }
    if let Some((x, y)) = frame.cursor
        && let Some(cursor_look) = look.cursor
        && (y as usize) >= start
        && (y as usize) < start + visible
    {
        let (px, py) = (x as f32 * cw, (y as usize - start) as f32 * ch);
        let cell = frame.cells.get(y as usize).and_then(|r| r.get(x as usize));
        let color = rgb(frame.cursor_color);
        let requested = if cursor_look == CursorLook::Hollow {
            CursorShape::Hollow
        } else {
            frame.cursor_shape
        };
        // Blink state is decoded, but terminal content remains static by product design.
        let (cursor, filled) = match requested {
            CursorShape::Bar => (
                el().key(format!("{key}/cursor"))
                    .absolute(px, py)
                    .size(2., ch)
                    .fill(color),
                false,
            ),
            CursorShape::Underline => (
                el().key(format!("{key}/cursor"))
                    .absolute(px, py + ch - 2.)
                    .size(cw, 2.)
                    .fill(color),
                false,
            ),
            CursorShape::Hollow => (
                el().key(format!("{key}/cursor"))
                    .absolute(px, py)
                    .size(cw, ch)
                    .border(1., color),
                false,
            ),
            CursorShape::Block => (
                el().key(format!("{key}/cursor"))
                    .absolute(px, py)
                    .size(cw, ch)
                    .fill(color),
                true,
            ),
        };
        let cursor = if look.ime {
            cursor.ime_cursor()
        } else {
            cursor
        };
        grid = grid.child(cursor);
        if filled && let Some(cell) = cell.filter(|c| !c.text.is_empty()) {
            let style = RunStyle {
                fg: look.colors.cursor_text.unwrap_or(body),
                ..RunStyle::of(cell, frame.bg, body)
            };
            grid = grid.child(glyph_run(cell.text.clone(), style, px, py, m));
        }
        if !term.preedit.is_empty() {
            let len = term.preedit.len();
            grid = grid.child(
                rich(
                    term.preedit.clone(),
                    vec![Span {
                        underline: Some(rgb(frame.fg)),
                        ..Span::new(0, len)
                    }],
                )
                .no_wrap()
                .font(m.style)
                .color(rgb(frame.fg))
                .fill(rgb(body))
                .absolute(px, py),
            );
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Cx, Env, Runtime};

    struct RowState {
        cells: Vec<Cell>,
    }
    fn view(s: &RowState, _: &mut Cx<RowState>) -> El {
        let m = metrics(15.);
        el().w_full().h_full().child(row_el(
            &s.cells,
            0.,
            0,
            None,
            &[],
            &m,
            &TermColors::of(&TerminalTheme::builtin()),
        ))
    }

    #[test]
    fn the_default_background_takes_the_theme_only_on_its_own_side() {
        let builtin = TermColors::of(&TerminalTheme::builtin());
        let ground = builtin.background;
        assert_eq!(builtin.body(0x000000), ground, "the server's black");
        assert_eq!(builtin.body(0x1e1e2e), ground, "another owner's dark theme");
        assert_eq!(
            builtin.body(0xfdf6e3),
            0xfdf6e3,
            "another owner's light theme"
        );
        let light = TermColors::of(&TerminalTheme {
            background: 0xfdf6e3,
            ..TerminalTheme::builtin()
        });
        assert_eq!(light.body(0xfdf6e3), 0xfdf6e3);
        assert_eq!(light.body(0xeeeeee), 0xfdf6e3);
        assert_eq!(
            light.body(0x000000),
            0x000000,
            "a server without set_theme keeps its black under its light default text"
        );
    }

    #[test]
    fn startup_waits_for_the_server_frame_at_the_layout_size_only_once() {
        let mut size = InitialSize::default();
        assert!(
            !size.renders(true),
            "a live stale frame is hidden before layout"
        );
        assert!(size.renders(false), "retained output stays visible offline");

        size.layout((80, 24), Some((160, 50)));
        assert!(
            !size.renders(true),
            "the stale startup geometry stays hidden"
        );
        size.observe(Some((80, 24)));
        assert!(size.renders(true), "the acknowledged frame becomes visible");

        size.layout((100, 30), Some((80, 24)));
        assert!(
            size.renders(true),
            "later window resizes keep the previous frame visible"
        );
    }
    #[test]
    fn typing_styled_terminal_runs_never_creates_a_second_ui_line() {
        let mut terminal = ghostty_vt::Terminal::new(ghostty_vt::TerminalOptions {
            cols: 253,
            rows: 4,
            ..Default::default()
        })
        .unwrap();
        let mut renderer = rex_linux::terminal::Renderer::new().unwrap();
        let fonts = crate::app::fonts::load("JetBrainsMono Nerd Font");
        let mut rt = Runtime::with_text(
            RowState { cells: vec![] },
            view,
            (2300., 100.),
            Env::new(true, 1.5),
            rune::text::TextSystem::new(fonts.files, fonts.families),
        );
        // Shell prompts mix fallback glyphs, styles and ASCII. Rebuild on every keystroke,
        // including runs preceded by blanks, at the user's fractional display scale.
        for n in 1..80 {
            terminal.vt_write(
                format!(
                    "\x1b[2J\x1b[H\u{f179} \u{e0b1} \x1b[36m\u{f015}~\x1b[0m   {}",
                    "a".repeat(n)
                )
                .as_bytes(),
            );
            let frame = renderer.frame(&terminal).unwrap();
            rt.update(|s, _| s.cells = frame.cells[0].clone());
            rt.frame(1. / 60.);
            for e in rt.elements().into_iter().filter(|e| e.text.is_some()) {
                assert!(
                    e.rect[3] <= 22.,
                    "keystroke {n}: {:?} wraps to {:?}",
                    e.text,
                    e.rect
                );
            }
            for q in rt.instances.iter().filter(|q| rune::compose::is_glyph(q)) {
                assert!(
                    q.rect[1] + q.rect[3] <= 33.,
                    "keystroke {n}: glyph drawn on the next line: {:?}",
                    q.rect
                );
            }
        }
    }
}
