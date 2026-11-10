//! Translate authoritative Rex split trees to normalized native pane geometry: tiled to the
//! window, or as a strip of columns that scrolls sideways.
use rexrs::api::{
    session::{Layer, ViewLayoutNode, Window},
    terminal::SetTheme,
};
use serde_json::{Value, json};

/// One new terminal block for a layout, starting `shell` when one is set (else the server
/// user's login shell), with this client's declared `theme` when it has one. Rex counts a
/// layout's theme as its creator's declaration.
pub fn shell_layout(shell: Option<&str>, theme: Option<&SetTheme>) -> Value {
    let mut block = json!({"flavor": "com.superlogical.terminal.shell"});
    if let Some(shell) = shell.map(str::trim).filter(|s| !s.is_empty()) {
        block["options"]["shell"] = json!(shell);
    }
    if let Some(theme) = theme {
        block["options"]["theme"] = json!(theme);
    }
    json!({"block": block})
}

/// Set the server-side starting directory of a new terminal, without changing shell or theme.
/// A layout that is not a single block is left alone.
pub fn with_cwd(layout: &mut Value, cwd: &str) {
    if let Some(block) = layout.get_mut("block").filter(|b| b.is_object()) {
        block["options"]["cwd"] = json!(cwd);
    }
}

#[cfg(test)]
#[test]
fn with_cwd_keeps_the_shell_and_theme_and_leaves_splits_alone() {
    let theme = SetTheme::new().background("#001528");
    let mut layout = shell_layout(Some("zsh"), Some(&theme));
    with_cwd(&mut layout, "/srv/app");
    assert_eq!(
        layout["block"]["options"],
        json!({"shell":"zsh","theme":{"background":"#001528"},"cwd":"/srv/app"})
    );
    let mut bare = shell_layout(None, None);
    with_cwd(&mut bare, "/srv/app");
    assert_eq!(bare["block"]["options"], json!({"cwd":"/srv/app"}));
    let mut split = json!({"split":{}});
    with_cwd(&mut split, "/srv/app");
    assert_eq!(split, json!({"split":{}}));
}

#[cfg(test)]
#[test]
fn shell_layout_sets_the_shell_and_theme_only_when_given() {
    assert_eq!(
        shell_layout(None, None),
        json!({"block":{"flavor":"com.superlogical.terminal.shell"}})
    );
    assert_eq!(shell_layout(Some("  "), None), shell_layout(None, None));
    assert_eq!(
        shell_layout(Some("zsh"), None)["block"]["options"],
        json!({"shell":"zsh"})
    );
    let theme = SetTheme::new().background("#001528");
    assert_eq!(
        shell_layout(Some("zsh"), Some(&theme))["block"]["options"],
        json!({"shell":"zsh","theme":{"background":"#001528"}})
    );
    assert_eq!(
        shell_layout(None, Some(&SetTheme::new()))["block"]["options"],
        json!({"theme":{}}),
        "Rex's own colours"
    );
}

#[cfg(test)]
#[test]
fn with_cwd_sets_the_blocks_directory_only() {
    let mut layout = shell_layout(Some("zsh"), None);
    with_cwd(&mut layout, "/srv/app");
    assert_eq!(
        layout["block"]["options"],
        json!({"shell":"zsh","cwd":"/srv/app"})
    );
    let mut bare = shell_layout(None, None);
    with_cwd(&mut bare, "/srv/app");
    assert_eq!(bare["block"]["options"], json!({"cwd":"/srv/app"}));
    let mut split = json!({"split":{}});
    with_cwd(&mut split, "/srv/app");
    assert_eq!(split, json!({"split":{}}));
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub block: String,
    pub rect: [f64; 4],
    /// The sides (left, top, right, bottom) where it meets another card rather than the edge
    /// of its window or strip.
    pub inner: [bool; 4],
}
#[derive(Clone, Debug)]
pub struct Divider {
    pub before: String,
    pub after: String,
    pub horizontal: bool,
    pub rect: [f64; 4],
    pub ratio: f64,
}
impl Divider {
    pub fn key(&self) -> String {
        format!("{}:{}", self.before, self.after)
    }
}
type Layout = (Vec<Placement>, Vec<Divider>);
const EPSILON: f64 = 1e-6;

pub fn placements(window: &Window, drag: Option<(&str, f64)>) -> Layout {
    if let Some(zoomed) = zoomed(window) {
        return zoomed;
    }
    let mut layout = (Vec::new(), Vec::new());
    for layer in &window.layers {
        // Its blocks keep running, but a minimized layer isn't drawn. Unknown states draw.
        if layer.state.as_deref() == Some("minimized") {
            continue;
        }
        if let Some(node) = &layer.layout {
            place(node, bounds(layer), [0., 1.], drag, &mut layout);
        }
    }
    layout
}

/// Scrolling-column widths, in window widths.
pub const DEFAULT_COLUMN_WIDTH: f64 = 0.5;
pub const MIN_COLUMN_WIDTH: f64 = 0.1;
pub const MAX_COLUMN_WIDTH: f64 = 1.0;
pub const COLUMN_WIDTH_PRESETS: [f64; 3] = [1. / 3., 0.5, 2. / 3.];

/// A scrolling layout: the tiled layer's top-level side-by-side parts are columns of a strip
/// that scrolls sideways. This client sizes the columns, ignoring the server's ratios between
/// them; inside a column the server's splits stand. Positions along the strip are in window
/// widths.
pub struct Strip<'a> {
    window: &'a Window,
    columns: Vec<Column<'a>>,
    /// The tiled layer's top and height.
    rows: [f64; 2],
    /// The strip's whole width.
    pub extent: f64,
}
struct Column<'a> {
    id: &'a str,
    node: &'a ViewLayoutNode,
    left: f64,
    width: f64,
}

/// A draggable boundary between two scrolling columns, in the visible window's units.
pub struct ColumnEdge<'a> {
    pub before: &'a str,
    pub after: &'a str,
    pub x: f64,
}
impl<'a> Strip<'a> {
    /// `window`'s strip with every column `width` wide; a lone column fills the window.
    pub fn new(window: &'a Window, width: f64) -> Self {
        Self::with_widths(window, |_| width)
    }

    /// `window`'s strip with client-local widths returned by `width_for`.
    pub fn with_widths(window: &'a Window, mut width_for: impl FnMut(&str) -> f64) -> Self {
        let tiled = window.layers.iter().find(|l| l.kind == "tiled");
        let mut nodes = Vec::new();
        if let Some(node) = tiled.and_then(|l| l.layout.as_ref()) {
            columns(node, &mut nodes);
        }
        let lone = nodes.len() == 1;
        let mut extent = 0.;
        let columns = nodes
            .into_iter()
            .filter_map(|node| {
                let id = column_id(node)?;
                let left = extent;
                let width = if lone {
                    1.
                } else {
                    width_for(id).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
                };
                extent += width;
                Some(Column {
                    id,
                    node,
                    left,
                    width,
                })
            })
            .collect();
        let rows = tiled.map_or([0., 1.], |l| {
            let [_, y, _, h] = bounds(l);
            [y, h]
        });
        Self {
            window,
            columns,
            rows,
            extent,
        }
    }

    /// The offset nearest `offset` that keeps the window on the strip.
    pub fn clamp(&self, offset: f64) -> f64 {
        offset.clamp(0., (self.extent - 1.).max(0.))
    }

    pub fn column_ids(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.columns.iter().map(|column| column.id)
    }

    /// Align the viewport's left edge with the nearest column edge, including the strip end.
    pub fn snap(&self, offset: f64) -> f64 {
        let offset = self.clamp(offset);
        self.columns
            .iter()
            .map(|column| self.clamp(column.left))
            .chain(std::iter::once(self.clamp(self.extent)))
            .min_by(|a, b| (a - offset).abs().total_cmp(&(b - offset).abs()))
            .unwrap_or(offset)
    }
    /// The ID and span of the column containing `block`, in strip units.
    pub fn column(&self, block: &str) -> Option<(&'a str, [f64; 2])> {
        self.columns
            .iter()
            .find(|c| contains(c.node, block))
            .map(|c| (c.id, [c.left, c.width]))
    }

    pub fn column_span(&self, block: &str) -> Option<[f64; 2]> {
        self.column(block).map(|(_, span)| span)
    }
    pub fn column_width(&self, column: &str) -> Option<f64> {
        self.columns
            .iter()
            .find(|c| c.id == column)
            .map(|c| c.width)
    }

    /// The block or split the server accepts as the anchor for a new column beside `block`.
    pub fn column_anchor(&self, block: &str) -> Option<&'a str> {
        self.columns
            .iter()
            .find(|c| contains(c.node, block))
            .map(|c| c.id)
    }

    /// The single block in an adjacent single-pane column.
    pub fn swap_target(&self, block: &str, direction: i8) -> Option<&'a str> {
        let index = self.columns.iter().position(|c| contains(c.node, block))?;
        let current = self.columns.get(index)?.node.block_id.as_deref()?;
        (current == block).then_some(())?;
        self.columns
            .get(neighbor(index, self.columns.len(), direction)?)?
            .node
            .block_id
            .as_deref()
    }

    /// A block in the adjacent column that `block` can be consumed beside.
    pub fn consume_anchor(&self, block: &str, direction: i8) -> Option<&'a str> {
        let index = self.columns.iter().position(|c| contains(c.node, block))?;
        first(
            self.columns
                .get(neighbor(index, self.columns.len(), direction)?)?
                .node,
        )
    }

    /// The other block in a two-pane column. Removing either pane collapses that column to the
    /// anchor, so moving the pane horizontally beside it creates a top-level column.
    pub fn expel_anchor(&self, block: &str) -> Option<&'a str> {
        let column = self.columns.iter().find(|c| contains(c.node, block))?;
        (block_count(column.node) == 2)
            .then(|| another(column.node, block))
            .flatten()
    }

    /// Center `block`'s column in the window, clamped at the strip's ends.
    pub fn center(&self, block: &str) -> Option<f64> {
        let [left, width] = self.column_span(block)?;
        Some(self.clamp(left + width / 2. - 0.5))
    }

    /// Visible boundaries between adjacent columns.
    pub fn column_edges(&self, offset: f64) -> Vec<ColumnEdge<'a>> {
        let offset = self.clamp(offset);
        self.columns
            .windows(2)
            .filter_map(|pair| {
                let x = pair[0].left + pair[0].width - offset;
                (-EPSILON..=1. + EPSILON)
                    .contains(&x)
                    .then_some(ColumnEdge {
                        before: pair[0].id,
                        after: pair[1].id,
                        x,
                    })
            })
            .collect()
    }

    /// The offset nearest `offset` that shows all of `block`'s column.
    pub fn reveal(&self, offset: f64, block: &str) -> f64 {
        let Some(column) = self.columns.iter().find(|c| contains(c.node, block)) else {
            return self.clamp(offset);
        };
        let right = column.left + column.width;
        let offset = if right > offset + 1. {
            right - 1.
        } else {
            offset
        };
        self.clamp(offset.min(column.left))
    }

    /// The strip seen from `offset`, in window units: columns shifted along it, floating layers
    /// where the server puts them, and a zoomed pane filling the window.
    pub fn placements(&self, offset: f64, drag: Option<(&str, f64)>) -> Layout {
        if let Some(zoomed) = zoomed(self.window) {
            return zoomed;
        }
        let offset = self.clamp(offset);
        let span = [-offset, self.extent - offset];
        let [top, height] = self.rows;
        let mut layout = (Vec::new(), Vec::new());
        for c in &self.columns {
            let rect = [c.left - offset, top, c.width, height];
            place(c.node, rect, span, drag, &mut layout);
        }
        for layer in self.window.layers.iter().filter(|l| l.kind != "tiled") {
            if layer.state.as_deref() == Some("minimized") {
                continue;
            }
            if let Some(node) = &layer.layout {
                place(node, bounds(layer), [0., 1.], drag, &mut layout);
            }
        }
        layout
    }
}

/// The top-level side-by-side parts of `node`, left to right.
fn columns<'a>(node: &'a ViewLayoutNode, out: &mut Vec<&'a ViewLayoutNode>) {
    if let Some(split) = &node.split
        && split.direction != rexrs::api::Direction::Vertical
        && let (Some(before), Some(after)) = (&split.before, &split.after)
    {
        columns(before, out);
        columns(after, out);
    } else if first(node).is_some() {
        out.push(node);
    }
}
fn column_id(node: &ViewLayoutNode) -> Option<&str> {
    node.split
        .as_ref()
        .and_then(|split| split.split_id.as_deref())
        .or_else(|| first(node))
}
fn contains(node: &ViewLayoutNode, block: &str) -> bool {
    node.block_id.as_deref() == Some(block)
        || node.split.as_ref().is_some_and(|s| {
            [&s.before, &s.after]
                .into_iter()
                .flatten()
                .any(|n| contains(n, block))
        })
}
fn neighbor(index: usize, len: usize, direction: i8) -> Option<usize> {
    match direction.cmp(&0) {
        std::cmp::Ordering::Less => index.checked_sub(1),
        std::cmp::Ordering::Greater => (index + 1 < len).then_some(index + 1),
        std::cmp::Ordering::Equal => None,
    }
}
fn block_count(node: &ViewLayoutNode) -> usize {
    usize::from(node.block_id.is_some())
        + node.split.as_ref().map_or(0, |split| {
            split.before.as_deref().map_or(0, block_count)
                + split.after.as_deref().map_or(0, block_count)
        })
}

fn another<'a>(node: &'a ViewLayoutNode, block: &str) -> Option<&'a str> {
    if let Some(candidate) = node.block_id.as_deref() {
        return (candidate != block).then_some(candidate);
    }
    let split = node.split.as_ref()?;
    [&split.before, &split.after]
        .into_iter()
        .flatten()
        .find_map(|node| another(node, block))
}
fn zoomed(window: &Window) -> Option<Layout> {
    let block = window.zoomed_block_id.as_ref()?;
    Some((
        vec![Placement {
            block: block.clone(),
            rect: [0., 0., 1., 1.],
            inner: [false; 4],
        }],
        vec![],
    ))
}
fn bounds(layer: &Layer) -> [f64; 4] {
    layer
        .bounds
        .as_slice()
        .try_into()
        .unwrap_or([0., 0., 1., 1.])
}
/// Lay `node` out in `rect`. Its cards meet the edge at the window's top and bottom and where
/// they reach `span`'s left and right.
fn place(
    node: &ViewLayoutNode,
    rect: [f64; 4],
    span: [f64; 2],
    drag: Option<(&str, f64)>,
    (panes, dividers): &mut Layout,
) {
    let start = panes.len();
    walk(node, rect, drag, panes, dividers);
    for pane in &mut panes[start..] {
        let [x, y, w, h] = pane.rect;
        pane.inner = [
            x > span[0] + EPSILON,
            y > EPSILON,
            x + w < span[1] - EPSILON,
            y + h < 1. - EPSILON,
        ];
    }
}
fn first(node: &ViewLayoutNode) -> Option<&str> {
    node.block_id
        .as_deref()
        .or_else(|| first(node.split.as_ref()?.before.as_deref()?))
}
fn walk(
    node: &ViewLayoutNode,
    rect: [f64; 4],
    drag: Option<(&str, f64)>,
    panes: &mut Vec<Placement>,
    dividers: &mut Vec<Divider>,
) {
    if let Some(block) = &node.block_id {
        panes.push(Placement {
            block: block.clone(),
            rect,
            inner: [false; 4],
        });
        return;
    }
    let Some(split) = &node.split else {
        return;
    };
    let (Some(before), Some(after)) = (&split.before, &split.after) else {
        return;
    };
    let horizontal = split.direction != rexrs::api::Direction::Vertical;
    let key = format!(
        "{}:{}",
        first(before).unwrap_or(""),
        first(after).unwrap_or("")
    );
    let ratio = drag
        .filter(|(id, _)| *id == key)
        .map_or(split.ratio, |(_, r)| r);
    if let (Some(before), Some(after)) = (first(before), first(after)) {
        dividers.push(Divider {
            before: before.into(),
            after: after.into(),
            horizontal,
            rect,
            ratio,
        });
    }
    let [x, y, w, h] = rect;
    let (a, b) = if horizontal {
        (
            [x, y, w * ratio, h],
            [x + w * ratio, y, w * (1. - ratio), h],
        )
    } else {
        (
            [x, y, w, h * ratio],
            [x, y + h * ratio, w, h * (1. - ratio)],
        )
    };
    walk(before, a, drag, panes, dividers);
    walk(after, b, drag, panes, dividers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn nested_layout_and_drag_keep_server_order_and_area() {
        let window: Window = serde_json::from_value(json!({"layers":[{"kind":"tiled","bounds":[0,0,1,1],"layout":{"split":{"direction":"horizontal","ratio":0.6,"before":{"block_id":"a"},"after":{"split":{"direction":"vertical","ratio":0.4,"before":{"block_id":"b"},"after":{"block_id":"c"}}}}},"blocks":[]}]})).unwrap();
        let (panes, borders) = placements(&window, None);
        assert_eq!(
            panes.iter().map(|p| p.block.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert_eq!(panes[1].rect, [0.6, 0., 0.4, 0.4]);
        let key = borders[0].key();
        let (panes, _) = placements(&window, Some((&key, 0.7)));
        assert_eq!(panes[0].rect, [0., 0., 0.7, 1.]);
        assert!((panes.iter().map(|p| p.rect[2] * p.rect[3]).sum::<f64>() - 1.).abs() < 1e-9);
    }
    #[test]
    fn zoom_and_floating_layers_use_window_coordinates() {
        let mut window: Window = serde_json::from_value(json!({"layers":[{"kind":"floating","bounds":[0.2,0.1,0.5,0.6],"layout":{"block_id":"floating"}}]})).unwrap();
        assert_eq!(placements(&window, None).0[0].rect, [0.2, 0.1, 0.5, 0.6]);
        window.zoomed_block_id = Some("floating".into());
        assert_eq!(placements(&window, None).0[0].rect, [0., 0., 1., 1.]);
        assert!(placements(&window, None).1.is_empty());
    }
    #[test]
    fn a_strip_makes_top_level_side_by_side_parts_columns_and_keeps_splits_inside_them() {
        // The server's 0.7 and 0.3 between columns are ignored; the 0.4 inside one stands.
        let window: Window = serde_json::from_value(json!({"layers":[
            {"kind":"tiled","bounds":[0,0,1,1],"layout":{"split":{"direction":"horizontal","ratio":0.7,
                "before":{"split":{"direction":"horizontal","ratio":0.3,"before":{"block_id":"a"},
                    "after":{"split":{"direction":"vertical","ratio":0.4,"before":{"block_id":"b"},"after":{"block_id":"c"}}}}},
                "after":{"block_id":"d"}}}},
            {"kind":"floating","bounds":[0.2,0.1,0.5,0.6],"layout":{"block_id":"f"}}]})).unwrap();
        let strip = Strip::new(&window, 0.5);
        assert_eq!(strip.extent, 1.5);
        let (panes, dividers) = strip.placements(0.5, None);
        let at = |block: &str| panes.iter().find(|p| p.block == block).unwrap();
        assert_eq!(at("a").rect, [-0.5, 0., 0.5, 1.]);
        assert_eq!(at("a").inner, [false, false, true, false]);
        assert_eq!(at("b").rect, [0., 0., 0.5, 0.4]);
        assert_eq!(at("b").inner, [true, false, true, true]);
        assert_eq!(at("c").rect, [0., 0.4, 0.5, 0.6]);
        assert_eq!(at("d").rect, [0.5, 0., 0.5, 1.]);
        assert_eq!(
            at("d").inner,
            [true, false, false, false],
            "the strip ends here"
        );
        assert_eq!(
            at("f").rect,
            [0.2, 0.1, 0.5, 0.6],
            "floating layers don't scroll"
        );
        assert_eq!(dividers.len(), 1, "no dividers between columns");
        assert_eq!(
            (dividers[0].key(), dividers[0].rect),
            ("b:c".into(), [0., 0., 0.5, 1.])
        );
        // An offset past either end is held to the strip.
        assert_eq!(strip.placements(9., None).0[0].rect, [-0.5, 0., 0.5, 1.]);
        assert_eq!((strip.clamp(-1.), strip.clamp(9.)), (0., 0.5));
    }
    #[test]
    fn revealing_a_pane_scrolls_only_as_far_as_its_column_needs() {
        let window: Window = serde_json::from_value(json!({"layers":[{"kind":"tiled","layout":{"split":{"direction":"horizontal","ratio":0.5,
            "before":{"split":{"direction":"horizontal","ratio":0.5,"before":{"block_id":"a"},"after":{"block_id":"b"}}},
            "after":{"split":{"direction":"vertical","ratio":0.5,"before":{"block_id":"c"},"after":{"block_id":"d"}}}}}}]})).unwrap();
        let strip = Strip::new(&window, 0.5);
        assert_eq!(strip.swap_target("a", 1), Some("b"));
        assert_eq!(strip.swap_target("b", -1), Some("a"));
        assert_eq!(
            strip.swap_target("b", 1),
            None,
            "the next column has two panes"
        );
        assert_eq!(strip.consume_anchor("b", 1), Some("c"));
        assert_eq!(strip.consume_anchor("c", -1), Some("b"));
        assert_eq!(strip.expel_anchor("c"), Some("d"));
        assert_eq!(strip.expel_anchor("a"), None);
        let three: Window = serde_json::from_value(json!({"layers":[{"kind":"tiled","layout":
            {"split":{"direction":"vertical","ratio":0.5,
                "before":{"block_id":"x"},
                "after":{"split":{"direction":"vertical","ratio":0.5,
                    "before":{"block_id":"y"},"after":{"block_id":"z"}}}}}}]}))
        .unwrap();
        assert_eq!(
            Strip::new(&three, 0.5).expel_anchor("x"),
            None,
            "a pane is only expellable when removing it collapses the column"
        );
        assert_eq!(strip.column_span("d"), Some([1., 0.5]));
        assert_eq!(strip.reveal(0., "d"), 0.5, "the right edge comes into view");
        assert_eq!(strip.reveal(0.5, "b"), 0.5, "already in view");
        assert_eq!(strip.reveal(0.5, "a"), 0., "the left edge comes into view");
        assert_eq!(strip.reveal(0.25, "gone"), 0.25);
    }
    #[test]
    fn local_widths_edges_center_and_column_anchors_follow_column_ids() {
        let window: Window = serde_json::from_value(json!({"layers":[{"kind":"tiled","layout":
            {"split":{"direction":"horizontal","ratio":0.5,
                "before":{"split":{"direction":"horizontal","ratio":0.5,
                    "before":{"block_id":"a"},
                    "after":{"split":{"split_id":"column:bc","direction":"vertical","ratio":0.5,
                        "before":{"block_id":"b"},"after":{"block_id":"c"}}}}},
                "after":{"block_id":"d"}}}}]}))
        .unwrap();
        let strip = Strip::with_widths(&window, |column| match column {
            "a" => 0.3,
            "column:bc" => 0.6,
            "d" => 0.4,
            _ => unreachable!(),
        });
        assert!((strip.extent - 1.3).abs() < EPSILON);
        assert_eq!(strip.column("c"), Some(("column:bc", [0.3, 0.6])));
        assert_eq!(strip.column_anchor("c"), Some("column:bc"));
        assert_eq!(strip.column_width("d"), Some(0.4));
        assert!((strip.center("b").unwrap() - 0.1).abs() < EPSILON);
        assert!((strip.center("d").unwrap() - 0.3).abs() < EPSILON);
        assert_eq!(
            strip.column_ids().collect::<Vec<_>>(),
            ["a", "column:bc", "d"]
        );
        assert!((strip.snap(0.14) - 0.).abs() < EPSILON);
        assert!((strip.snap(0.16) - 0.3).abs() < EPSILON);
        let edges = strip.column_edges(0.2);
        assert_eq!(
            edges
                .iter()
                .map(|edge| (edge.before, edge.after))
                .collect::<Vec<_>>(),
            [("a", "column:bc"), ("column:bc", "d")]
        );
        assert!((edges[0].x - 0.1).abs() < EPSILON);
        assert!((edges[1].x - 0.7).abs() < EPSILON);
    }
    #[test]
    fn a_lone_column_fills_the_window_and_zoom_still_wins() {
        let mut window: Window = serde_json::from_value(
            json!({"layers":[{"kind":"tiled","layout":{"split":{"direction":"vertical","ratio":0.5,
            "before":{"block_id":"a"},"after":{"block_id":"b"}}}}]}),
        )
        .unwrap();
        let strip = Strip::new(&window, 0.5);
        assert_eq!((strip.extent, strip.clamp(0.4)), (1., 0.));
        assert_eq!(strip.placements(0., None).0[1].rect, [0., 0.5, 1., 0.5]);
        window.zoomed_block_id = Some("b".into());
        let strip = Strip::new(&window, 0.5);
        let (panes, dividers) = strip.placements(0., None);
        assert_eq!(
            (panes[0].block.as_str(), panes[0].rect),
            ("b", [0., 0., 1., 1.])
        );
        assert!(dividers.is_empty());
    }
    #[test]
    fn minimized_layers_are_not_drawn_and_unknown_states_are() {
        let window: Window = serde_json::from_value(json!({"layers":[
            {"kind":"tiled","bounds":[0,0,1,1],"layout":{"block_id":"tiled"}},
            {"kind":"floating","state":"minimized","bounds":[0.2,0.1,0.5,0.6],"layout":{"block_id":"hidden"}},
            {"kind":"floating","state":"docked","bounds":[0.2,0.1,0.5,0.6],"layout":{"block_id":"later"}}
        ]}))
        .unwrap();
        let (panes, _) = placements(&window, None);
        assert_eq!(
            panes.iter().map(|p| p.block.as_str()).collect::<Vec<_>>(),
            ["tiled", "later"]
        );
    }
}
