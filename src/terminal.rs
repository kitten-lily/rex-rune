use crate::wire::SnapshotBuffer;
use anyhow::{Result, ensure};
use ghostty_vt::{
    RenderState, Terminal,
    render::{CellIterator, RowIterator},
    search::Search,
    style::RgbColor,
    terminal::{Point, PointCoordinate, PointSpace},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Output(Vec<u8>),
    Metadata(Value),
}

/// Retains partial escape prefixes across frames and reconnects. Only a Rex
/// APC with this channel's nonce is interpreted as transport metadata.
pub struct ApcParser {
    prefix: Vec<u8>,
    pending: Vec<u8>,
}
impl ApcParser {
    pub fn new(nonce: u64) -> Self {
        Self {
            prefix: format!("\x1b_rex1;{nonce};").into_bytes(),
            pending: Vec::new(),
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Part>> {
        let mut parts = Vec::new();
        let mut output = Vec::new();
        for &byte in bytes {
            self.pending.push(byte);
            loop {
                if self.pending.starts_with(&self.prefix) {
                    ensure!(self.pending.len() <= 1024 * 1024, "Rex APC exceeds 1 MiB");
                    if self.pending.ends_with(b"\x1b\\") {
                        if !output.is_empty() {
                            parts.push(Part::Output(std::mem::take(&mut output)));
                        }
                        let json = &self.pending[self.prefix.len()..self.pending.len() - 2];
                        parts.push(Part::Metadata(serde_json::from_slice(json)?));
                        self.pending.clear();
                    }
                    break;
                }
                if self.prefix.starts_with(&self.pending) {
                    break;
                }
                output.push(self.pending.remove(0));
            }
        }
        if !output.is_empty() {
            parts.push(Part::Output(output));
        }
        Ok(parts)
    }
}

pub struct TerminalStream {
    pub terminal: Option<Terminal>,
    snapshot: SnapshotBuffer,
    parser: ApcParser,
    pub received: u64,
    pub snapshot_count: usize,
    pub exit_code: Option<i64>,
}
impl TerminalStream {
    pub fn new(nonce: u64) -> Self {
        Self {
            terminal: None,
            snapshot: SnapshotBuffer::default(),
            parser: ApcParser::new(nonce),
            received: 0,
            snapshot_count: 0,
            exit_code: None,
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        self.received += bytes.len() as u64;
        let tail;
        let bytes = if self.terminal.is_none() {
            let Some((mut terminal, rest)) = self.snapshot.push(bytes)? else {
                return Ok(());
            };
            terminal.set_kitty_image_storage_limit(64 * 1024 * 1024)?;
            self.terminal = Some(terminal);
            self.snapshot_count += 1;
            tail = rest;
            &tail
        } else {
            bytes
        };
        let terminal = self.terminal.as_mut().expect("snapshot installed terminal");
        for part in self.parser.feed(bytes)? {
            match part {
                Part::Output(bytes) => terminal.vt_write(&bytes),
                Part::Metadata(value) => match value["type"].as_str() {
                    Some("resize") => {
                        let cols = value["cols"]
                            .as_u64()
                            .filter(|v| (1..=65535).contains(v))
                            .ok_or_else(|| anyhow::anyhow!("invalid resize columns"))?
                            as u16;
                        let rows = value["rows"]
                            .as_u64()
                            .filter(|v| (1..=65535).contains(v))
                            .ok_or_else(|| anyhow::anyhow!("invalid resize rows"))?
                            as u16;
                        terminal.resize(
                            cols,
                            rows,
                            value["cell_width_px"].as_u64().unwrap_or(0) as u32,
                            value["cell_height_px"].as_u64().unwrap_or(0) as u32,
                        )?;
                    }
                    Some("theme") => {
                        terminal.set_default_fg_color(parse_color(value["foreground"].as_str()))?;
                        terminal.set_default_bg_color(parse_color(value["background"].as_str()))?;
                        terminal.set_default_cursor_color(parse_color(value["cursor"].as_str()))?;
                        // Each theme replaces the previous palette, including omissions.
                        terminal.set_default_color_palette(None)?;
                        if let Some(entries) = value["palette"].as_array() {
                            let mut palette = terminal.color_palette()?;
                            for entry in entries.iter().filter_map(Value::as_str) {
                                if let Some((index, color)) = entry.split_once('=')
                                    && let (Ok(index), Some(color)) =
                                        (index.parse::<u8>(), parse_color(Some(color)))
                                {
                                    palette.0[index as usize] = color;
                                }
                            }
                            terminal.set_default_color_palette(Some(palette))?;
                        }
                    }
                    Some("child_exited") => self.exit_code = value["exit_code"].as_i64(),
                    _ => {}
                },
            }
        }
        Ok(())
    }
    pub fn text(&self) -> Result<String> {
        let terminal = self
            .terminal
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("snapshot not ready"))?;
        let mut formatter = ghostty_vt::fmt::Formatter::new(
            terminal,
            ghostty_vt::fmt::FormatterOptions::default(),
        )?;
        Ok(String::from_utf8_lossy(&formatter.format_alloc()?).into_owned())
    }
}

fn parse_color(value: Option<&str>) -> Option<RgbColor> {
    let value = value?.trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let color = u32::from_str_radix(value, 16).ok()?;
    Some(RgbColor {
        r: (color >> 16) as u8,
        g: (color >> 8) as u8,
        b: color as u8,
    })
}
pub fn rgb(color: RgbColor) -> u32 {
    (color.r as u32) << 16 | (color.g as u32) << 8 | color.b as u32
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CursorShape {
    Bar,
    Block,
    Underline,
    Hollow,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct SearchMatch {
    pub start: (u16, u32),
    pub end: (u16, u32),
}

#[derive(Clone, Debug)]
pub struct KittyPlacement {
    pub key: u64,
    pub rgba: Arc<[u8]>,
    pub source_size: (u32, u32),
    pub col: i32,
    pub row: i32,
    pub x_offset_px: u32,
    pub y_offset_px: u32,
    pub pixel_size: (u32, u32),
    pub cell_size_px: (f32, f32),
    pub z: i32,
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub text: String,
    pub spacer_tail: bool,
    pub fg: u32,
    pub bg: u32,
    pub underline_color: u32,
    pub style: ghostty_vt::style::Style,
    /// The cell's OSC 8 hyperlink target.
    pub link: Option<Arc<str>>,
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Vec<Cell>>,
    pub fg: u32,
    pub bg: u32,
    pub cursor: Option<(u16, u16)>,
    pub cursor_color: u32,
    pub cursor_shape: CursorShape,
    pub cursor_blinking: bool,
    /// The program asked for mouse reports (DEC 9, 1000, 1002 or 1003).
    pub mouse_tracking: bool,
    /// Per row: the row soft-wraps into the next one.
    pub wrapped: Vec<bool>,
    pub search_matches: Vec<SearchMatch>,
    pub search_selected: Option<SearchMatch>,
    pub search_index: Option<usize>,
    pub search_total: usize,
    pub images: Vec<KittyPlacement>,
}

impl Frame {
    /// Local viewport cropping must retain the newest rows, or the cursor when
    /// a TUI has positioned it above them. This never changes the server size.
    pub fn display_row_start(&self, visible_rows: usize) -> usize {
        let bottom = (self.rows as usize).saturating_sub(visible_rows.max(1));
        self.cursor.map_or(bottom, |(_, y)| bottom.min(y as usize))
    }

    pub fn selected_text(&self, a: usize, b: usize) -> String {
        let (start, end) = (a.min(b), a.max(b));
        let cols = self.cols as usize;
        let mut lines = Vec::new();
        for row in start / cols..=end / cols {
            let Some(cells) = self.cells.get(row) else {
                break;
            };
            let mut line = String::new();
            for (col, cell) in cells.iter().enumerate() {
                let index = row * cols + col;
                if index >= start && index <= end && !cell.spacer_tail {
                    line.push_str(if cell.text.is_empty() {
                        " "
                    } else {
                        &cell.text
                    });
                }
            }
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
    }

    pub fn word_range(&self, index: usize) -> (usize, usize) {
        let cols = self.cols as usize;
        let row = index / cols;
        let cells = match self.cells.get(row) {
            Some(cells) => cells,
            None => return (index, index),
        };
        let mut col = (index % cols).min(cells.len().saturating_sub(1));
        if cells.get(col).is_some_and(|cell| cell.spacer_tail) && col > 0 {
            col -= 1;
        }
        let class = |cell: &Cell| {
            let ch = cell.text.chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                0
            } else if ch.is_alphanumeric() || ch == '_' {
                1
            } else {
                2
            }
        };
        let wanted = class(&cells[col]);
        let mut start = col;
        while start > 0 && class(&cells[start - 1]) == wanted {
            start -= 1;
        }
        let mut end = col;
        while end + 1 < cells.len() && class(&cells[end + 1]) == wanted {
            end += 1;
        }
        (row * cols + start, row * cols + end)
    }

    pub fn line_range(&self, index: usize) -> (usize, usize) {
        let cols = self.cols as usize;
        let mut first = (index / cols).min(self.cells.len().saturating_sub(1));
        while first > 0 && self.wrapped.get(first - 1).copied().unwrap_or(false) {
            first -= 1;
        }
        let mut last = index / cols;
        while last + 1 < self.cells.len() && self.wrapped.get(last).copied().unwrap_or(false) {
            last += 1;
        }
        (first * cols, (last + 1) * cols - 1)
    }
}
pub struct Renderer {
    state: RenderState,
    rows: RowIterator,
    cells: CellIterator,
    placements: ghostty_vt::kitty::graphics::PlacementIterator,
    image_cache: HashMap<ImageKey, Arc<[u8]>>,
}
impl Renderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            placements: ghostty_vt::kitty::graphics::PlacementIterator::new()?,
            image_cache: HashMap::new(),
        })
    }
    pub fn frame(&mut self, terminal: &Terminal) -> Result<Frame> {
        self.frame_with_search(terminal, None)
    }

    pub fn frame_with_search(
        &mut self,
        terminal: &Terminal,
        search: Option<&Search>,
    ) -> Result<Frame> {
        let snapshot = self.state.update(terminal)?;
        let colors = snapshot.colors()?;
        let fg = rgb(colors.foreground);
        let bg = rgb(colors.background);
        let mut frame = Frame {
            cols: snapshot.cols()?,
            rows: snapshot.rows()?,
            cells: Vec::new(),
            fg,
            bg,
            cursor: if snapshot.cursor_visible()? {
                snapshot.cursor_viewport()?.map(|p| (p.x, p.y))
            } else {
                None
            },
            cursor_color: snapshot.cursor_color()?.map(rgb).unwrap_or(fg),
            cursor_shape: match snapshot.cursor_visual_style()? {
                ghostty_vt::render::CursorVisualStyle::Bar => CursorShape::Bar,
                ghostty_vt::render::CursorVisualStyle::Underline => CursorShape::Underline,
                ghostty_vt::render::CursorVisualStyle::BlockHollow => CursorShape::Hollow,
                _ => CursorShape::Block,
            },
            cursor_blinking: snapshot.cursor_blinking()?,
            mouse_tracking: terminal.is_mouse_tracking()?,
            wrapped: Vec::new(),
            search_matches: Vec::new(),
            search_selected: None,
            search_index: None,
            search_total: 0,
            images: Vec::new(),
        };
        let mut uri = Vec::new();
        let mut rows = self.rows.update(&snapshot)?;
        let mut y = 0;
        while let Some(row) = rows.next() {
            let raw_row = row.raw_row()?;
            frame.wrapped.push(raw_row.is_wrapped()?);
            let row_links = raw_row.has_hyperlink()?;
            let mut cells = self.cells.update(row)?;
            let mut line: Vec<Cell> = Vec::new();
            while let Some(cell) = cells.next() {
                let mut text = String::new();
                cell.graphemes_utf8(&mut text)?;
                let style = cell.style()?;
                let mut fg = cell.fg_color()?.map(rgb).unwrap_or(fg);
                let mut bg = cell.bg_color()?.map(rgb).unwrap_or(bg);
                if style.inverse {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if style.invisible {
                    text.clear();
                }
                let raw = cell.raw_cell()?;
                let link = if row_links && raw.has_hyperlink()? {
                    let at = (line.len() as u16, y);
                    hyperlink(terminal, at, &mut uri)?.then(|| {
                        // Neighbouring cells of one link share its string.
                        match line.last().and_then(|c| c.link.clone()) {
                            Some(prev) if prev.as_bytes() == uri => prev,
                            _ => Arc::from(String::from_utf8_lossy(&uri)),
                        }
                    })
                } else {
                    None
                };
                let underline_color = match style.underline_color {
                    ghostty_vt::style::StyleColor::Rgb(color) => rgb(color),
                    ghostty_vt::style::StyleColor::Palette(index) => {
                        rgb(colors.palette[index.0 as usize])
                    }
                    ghostty_vt::style::StyleColor::None => fg,
                };
                line.push(Cell {
                    text,
                    spacer_tail: raw.wide()? == ghostty_vt::screen::CellWide::SpacerTail,
                    fg,
                    bg,
                    underline_color,
                    style,
                    link,
                });
            }
            frame.cells.push(line);
            y += 1;
        }
        if let Some(search) = search {
            frame.search_total = search.total_matches()?;
            frame.search_index = search.selected_index()?;
            for selection in search.viewport_matches(terminal)? {
                if let Some(found) = search_match(terminal, &selection)? {
                    frame.search_matches.push(found);
                }
            }
            frame.search_selected = search
                .selected_match(terminal)?
                .as_ref()
                .map(|selection| search_match(terminal, selection))
                .transpose()?
                .flatten();
        }
        render_kitty(
            terminal,
            &mut self.placements,
            &mut self.image_cache,
            &mut frame,
        )?;
        snapshot.clean()?;
        Ok(frame)
    }
}

fn search_match(
    terminal: &Terminal,
    selection: &ghostty_vt::selection::Selection<'_>,
) -> Result<Option<SearchMatch>> {
    let Some(start) = terminal.point_from_grid_ref(&selection.start(), PointSpace::Viewport)?
    else {
        return Ok(None);
    };
    let Some(end) = terminal.point_from_grid_ref(&selection.end(), PointSpace::Viewport)? else {
        return Ok(None);
    };
    Ok(Some(SearchMatch {
        start: (start.x, start.y),
        end: (end.x, end.y),
    }))
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct ImageKey {
    generation: u64,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn render_kitty(
    terminal: &Terminal,
    placements: &mut ghostty_vt::kitty::graphics::PlacementIterator,
    cache: &mut HashMap<ImageKey, Arc<[u8]>>,
    frame: &mut Frame,
) -> Result<()> {
    use ghostty_vt::kitty::graphics::ImageFormat;
    let graphics = terminal.kitty_graphics()?;
    let mut used = HashSet::new();
    let mut iteration = placements.update(&graphics)?;
    while iteration.next().is_some() {
        let Some(image) = graphics.image(iteration.image_id()?) else {
            continue;
        };
        let info = iteration.placement_render_info(&image, terminal)?;
        if !info.viewport_visible || info.source_width == 0 || info.source_height == 0 {
            continue;
        }
        let key = ImageKey {
            generation: image.generation()?,
            x: info.source_x,
            y: info.source_y,
            width: info.source_width,
            height: info.source_height,
        };
        used.insert(key);
        let rgba = if let Some(rgba) = cache.get(&key) {
            rgba.clone()
        } else {
            let image_width = image.width()?;
            let image_height = image.height()?;
            let bytes = image.data()?;
            let channels = match image.format()? {
                ImageFormat::Rgb => 3,
                ImageFormat::Rgba | ImageFormat::Png => 4,
                ImageFormat::GrayAlpha => 2,
                ImageFormat::Gray => 1,
                _ => continue,
            };
            let Some(expected) = (image_width as usize)
                .checked_mul(image_height as usize)
                .and_then(|pixels| pixels.checked_mul(channels))
            else {
                continue;
            };
            if bytes.len() < expected
                || info.source_x.saturating_add(info.source_width) > image_width
                || info.source_y.saturating_add(info.source_height) > image_height
            {
                continue;
            }
            let mut out =
                Vec::with_capacity(info.source_width as usize * info.source_height as usize * 4);
            for y in info.source_y..info.source_y + info.source_height {
                for x in info.source_x..info.source_x + info.source_width {
                    let at = (y as usize * image_width as usize + x as usize) * channels;
                    match channels {
                        4 => out.extend_from_slice(&bytes[at..at + 4]),
                        3 => out.extend_from_slice(&[bytes[at], bytes[at + 1], bytes[at + 2], 255]),
                        2 => {
                            out.extend_from_slice(&[bytes[at], bytes[at], bytes[at], bytes[at + 1]])
                        }
                        _ => out.extend_from_slice(&[bytes[at], bytes[at], bytes[at], 255]),
                    }
                }
            }
            let rgba: Arc<[u8]> = out.into();
            cache.insert(key, rgba.clone());
            rgba
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        frame.images.push(KittyPlacement {
            key: hasher.finish(),
            rgba,
            source_size: (info.source_width, info.source_height),
            col: info.viewport_col,
            row: info.viewport_row,
            x_offset_px: iteration.x_offset()?,
            y_offset_px: iteration.y_offset()?,
            pixel_size: (info.pixel_width, info.pixel_height),
            cell_size_px: (
                terminal.width_px()? as f32 / frame.cols.max(1) as f32,
                terminal.height_px()? as f32 / frame.rows.max(1) as f32,
            ),
            z: iteration.z()?,
        });
    }
    cache.retain(|key, _| used.contains(key));
    Ok(())
}

/// Reads the OSC 8 target of the viewport cell `at` into `uri`; false if it has none.
fn hyperlink(terminal: &Terminal, at: (u16, u16), uri: &mut Vec<u8>) -> Result<bool> {
    let point = Point::Viewport(PointCoordinate {
        x: at.0,
        y: at.1.into(),
    });
    let cell = terminal.grid_ref(point)?;
    uri.resize(uri.capacity().max(256), 0);
    let len = match cell.hyperlink_uri(uri) {
        Err(ghostty_vt::Error::OutOfSpace { required }) => {
            uri.resize(required, 0);
            cell.hyperlink_uri(uri)?
        }
        result => result?,
    };
    uri.truncate(len);
    Ok(len > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cropped_snapshot_keeps_cursor_and_copy_coordinates() {
        let mut terminal = Terminal::new(ghostty_vt::TerminalOptions {
            cols: 12,
            rows: 5,
            ..Default::default()
        })
        .unwrap();
        terminal.vt_write(b"old\r\nolder\r\nrecent\r\nnewest\r\nprompt");
        let mut renderer = Renderer::new().unwrap();
        let frame = renderer.frame(&terminal).unwrap();
        let start = frame.display_row_start(3);
        assert_eq!(start, 2);
        assert_eq!(frame.selected_text(start * 12, start * 12 + 5), "recent");
        assert_eq!(frame.cursor.unwrap().1 as usize - start, 2);
        assert_eq!(frame.display_row_start(8), 0);

        // A TUI cursor on an older row takes precedence over bottom alignment.
        terminal.vt_write(b"\x1b[2;1H");
        assert_eq!(renderer.frame(&terminal).unwrap().display_row_start(3), 1);
        terminal.vt_write(b"\x1b[?25l");
        let hidden_cursor = renderer.frame(&terminal).unwrap();
        assert!(hidden_cursor.cursor.is_none());
        assert_eq!(hidden_cursor.display_row_start(3), 2);
    }
    #[test]
    fn copy_preserves_graphemes_without_wide_cell_padding() {
        let mut terminal = Terminal::new(ghostty_vt::TerminalOptions {
            cols: 20,
            rows: 3,
            ..Default::default()
        })
        .unwrap();
        terminal.vt_write("界 e\u{301}\r\nsecond".as_bytes());
        let frame = Renderer::new().unwrap().frame(&terminal).unwrap();
        assert_eq!(frame.selected_text(0, 25), "界 e\u{301}\nsecond");
        assert_eq!(frame.selected_text(25, 0), "界 e\u{301}\nsecond");
    }

    #[test]
    fn word_line_cursor_and_decoration_state_reach_the_frame() {
        let mut terminal = Terminal::new(ghostty_vt::TerminalOptions {
            cols: 12,
            rows: 4,
            ..Default::default()
        })
        .unwrap();
        terminal.vt_write(b"alpha beta\r\nabcdefghijklmnop\r\n");
        terminal.vt_write(b"\x1b[5 q\x1b[4:3;58:2::1:2:3mX");
        let frame = Renderer::new().unwrap().frame(&terminal).unwrap();
        assert_eq!(frame.word_range(7), (6, 9));
        assert_eq!(frame.line_range(13), (12, 35));
        assert_eq!(frame.cursor_shape, CursorShape::Bar);
        assert!(frame.cursor_blinking);
        let decorated = frame
            .cells
            .iter()
            .flatten()
            .find(|cell| cell.text == "X")
            .expect("decorated cell");
        assert_eq!(
            decorated.style.underline,
            ghostty_vt::style::Underline::Curly
        );
        assert_eq!(decorated.underline_color, 0x010203);
    }

    #[test]
    fn search_selects_and_exposes_viewport_matches() {
        let mut terminal = Terminal::new(ghostty_vt::TerminalOptions {
            cols: 20,
            rows: 3,
            max_scrollback: 1 << 20,
        })
        .unwrap();
        terminal.vt_write(b"needle one\r\nother\r\nneedle two\r\n");
        let mut search = Search::new(&terminal).unwrap();
        search.set_needle(&terminal, Some("needle")).unwrap();
        search.run(&terminal).unwrap();
        assert!(search.select_next(&terminal).unwrap());
        let frame = Renderer::new()
            .unwrap()
            .frame_with_search(&terminal, Some(&search))
            .unwrap();
        assert_eq!(frame.search_total, 2);
        assert_eq!(frame.search_index, Some(0));
        assert!(frame.search_selected.is_some());
        assert!(!frame.search_matches.is_empty());
    }

    #[test]
    fn corrupted_snapshot_never_installs_a_terminal() {
        let terminal = Terminal::new(ghostty_vt::TerminalOptions::default()).unwrap();
        let mut bytes = ghostty_vt::snapshot::encode(&terminal).unwrap().to_vec();
        bytes[16] ^= 1; // First record's CRC, not a transport boundary.
        let mut stream = TerminalStream::new(42);
        assert!(stream.feed(&bytes).is_err());
        assert!(stream.terminal.is_none());
    }
    #[test]
    fn snapshot_with_kitty_image_in_history_installs_a_terminal() {
        let mut terminal = Terminal::new(ghostty_vt::TerminalOptions {
            cols: 10,
            rows: 3,
            max_scrollback: 1 << 20,
        })
        .unwrap();
        terminal.set_kitty_image_storage_limit(1 << 20).unwrap();
        terminal.resize(10, 3, 9, 21).unwrap();
        terminal.vt_write(b"\x1b_Ga=T,f=24,s=1,v=1;CQkJ\x1b\\\r\n");
        let frame = Renderer::new().unwrap().frame(&terminal).unwrap();
        assert_eq!(frame.images.len(), 1);
        assert_eq!(frame.images[0].rgba.as_ref(), &[9, 9, 9, 255]);
        for line in 0..2000 {
            terminal.vt_write(format!("line {line}\r\n").as_bytes());
        }
        let snapshot = ghostty_vt::snapshot::encode(&terminal).unwrap();
        let mut stream = TerminalStream::new(42);
        stream.feed(&snapshot).unwrap();
        assert!(stream.terminal.is_some());
        assert!(stream.text().unwrap().contains("line 0\n"));
    }
    #[test]
    fn every_apc_split_and_false_nonce() {
        let message = b"a\x1b_rex1;123;{\"type\":\"resize\",\"cols\":80,\"rows\":24}\x1b\\z";
        for split in 0..=message.len() {
            let mut parser = ApcParser::new(123);
            let mut parts = parser.feed(&message[..split]).unwrap();
            parts.extend(parser.feed(&message[split..]).unwrap());
            let text: Vec<u8> = parts
                .iter()
                .filter_map(|p| {
                    if let Part::Output(b) = p {
                        Some(b.as_slice())
                    } else {
                        None
                    }
                })
                .flatten()
                .copied()
                .collect();
            assert_eq!(text, b"az");
            assert_eq!(
                parts
                    .iter()
                    .filter(|p| matches!(p, Part::Metadata(_)))
                    .count(),
                1
            );
        }
        let mut parser = ApcParser::new(456);
        assert_eq!(
            parser.feed(message).unwrap(),
            vec![Part::Output(message.to_vec())]
        );
    }
    #[test]
    fn snapshot_tail_and_parser_continuation_survive_fragmentation() {
        let mut terminal = Terminal::new(ghostty_vt::TerminalOptions {
            cols: 20,
            rows: 3,
            ..Default::default()
        })
        .unwrap();
        terminal.set_continuation_max_bytes(1024).unwrap();
        terminal.vt_write(b"before\x1b[31");
        let snapshot = ghostty_vt::snapshot::encode(&terminal).unwrap();
        let mut bytes = snapshot.to_vec();
        bytes.extend_from_slice(
            b"mAFTER\x1b_rex1;42;{\"type\":\"resize\",\"cols\":30,\"rows\":4}\x1b\\",
        );
        for size in [1, 7, 10, 64, 1024, bytes.len()] {
            let mut stream = TerminalStream::new(42);
            for chunk in bytes.chunks(size) {
                stream.feed(chunk).unwrap();
            }
            assert_eq!(stream.received, bytes.len() as u64);
            assert_eq!(stream.snapshot_count, 1);
            assert!(stream.text().unwrap().contains("beforeAFTER"));
            assert_eq!(stream.terminal.as_ref().unwrap().cols().unwrap(), 30);
        }
    }
}
