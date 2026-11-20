//! Floating layers: what the server's session view says about a window's layers, and the
//! refusals this client decides itself around the layer methods of API revision 13.
//!
//! `session.view` lists a window's layers from back to front, each with a `kind`, `bounds`,
//! `state`, layout and blocks; `session.float_block`, `session.new_layer`, `session.raise_layer`,
//! `session.set_layer_state`, `session.move_block` and `session.close_layer` change them. Those
//! methods were checked live at revision 13 only (see `docs/parity-2026-11/M4-floating-layers.md`),
//! so a host that negotiated 10–12 still attaches and renders its tiled layouts as before, and
//! gets no layer change from this client. [`unsupported`] and [`Blocked`] carry the words both
//! the rows and the notices use.
//!
//! Nothing here has a UI dependency, and nothing here invents layout: the view is the only
//! authority for which layer holds a block, which layers are visible, and what the layer's
//! bounds and order are. A layer kind this client does not know draws as floating and a state it
//! does not know draws as normal, as the server's documentation instructs.
use rexrs::api::session::{Layer, ViewLayoutNode, Window};

/// The API revision that advertises the layer methods and the view's layers.
pub const MIN_API_REVISION: u32 = 13;

/// The rectangle `Float pane` and `New floating pane` ask for, as `[x, y, w, h]` fractions of
/// the window: centred and inside the window at every size. It is the request's stated intent
/// only; the layer's real geometry is whatever the server reports afterwards.
pub const REQUESTED_BOUNDS: [f64; 4] = [0.15, 0.12, 0.7, 0.7];

/// The `direction`, `side` and `ratio` this client asks for when it returns a pane to the tiled
/// layer: beside the anchor, to its right. The server owns the resulting split.
pub const RETURN_DIRECTION: &str = "horizontal";
pub const RETURN_SIDE: &str = "after";
pub const RETURN_RATIO: f64 = 0.5;

/// A reason a floating-layer action did not run. The sentence is for a notice; the hint is the
/// few words a row's hint column can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// The host on screen is not connected.
    NotConnected,
    /// The host negotiated an API revision older than [`MIN_API_REVISION`].
    OldServer(u32),
    /// No pane on screen to name.
    NoPane,
    /// No tab on screen to put a floating layer in.
    NoTab,
    /// The captured layer (or the pane it named) is no longer what the view shows.
    Stale,
    /// The pane is already in a floating layer.
    AlreadyFloating,
    /// Minimizing would leave the window without a visible pane.
    LastVisible,
    /// The tiled layer has no block to return beside.
    NoAnchor,
    /// The layer is already in front of every other floating layer.
    InFront,
    /// The action needs a floating layer, and this one is tiled.
    NotFloating,
}

impl Blocked {
    /// What a notice says. It names the refusal, not a retry.
    pub fn sentence(self) -> String {
        match self {
            Self::NotConnected => "Not connected to this host.".into(),
            Self::OldServer(revision) => format!(
                "This host's Rex is API revision {revision}; floating panes need revision \
                 {MIN_API_REVISION}. Nothing was changed."
            ),
            Self::NoPane => "No pane on screen to act on.".into(),
            Self::NoTab => "Open a tab before adding a floating pane.".into(),
            Self::Stale => {
                "That floating layer changed since it was listed. Nothing was changed.".into()
            }
            Self::AlreadyFloating => {
                "This pane is already in a floating layer. Nothing was changed.".into()
            }
            Self::LastVisible => {
                "This layer holds the tab's only visible panes, and Rex keeps a tab's panes \
                 visible. Restore another layer or open another visible pane first."
                    .into()
            }
            Self::NoAnchor => {
                "This tab has no tiled pane to return beside, and this client returns a pane \
                 only beside a tiled one."
                    .into()
            }
            Self::InFront => "This layer is already in front.".into(),
            Self::NotFloating => "This layer is not a floating layer.".into(),
        }
    }

    /// What a disabled row's hint column says.
    pub fn hint(self) -> String {
        match self {
            Self::NotConnected => "Not connected".into(),
            Self::OldServer(revision) => {
                format!("Needs Rex API {MIN_API_REVISION}, not {revision}")
            }
            Self::NoPane => "No pane".into(),
            Self::NoTab => "No tab".into(),
            Self::Stale => "Layer changed".into(),
            Self::AlreadyFloating => "Already floating".into(),
            Self::LastVisible => "Only visible panes".into(),
            Self::NoAnchor => "No tiled pane beside it".into(),
            Self::InFront => "Already in front".into(),
            Self::NotFloating => "Not floating".into(),
        }
    }
}

/// Why this host cannot change floating layers: the negotiated revision, before anything is
/// sent. None while it can.
pub fn unsupported(connected: bool, api_revision: u32) -> Option<Blocked> {
    if !connected {
        return Some(Blocked::NotConnected);
    }
    (api_revision < MIN_API_REVISION).then_some(Blocked::OldServer(api_revision))
}

/// A layer the server's documented rule says to draw as floating: anything not `tiled`,
/// including a kind this client does not know.
pub fn floating(layer: &Layer) -> bool {
    layer.kind != "tiled"
}

/// A layer the server says not to draw. A state this client does not know draws as normal.
pub fn minimized(layer: &Layer) -> bool {
    layer.state.as_deref() == Some("minimized")
}

/// The window's tiled layer, the one that holds its layout and fills it.
pub fn tiled(window: &Window) -> Option<&Layer> {
    window.layers.iter().find(|layer| layer.kind == "tiled")
}

/// The layer whose blocks include `block`, as the view places it.
pub fn layer_of<'a>(window: &'a Window, block: &str) -> Option<&'a Layer> {
    window.layers.iter().find(|layer| {
        layer
            .blocks
            .iter()
            .any(|b| b.block_id.as_deref() == Some(block))
    })
}

/// The window's floating layers, back to front: the order the server draws them in.
pub fn floating_layers(window: &Window) -> impl Iterator<Item = &Layer> {
    window.layers.iter().filter(|layer| floating(layer))
}

/// The frontmost floating layer, which every other floating layer is behind.
pub fn frontmost(window: &Window) -> Option<&Layer> {
    floating_layers(window).last()
}

/// How many blocks the window would still show with every layer but its own rules applied:
/// the blocks of every layer that is not minimized. A tiled layer with no blocks contributes
/// none, so a window whose only panes float is counted from its floating layers.
pub fn visible_panes(window: &Window) -> usize {
    window
        .layers
        .iter()
        .filter(|layer| !minimized(layer))
        .map(|layer| layer.blocks.len())
        .sum()
}

/// Whether minimizing `layer` would leave the window without a visible pane, which the server
/// refuses: this layer is visible, holds panes, and every visible pane in the window is in it.
pub fn holds_last_visible(window: &Window, layer: &Layer) -> bool {
    !minimized(layer) && !layer.blocks.is_empty() && visible_panes(window) == layer.blocks.len()
}

fn first_block<'a>(node: &'a ViewLayoutNode, exclude: &str) -> Option<&'a str> {
    if let Some(id) = node.block_id.as_deref() {
        return (id != exclude).then_some(id);
    }
    let split = node.split.as_ref()?;
    first_block(split.before.as_deref()?, exclude)
        .or_else(|| first_block(split.after.as_deref()?, exclude))
}

/// The tiled block a floating pane returns beside: the first block of the window's tiled layout,
/// in layout order, never `block` itself. The server would accept an anchor in any layer; this
/// client's policy is a tiled one in the same window. None when the tiled layer has none.
pub fn tiled_anchor(window: &Window, block: &str) -> Option<String> {
    let layer = tiled(window)?;
    if let Some(node) = &layer.layout
        && let Some(found) = first_block(node, block)
    {
        return Some(found.to_owned());
    }
    // A view without a layout tree still lists the layer's blocks in layout order.
    layer
        .blocks
        .iter()
        .filter_map(|b| b.block_id.as_deref())
        .find(|id| *id != block)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn window() -> Window {
        serde_json::from_value(json!({"window_id":"w","layers":[
            {"layer_id":"tiled","kind":"tiled","bounds":[0,0,1,1],
             "layout":{"split":{"direction":"horizontal","ratio":0.5,
                "before":{"block_id":"a"},"after":{"block_id":"b"}}},
             "blocks":[{"block_id":"a","label":"a"},{"block_id":"b","label":"b"}]},
            {"layer_id":"first","kind":"floating","bounds":[0.15,0.12,0.7,0.7],
             "layout":{"block_id":"f"},"blocks":[{"block_id":"f","label":"f"}]},
            {"layer_id":"second","kind":"stacked","bounds":[0.2,0.2,0.4,0.4],
             "layout":{"block_id":"g"},"blocks":[{"block_id":"g"}]}
        ]}))
        .unwrap()
    }

    #[test]
    fn the_gate_is_the_negotiated_revision_and_says_which_one() {
        assert_eq!(unsupported(false, 13), Some(Blocked::NotConnected));
        assert_eq!(unsupported(true, 10), Some(Blocked::OldServer(10)));
        assert_eq!(unsupported(true, 12), Some(Blocked::OldServer(12)));
        assert_eq!(unsupported(true, 13), None);
        assert_eq!(unsupported(true, 14), None);
        assert_eq!(MIN_API_REVISION, 13);
        // A row's hint and a notice's sentence both name the revision it needs, truthfully.
        for revision in 10..13 {
            let blocked = Blocked::OldServer(revision);
            assert!(blocked.hint().contains("13"), "{revision}");
            assert!(blocked.hint().contains(&revision.to_string()), "{revision}");
            assert!(
                blocked
                    .sentence()
                    .contains(&format!("API revision {revision}")),
                "{revision}"
            );
            assert!(blocked.sentence().contains("revision 13"), "{revision}");
        }
    }

    #[test]
    fn unknown_kinds_draw_as_floating_and_unknown_states_as_normal() {
        let window = window();
        let kinds: Vec<_> = floating_layers(&window)
            .map(|layer| layer.layer_id.as_deref().unwrap())
            .collect();
        assert_eq!(kinds, ["first", "second"]);
        assert_eq!(
            frontmost(&window).unwrap().layer_id.as_deref(),
            Some("second")
        );
        assert_eq!(tiled(&window).unwrap().layer_id.as_deref(), Some("tiled"));
        let mut later = window.clone();
        later.layers[2].state = Some("docked".into());
        assert!(!minimized(&later.layers[2]));
        assert_eq!(visible_panes(&later), 4);
        later.layers[2].state = Some("minimized".into());
        assert_eq!(visible_panes(&later), 3);
        // A minimized layer keeps its place in the stack: raising another still changes order.
        assert_eq!(
            frontmost(&later).unwrap().layer_id.as_deref(),
            Some("second")
        );
    }

    #[test]
    fn a_block_belongs_to_the_one_layer_the_view_places_it_in() {
        let window = window();
        let layer = |block: &str| {
            layer_of(&window, block)
                .and_then(|layer| layer.layer_id.clone())
                .unwrap_or_default()
        };
        assert_eq!(layer("a"), "tiled");
        assert_eq!(layer("g"), "second");
        assert_eq!(layer("missing"), "");
    }

    #[test]
    fn minimizing_counts_every_visible_pane_not_the_zoom_projection() {
        let mut window = window();
        assert!(!holds_last_visible(&window, &window.layers[1]));
        assert!(!holds_last_visible(&window, &window.layers[0]));
        // The tiled layer emptied by floating its last pane: the floating layer is all there is.
        window.layers[0].layout = None;
        window.layers[0].blocks.clear();
        window.layers[2].state = Some("minimized".into());
        assert!(holds_last_visible(&window, &window.layers[1]));
        // A zoomed block does not change which panes are visible.
        window.zoomed_block_id = Some("f".into());
        assert!(holds_last_visible(&window, &window.layers[1]));
        // A second visible pane in another layer ends that.
        window.layers[2].state = None;
        assert!(!holds_last_visible(&window, &window.layers[1]));
        // A minimized layer is never the last visible one.
        window.layers[1].state = Some("minimized".into());
        assert!(!holds_last_visible(&window, &window.layers[1]));
    }

    #[test]
    fn the_anchor_is_the_first_tiled_block_and_never_the_moving_pane() {
        let window = window();
        assert_eq!(tiled_anchor(&window, "f").as_deref(), Some("a"));
        assert_eq!(tiled_anchor(&window, "a").as_deref(), Some("b"));
        // With no tiled layer, or no block in it, there is nothing to return beside: this
        // client never makes one up.
        let mut empty = window.clone();
        empty.layers.remove(0);
        assert_eq!(tiled_anchor(&empty, "f"), None);
        let mut bare = window.clone();
        bare.layers[0].layout = None;
        bare.layers[0].blocks.clear();
        assert_eq!(tiled_anchor(&bare, "f"), None);
        // A layout that is one block, which is also the pane returning.
        let mut alone = window.clone();
        alone.layers[0].layout = Some(serde_json::from_value(json!({"block_id":"a"})).unwrap());
        alone.layers[0].blocks = alone.layers[0].blocks[..1].to_vec();
        assert_eq!(tiled_anchor(&alone, "f").as_deref(), Some("a"));
        assert_eq!(tiled_anchor(&alone, "a"), None);
    }
}
