//! The pane area: server-placed terminals as floating cards in 8pt gutters, each with a header
//! (status, label, size, exception pill), drag handles in the gutters, and empty states. A tab
//! with a single pane draws it edge to edge instead. A scrolling tab's columns slide sideways
//! under the wheel and to bring the focused pane into view.
//! Cards spring when the layout changes; the terminal grid inside never animates.
use super::{
    panel::{Form, Intent, PanelKind},
    term::{self, CursorLook, GridLook, Metrics, MouseGesture, PADDING},
    theme::*,
    widgets::{Pill, pill},
    workspace::{DividerDrag, Workspace},
};
use rex_linux::settings::TabLayout;
use rex_linux::{
    connection::Command,
    layout::{self, Placement},
};
use rexrs::api::session::Window;
use rune::{
    Cursor, El, GesturePhase, Phase, PointerButton, PointerEvent, Tx, WheelEvent, col, el, row,
    text,
};
use rune_kit::TextInput;

/// The area cards are laid out in, in the pane area's own points.
fn inner(s: &Workspace) -> [f32; 4] {
    let [_, _, aw, ah] = s.area;
    [
        MARGIN,
        0.,
        (aw - 2. * MARGIN).max(0.),
        (ah - MARGIN).max(0.),
    ]
}

/// A placement's card rect: its share of the inner area, inset by half a gutter wherever it
/// meets another card.
pub fn card_rect(s: &Workspace, placement: &Placement) -> [f32; 4] {
    let [ix, iy, iw, ih] = inner(s);
    let [x, y, w, h] = placement.rect;
    let half = GUTTER / 2.;
    let [l, t, r, b] = placement.inner;
    let mut c = [
        ix + x as f32 * iw,
        iy + y as f32 * ih,
        w as f32 * iw,
        h as f32 * ih,
    ];
    if l {
        c[0] += half;
        c[2] -= half;
    }
    if t {
        c[1] += half;
        c[3] -= half;
    }
    if r {
        c[2] -= half;
    }
    if b {
        c[3] -= half;
    }
    c[2] = c[2].max(0.);
    c[3] = c[3].max(0.);
    c
}

enum CardState {
    Focused,
    Target,
    Plain,
}

/// A tab with one pane, not zoomed, draws it edge to edge: no card, header or margin.
pub fn full_bleed(window: &Window) -> bool {
    window.zoomed_block_id.is_none() && layout::placements(window, None).0.len() == 1
}

fn header(s: &Workspace, block: &str, width: f32, focused: bool, m: &Metrics) -> El {
    let label = s.block_label(block);
    let id = block.to_owned();
    let menu_id = block.to_owned();
    let h = row()
        .key(format!("pane-header:{block}"))
        .w_full()
        .h(CARD_HEADER)
        .px(14.)
        .gap(8.)
        .center_y()
        .preserve_focus_on_press(true)
        .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
            if s.focused_block().as_deref() != Some(id.as_str())
                && let Some(t) = s.terminals.get(&s.term_key(&id))
            {
                let _ = t.connection.commands.focus();
            }
            tx.blur();
        })
        .on_secondary_press(move |s: &mut Workspace, p, _| {
            s.pane_menu(menu_id.clone(), Some((p.x, p.y)))
        })
        .child(
            text(label.clone())
                .font(medium(11.5))
                .color(rgb(if focused { INK } else { MUTED }))
                .truncate()
                .help_when_truncated(label)
                .shrink(),
        )
        .child(el().grow(1.));
    match strip(s, block, width, m) {
        Some(strip) => h.child(strip),
        None => h,
    }
}

/// The exception strip for a pane that is reconnecting, with its Retry action.
fn strip(s: &Workspace, block: &str, width: f32, m: &Metrics) -> Option<El> {
    let key = s.term_key(block);
    let t = s.terminals.get(&key);
    let status = t.map(|t| t.status());
    let pill_label = status.as_ref().and_then(|st| {
        let hidden = t.map_or(0, |t| t.hidden_rows(m.cell));
        let rows = if hidden > 0 {
            format!(
                " · {hidden} row{} hidden",
                if hidden == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        };
        (!st.live).then(|| (format!("Reconnecting · input paused{rows}"), "Retry"))
    });
    pill_label.map(|(msg, action)| {
        let target = key.clone();
        let status_text = status.map(|s| s.status).unwrap_or_default();
        let mut p = row()
            .key("strip")
            .h(22.)
            .pl(10.)
            .pr(3.)
            .gap(8.)
            .center_y()
            .radius(11.)
            .fill(rgb(mix(WARNING, GROUND, 0.1)))
            .border(1., rgb(WARNING).alpha(0.35))
            .shrink()
            .min_w(0.);
        if width >= 360. {
            p = p.child(
                text(msg)
                    .font(medium(11.))
                    .color(rgb(WARNING))
                    .truncate()
                    .shrink()
                    .help(status_text),
            );
        }
        p.child(
            pill(
                &format!("strip-action:{block}"),
                action,
                Pill::Warning,
                18.,
                move |s: &mut Workspace, tx: &mut Tx| {
                    s.terminal_command(&target, Command::Reconnect);
                    tx.blur();
                },
            )
            .focusable(false)
            .preserve_focus_on_press(true)
            .tab_index(-1),
        )
    })
}

fn card(
    s: &Workspace,
    block: &str,
    rect: [f32; 4],
    m: &Metrics,
    state: CardState,
    input: bool,
    bleed: bool,
) -> El {
    let key = s.term_key(block);
    let id = block.to_owned();
    let focused = matches!(state, CardState::Focused);
    let mut c = col()
        .key(format!("pane:{block}"))
        .absolute(rect[0], rect[1])
        .size(rect[2], rect[3])
        .fill(rgb(GROUND))
        .motion(SNAPPY)
        // Cards appear and leave at once (a tab or session switch never crossfades terminals);
        // only a card that stays springs to its new place.
        .transition(no_presence())
        .on_secondary_press(move |s: &mut Workspace, p, _| {
            s.pane_menu(id.clone(), Some((p.x, p.y)))
        });
    if !bleed {
        c = c
            .radius(CARD_RADIUS)
            .continuous_corners()
            .clip_rounded(CARD_RADIUS);
        c = match state {
            CardState::Focused => c
                .border_gradient(1., rgb(ACCENT).alpha(0.85), rgb(ACCENT).alpha(0.5))
                .box_shadow(shadow(0., 0., 24., rgb(ACCENT).alpha(0.2)))
                .box_shadow(shadow(0., 14., 40., black(0.55))),
            CardState::Target => {
                c.border(1., rgb(ACCENT).alpha(0.85))
                    .box_shadow(shadow(0., 10., 30., black(0.45)))
            }
            CardState::Plain => c
                .border_gradient(1., white(0.10), white(0.035))
                .box_shadow(shadow(0., 10., 30., black(0.45))),
        };
        c = c.child(header(s, block, rect[2], focused, m));
    }
    let c = match s.terminals.get(&key) {
        Some(t) => {
            let cursor = if input && s.window_focused {
                CursorLook::Block
            } else {
                CursorLook::Hollow
            };
            let colors = s.term_colors();
            let look = GridLook {
                cursor: Some(cursor),
                ime: input && s.window_focused,
                colors,
            };
            let (k1, k2, k3) = (key.clone(), key.clone(), key.clone());
            let (k4, k5, k6) = (key.clone(), key.clone(), key.clone());
            let menu_block = block.to_owned();
            let mut grid = el()
                .key(format!("grid:{block}"))
                .grow(1.)
                .w_full()
                .min_h(0.)
                // A program that tracks the mouse owns the pointer, as in Ghostty; a link under
                // Ctrl is the exception.
                .cursor(if t.link.is_some() {
                    Cursor::Pointer
                } else if t.tracking() {
                    Cursor::Default
                } else {
                    Cursor::Text
                });
            // The middle button only presses the grid while a program wants it.
            if t.tracking() {
                grid = grid.pointer_buttons([PointerButton::Middle]);
            }
            let grid = grid
                // The terminal never animates: not its cells, colours or place, even while the
                // card around it springs. The card clips it.
                .instant_changes()
                .motion(STILL)
                .on_layout(move |s: &mut Workspace, r| {
                    let font = s.settings.font_size;
                    if let Some(t) = s.terminals.get_mut(&k1) {
                        t.rect = r;
                        t.resize_if_owner(font);
                    }
                })
                .on_drag(move |s: &mut Workspace, d, tx| {
                    let cell = term::metrics(s.settings.font_size).cell;
                    let focused = s.focused_block().as_deref() == Some(k2.1.as_str());
                    let Some(t) = s.terminals.get_mut(&k2) else {
                        return;
                    };
                    let button = match tx.button() {
                        PointerButton::Primary => "left",
                        PointerButton::Middle => "middle",
                        _ => return,
                    };
                    let mods = tx.mods();
                    let at = t.cell_at(d.x, d.y, cell);
                    if d.phase == Phase::Down {
                        if !focused {
                            let _ = t.connection.commands.focus();
                        }
                        // Ctrl+click opens a link, even over a program that tracks the mouse.
                        t.pressed_link = None;
                        if button == "left"
                            && mods.ctrl
                            && let Some((_, link)) = t.link_at(d.x, d.y, cell)
                        {
                            t.selection = None;
                            t.mouse = None;
                            t.pressed_link = Some(link.uri);
                            return;
                        }
                        // Shift keeps local selection, as Ghostty and xterm do.
                        if t.tracking() && !mods.shift && t.status().live {
                            t.selection = None;
                            t.mouse = Some(MouseGesture { button });
                            t.send_mouse("press", Some(button), at, mods);
                            return;
                        }
                        t.mouse = None;
                    }
                    // A Ctrl+press on a link opens it if released over the same link.
                    if let Some(uri) = &t.pressed_link {
                        if d.phase != Phase::Up {
                            tx.live_only();
                            return;
                        }
                        let same = t
                            .link_at(d.x, d.y, cell)
                            .is_some_and(|(_, l)| &l.uri == uri);
                        let uri = t.pressed_link.take().filter(|_| same);
                        if let Some(uri) = uri {
                            s.open_link(uri);
                        }
                        return;
                    }
                    if let Some(gesture) = t.mouse {
                        match d.phase {
                            Phase::Move => {
                                // The view reads nothing a forwarded drag changes.
                                tx.live_only();
                                if t.mouse_cell != Some(at) {
                                    t.send_mouse("motion", Some(gesture.button), at, mods)
                                }
                            }
                            Phase::Up => {
                                t.mouse = None;
                                t.send_mouse("release", Some(gesture.button), at, mods);
                            }
                            _ => {}
                        }
                        return;
                    }
                    if button != "left" {
                        return;
                    }
                    let i = t.index(d.x, d.y, cell);
                    match d.phase {
                        Phase::Down => t.begin_selection(i, d.clicks),
                        Phase::Move => t.extend_selection(i),
                        Phase::Up => {}
                    }
                })
                // Pointer motion with no button held: Ctrl shows the link under it, and programs
                // that track every move (1003) hear it (the server drops it for the others).
                // Only the focused pane reports it.
                .on_pointer_move_local(move |s: &mut Workspace, p: PointerEvent, tx: &mut Tx| {
                    let cell = term::metrics(s.settings.font_size).cell;
                    let focused = s.focused_block().as_deref() == Some(k4.1.as_str());
                    let Some(t) = s.terminals.get_mut(&k4) else {
                        tx.live_only();
                        return;
                    };
                    // Holding Ctrl shows the link a click would open.
                    let link = if tx.mods().ctrl {
                        t.link_at(p.x, p.y, cell)
                    } else {
                        None
                    };
                    let same = link.as_ref().map(|(_, l)| l) == t.link.as_ref().map(|(_, l)| l);
                    t.link = link;
                    if same {
                        tx.live_only();
                    }
                    if !focused || t.mouse.is_some() || !t.tracking() || !t.status().live {
                        return;
                    }
                    let at = t.cell_at(p.x, p.y, cell);
                    if t.mouse_cell != Some(at) {
                        t.send_mouse("motion", None, at, tx.mods());
                    }
                })
                .on_hover_change(move |s: &mut Workspace, inside: bool, _: &mut Tx| {
                    if !inside && let Some(t) = s.terminals.get_mut(&k6) {
                        t.link = None;
                    }
                })
                // Right-click opens the pane menu, even over a program that tracks the mouse;
                // Shift+right-click goes to such a program (sent on release, when Rune reports
                // it).
                .on_secondary_press_local(move |s: &mut Workspace, p: PointerEvent, tx: &mut Tx| {
                    let cell = term::metrics(s.settings.font_size).cell;
                    let mods = tx.mods();
                    let focused = s.focused_block().as_deref() == Some(k5.1.as_str());
                    if let Some(t) = s.terminals.get_mut(&k5)
                        && t.tracking()
                        && mods.shift
                        && t.status().live
                    {
                        if !focused {
                            let _ = t.connection.commands.focus();
                        }
                        let at = t.cell_at(p.x, p.y, cell);
                        t.selection = None;
                        t.send_mouse("press", Some("right"), at, mods);
                        t.send_mouse("release", Some("right"), at, mods);
                        return;
                    }
                    let w = p.window();
                    s.pane_menu(menu_block.clone(), Some((w.x, w.y)));
                })
                .on_wheel(move |s: &mut Workspace, e: WheelEvent, _| {
                    // Sideways scrolling and a precise gesture's zero-delta end belong to the
                    // pane area, so it can pan and snap the scrolling strip.
                    let strip_end = e.precise
                        && e.phase == GesturePhase::Ended
                        && s.model()
                            .window()
                            .and_then(|window| window.window_id.as_deref())
                            .is_some_and(|id| s.tab_layout(id) == TabLayout::Scrolling);
                    if e.dx.abs() > e.dy.abs() || strip_end {
                        return false;
                    }
                    let scale = if e.precise { s.ui_scale() } else { 1. };
                    let cell = term::metrics(s.settings.font_size).cell;
                    let Some(t) = s.terminals.get_mut(&k3) else {
                        return false;
                    };
                    t.scroll -= e.dy / (cell.1 * scale);
                    let lines = t.scroll.trunc();
                    if lines != 0. {
                        t.scroll -= lines;
                        t.selection = None;
                        // Shift + wheel keeps scrolling the local scrollback.
                        if t.tracking() && !e.mods.shift && t.status().live {
                            let at = t.cell_at(e.x, e.y, cell);
                            t.send_wheel(at, lines as i32);
                        } else {
                            t.send(Command::Scroll(lines as isize));
                        }
                    }
                    true
                })
                .child(term::grid(&format!("g{block}"), t, m, look));
            let mut grid = grid;
            if let Some(edit) = &t.search {
                let (edit_key, change_key) = (key.clone(), key.clone());
                let status = t.frame.as_ref().map_or_else(
                    || "0/0".into(),
                    |frame| match frame.search_index {
                        Some(index) => format!("{}/{}", index + 1, frame.search_total),
                        None => format!("0/{}", frame.search_total),
                    },
                );
                grid = grid.child(
                    row()
                        .key(format!("search:{block}"))
                        .w(260.)
                        .h(34.)
                        .px(2.)
                        .center_y()
                        .radius(8.)
                        .continuous_corners()
                        .fill(rgb(GROUND))
                        .border(1., white(0.12))
                        .box_shadow(shadow(0., 4., 16., black(0.45)))
                        .pin(None, Some(6.), Some(6.), None)
                        .child(
                            TextInput::new(edit)
                                .placeholder("Search scrollback")
                                .leading_icon(&SEARCH)
                                .leading_icon_size(14.)
                                .text_style(ui(13.))
                                .height(32.)
                                .plain()
                                .trailing(text(status).font(medium(10.5)).color(rgb(MUTED)))
                                .on_edit(move |s: &mut Workspace| s.search_input(&edit_key))
                                .on_change(move |s: &mut Workspace, _: &mut Tx| {
                                    s.search_changed(&change_key)
                                })
                                .on_key(super::input::search_key)
                                .width(None)
                                .build(kit())
                                .w_full(),
                        ),
                );
            }
            // The terminal's background sits inside the card's border, never over it. Where
            // it is the card's own ground (the built-in theme), the card draws it, edge included.
            let (edge, radius) = if bleed {
                (0., 0.)
            } else {
                (1., CARD_RADIUS - 1.)
            };
            let mut body = col()
                .grow(1.)
                .self_stretch()
                .min_h(0.)
                .mx(edge)
                .mb(edge)
                .px(PADDING + 4. - edge)
                .pt(if bleed { PADDING } else { 0. })
                .pb(PADDING - edge)
                .corners([0., 0., radius, radius])
                .continuous_corners();
            let fill = t.body(&colors);
            if fill != GROUND {
                body = body.fill(rgb(fill));
            }
            c.child(body.instant_changes().motion(STILL).child(grid))
        }
        None => c.child(
            col().grow(1.).center().child(
                text(if s.model().connected {
                    "Opening terminal…"
                } else {
                    "Waiting for host…"
                })
                .font(ui(13.))
                .color(rgb(MUTED)),
            ),
        ),
    };
    // With no header to hold it, the strip floats over the pane's top-right corner.
    match strip(s, block, rect[2], m).filter(|_| bleed) {
        Some(strip) => c.child(strip.pin(None, Some(8.), Some(12.), None)),
        None => c,
    }
}

fn empty(s: &Workspace) -> El {
    let model = s.model();
    let (title, detail, label, intent) = if !model.connected {
        (
            "Host unavailable",
            model.status.clone(),
            "Retry connection",
            Intent::Retry,
        )
    } else if model.selected.is_some() {
        (
            "This session has no tabs",
            "Create a tab to start a terminal.".into(),
            "New tab",
            Intent::NewWindow,
        )
    } else if model.sessions.is_empty() {
        (
            "Your workspace starts here",
            "Create a session on this host to begin.".into(),
            "New session",
            Intent::NewSession,
        )
    } else {
        (
            "Choose a session",
            "Select a session in the sidebar or press Ctrl K.".into(),
            "New session",
            Intent::NewSession,
        )
    };
    col().key("empty").w_full().h_full().center().child(
        glass(
            col().w(420.).padding(28.).gap(12.).items_center(),
            16.,
            false,
        )
        .child(super::widgets::app_logo(s, 44.))
        .child(text(title).font(strong(18.)).color(rgb(INK)))
        .child(
            text(detail)
                .font(ui(13.))
                .color(rgb(MUTED))
                .wrap()
                .max_w(360.)
                .align(rune::Align::Center),
        )
        .child(el().h(4.))
        .child(
            pill(
                "empty-action",
                label,
                Pill::Primary,
                32.,
                move |s: &mut Workspace, tx: &mut Tx| {
                    s.perform(intent.clone(), tx);
                    tx.blur();
                },
            )
            .preserve_focus_on_press(true),
        ),
    )
}

/// The block a pane-specific panel targets, if one is open.
fn target_block(s: &Workspace) -> Option<String> {
    let panel = s.panel.as_ref()?;
    matches!(
        panel.kind,
        PanelKind::Pane | PanelKind::Form(Form::RenamePane(_)) | PanelKind::Confirm { .. }
    )
    .then(|| panel.target.as_ref().map(|t| t.block.clone()))
    .flatten()
}

/// `px` is device pixels per point, for drawing terminal lines on whole pixels.
pub fn area(s: &Workspace, px: f32) -> El {
    let m = Metrics {
        px,
        ..term::metrics(s.settings.font_size)
    };
    let mut area = el()
        .key("panes")
        .grow(1.)
        .h_full()
        .min_w(0.)
        .on_layout(|s: &mut Workspace, r| s.area = r);
    let Some(current) = s.model().window() else {
        return area.child(empty(s));
    };
    let focused = s.focused_block();
    let target = target_block(s);
    let preview = s.drag.as_ref().map(|d| (d.divider.key(), d.ratio));
    let (panes, dividers, column_edges) =
        s.arrange(current, preview.as_ref().map(|(k, r)| (k.as_str(), *r)));
    let bleed = full_bleed(current);
    let scrolls = current
        .window_id
        .as_deref()
        .is_some_and(|id| s.tab_layout(id) == TabLayout::Scrolling);
    if scrolls {
        // Columns off either side are cut at the area's edges. Over a pane only sideways
        // scrolling arrives here; over gutters and headers the plain wheel pans as well.
        area = area
            .clip_rounded(0.)
            .on_wheel(|s: &mut Workspace, e: WheelEvent, tx: &mut Tx| {
                let delta = if e.dx.abs() >= e.dy.abs() { e.dx } else { e.dy };
                let scale = if e.precise { s.ui_scale() } else { 1. };
                let width = inner(s)[2] * scale;
                let moved = width > 0. && s.scroll_strip(-f64::from(delta / width));
                let ended = e.precise && e.phase == GesturePhase::Ended;
                let snapped = ended && s.snap_strip();
                if !moved && !snapped && !ended {
                    return false;
                }
                if !ended {
                    tx.instant();
                }
                true
            });
    }
    for placement in panes {
        let rect = if bleed {
            [0., 0., s.area[2], s.area[3]]
        } else {
            card_rect(s, &placement)
        };
        let is_focus = focused.as_deref() == Some(placement.block.as_str());
        let state = if target.as_deref() == Some(placement.block.as_str()) {
            CardState::Target
        } else if is_focus && s.panel.is_none() {
            CardState::Focused
        } else {
            CardState::Plain
        };
        area = area.child(card(
            s,
            &placement.block,
            rect,
            &m,
            state,
            is_focus && s.panel.is_none(),
            bleed,
        ));
    }
    let [ix, iy, iw, ih] = inner(s);
    for edge in column_edges {
        let before = edge.before.to_owned();
        let after = edge.after.to_owned();
        let dragging = s
            .column_drag
            .as_ref()
            .is_some_and(|drag| drag.before == before && drag.after == after);
        let key = format!("column-divider:{before}:{after}");
        let drag_before = before.clone();
        let drag_after = after.clone();
        area = area.child(
            divider_handle(
                el().key(key)
                    .absolute(ix + edge.x as f32 * iw - 5., iy)
                    .w(10.)
                    .h(ih),
                true,
                dragging,
            )
            .on_drag(move |s: &mut Workspace, g, tx| {
                let same = s
                    .column_drag
                    .as_ref()
                    .is_some_and(|drag| drag.before == drag_before && drag.after == drag_after);
                if g.phase == Phase::Down || !same {
                    s.begin_column_drag(drag_before.clone(), drag_after.clone());
                }
                let width = inner(s)[2] * s.ui_scale();
                if width > 0. {
                    s.update_column_drag(f64::from(g.dx / width));
                }
                tx.instant();
                if g.phase == Phase::Up {
                    s.finish_column_drag(g.cancelled());
                }
            }),
        );
    }
    for divider in dividers {
        let [x, y, w, h] = divider.rect;
        let horizontal = divider.horizontal;
        let dragging = s
            .drag
            .as_ref()
            .is_some_and(|d| d.divider.key() == divider.key());
        let handle = if horizontal {
            el().absolute(
                ix + (x + w * divider.ratio) as f32 * iw - 5.,
                iy + y as f32 * ih,
            )
            .w(10.)
            .h(h as f32 * ih)
        } else {
            el().absolute(
                ix + x as f32 * iw,
                iy + (y + h * divider.ratio) as f32 * ih - 5.,
            )
            .h(10.)
            .w(w as f32 * iw)
        };
        let d = divider.clone();
        area = area.child(
            divider_handle(
                handle.key(format!("divider:{}", divider.key())),
                horizontal,
                dragging,
            )
            .on_drag(move |s: &mut Workspace, g, tx| {
                let Some(session) = s.model().selected.clone() else {
                    return;
                };
                if g.phase == Phase::Down || s.drag.is_none() {
                    s.drag = Some(DividerDrag {
                        host: s.active,
                        session,
                        divider: d.clone(),
                        start: d.ratio,
                        ratio: d.ratio,
                    });
                }
                let [_, _, iw, ih] = inner(s);
                let scale = s.ui_scale() as f64;
                if let Some(drag) = &mut s.drag {
                    let [_, _, w, h] = drag.divider.rect;
                    let moved = if drag.divider.horizontal {
                        g.dx as f64 / (iw as f64 * w * scale)
                    } else {
                        g.dy as f64 / (ih as f64 * h * scale)
                    };
                    drag.ratio = (drag.start + moved).clamp(0.1, 0.9);
                }
                tx.instant();
                if g.phase == Phase::Up {
                    if g.cancelled() {
                        s.drag = None;
                    } else {
                        s.finish_drag();
                    }
                }
            }),
        );
    }
    area
}

/// An invisible drag target in a gutter with a 2pt line: faint under the pointer, accent while
/// dragging.
pub fn divider_handle(e: El, vertical_line: bool, dragging: bool) -> El {
    let line = if vertical_line {
        el().pin(Some(4.), Some(8.), None, Some(8.)).w(2.)
    } else {
        el().pin(Some(8.), Some(4.), Some(8.), None).h(2.)
    };
    let line = line.radius(1.);
    let line = if dragging {
        line.fill(rgb(ACCENT))
            .box_shadow(shadow(0., 0., 10., rgb(ACCENT).alpha(0.5)))
    } else {
        line.fill(white(0.18)).group_opacity(0., 1.)
    };
    e.cursor(if vertical_line {
        Cursor::ResizeColumn
    } else {
        Cursor::ResizeRow
    })
    .hover_group()
    .preserve_focus_on_press(true)
    .child(line)
}
