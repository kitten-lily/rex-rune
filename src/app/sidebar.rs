//! The optional host → session sidebar. It slides in and out; rows activate on click only
//! (keyboard session navigation is the Ctrl K switcher, so arrows never switch sessions).
use super::{
    activity::{
        TabActivity, activity_word, program_icon, progress_bar, tab_activity, tab_name,
        tab_progress, tab_word,
    },
    activity_list::{ActivityGroup, ActivityRow, TaskLine, activity_groups},
    panel::Intent,
    panes::divider_handle,
    theme::*,
    widgets::{chrome_button, glyph},
    workspace::Workspace,
};
use rex_linux::{
    connection::{
        HostCommand,
        activity::{ProgramState, TerminalProgress},
    },
    settings::{ActivityGrouping, SidebarMode},
};
use rune::{Cursor, Cx, Edge, El, ScrollSpec, SpringPair, Transition, Tx, col, el, row, text};

const HOST_ROW: f32 = 28.;
const SESSION_ROW: f32 = 32.;
const NOTE_ROW: f32 = 28.;
const GROUP_GAP: f32 = 12.;
const GROUP_LABEL_ROW: f32 = 26.;
const ACTIVITY_ROW: f32 = 44.;
const TASK_ROW: f32 = 24.;
const SEGMENT: f32 = 26.;

fn host_header(s: &Workspace, index: usize) -> El {
    let config = &s.settings.hosts[index];
    let connected = s.hosts[index].model.connected;
    let active = index == s.active;
    // An offline host dims; its Retry connection row below says why.
    let tone = rgb(if active && connected { ink() } else { muted() });
    let new_help = format!("New session on {}", config.label);
    // One step to a new session; offline hosts keep the slot so the column lines up.
    let plus = if connected {
        el().key(format!("host-new-session:{index}"))
            .role("button")
            .label(new_help.clone())
            .help(new_help)
            .size(22., 22.)
            .center()
            .radius(6.)
            .hover(white(0.06))
            .press(white(0.1))
            .cursor(Cursor::Pointer)
            .preserve_focus_on_press(true)
            .tab_index(-1)
            .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
                s.create_session(index, None, tx);
                tx.blur();
            })
            .child(glyph(&PLUS, 13., rgb(muted())))
    } else {
        el().key(format!("host-new-session-slot:{index}"))
            .size(22., 22.)
    };
    row()
        .key(format!("host:{index}"))
        .h(HOST_ROW)
        .pl(2.)
        // The + lines up under the panel header's +.
        .pr(1.)
        .gap(4.)
        .center_y()
        .on_secondary_press(move |s: &mut Workspace, p, _| s.host_menu(index, (p.x, p.y)))
        .child(
            el().key(format!("host-disclosure:{index}"))
                .role("button")
                .label(if config.collapsed {
                    "Expand host"
                } else {
                    "Collapse host"
                })
                .size(18., 22.)
                .center()
                .radius(6.)
                .hover(white(0.06))
                .cursor(Cursor::Pointer)
                .preserve_focus_on_press(true)
                .help(if config.collapsed {
                    "Expand host"
                } else {
                    "Collapse host"
                })
                .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
                    s.settings.hosts[index].collapsed = !s.settings.hosts[index].collapsed;
                    s.save();
                    tx.blur();
                })
                .child(glyph(
                    if config.collapsed { &RIGHT } else { &CHEVRON },
                    13.,
                    rgb(muted()),
                )),
        )
        .child(
            row()
                .key(format!("host-label:{index}"))
                .role("button")
                .label(config.label.clone())
                .grow(1.)
                .shrink()
                .h_full()
                .gap(6.)
                .center_y()
                .cursor(Cursor::Pointer)
                .preserve_focus_on_press(true)
                .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
                    s.activate_host(index, tx);
                    tx.blur();
                })
                .child(glyph(s.host_icon(index), 13., tone))
                .child(
                    text(config.label.clone())
                        .font(label_caps())
                        .text_case(rune::TextCase::Uppercase)
                        .color(tone)
                        .truncate()
                        .grow(1.)
                        .shrink(),
                ),
        )
        .child(plus)
}

fn session_row(s: &Workspace, index: usize, id: &str, label: &str) -> El {
    let host = &s.hosts[index];
    let selected = index == s.active && host.model.selected.as_deref() == Some(id);
    let count = host.model.views.get(id).map(|v| v.windows.len());
    let target = id.to_owned();
    let menu_id = id.to_owned();
    // The session a session menu, its rename form or its close confirm acts on.
    let targeted = s
        .panel
        .as_ref()
        .and_then(|p| p.session.as_ref())
        .is_some_and(|t| t.host == index && t.session == id);
    let mut e = row()
        .key(format!("session:{index}:{id}"))
        .role("button")
        .label(if selected {
            format!("{label} (current)")
        } else {
            label.to_owned()
        })
        .h(if s.settings.vertical_tabs {
            24.
        } else {
            SESSION_ROW
        })
        .px(10.)
        .gap(10.)
        .center_y()
        .radius(9.)
        .continuous_corners()
        .cursor(Cursor::Pointer)
        .preserve_focus_on_press(true)
        .help_when_truncated(format!("{} · {label}", s.settings.hosts[index].label))
        .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
            s.select(index, target.clone(), tx);
            tx.blur();
        })
        .on_secondary_press(move |s: &mut Workspace, p, _| {
            s.session_menu(index, menu_id.clone(), (p.x, p.y))
        });
    if selected && !s.settings.vertical_tabs {
        e = e.child(
            el().pin(Some(0.), Some(0.), Some(0.), Some(0.))
                .radius(9.)
                .continuous_corners()
                .fill(white(0.1))
                .box_shadow(inset(1., white(0.07)))
                .box_shadow(shadow(0., 1., 3., black(0.3)))
                .matched("sb-current")
                .transition(no_presence())
                .motion(SNAPPY),
        );
    } else {
        e = e.hover(white(0.05));
    }
    e = e
        .child(if s.settings.vertical_tabs {
            el()
        } else {
            glyph(
                &TERMINAL,
                14.,
                rgb(if selected { accent() } else { muted() }),
            )
        })
        .child(
            text(label.to_owned())
                .font(if s.settings.vertical_tabs {
                    medium(11.5)
                } else if selected {
                    medium(13.)
                } else {
                    ui(13.)
                })
                .color(rgb(if selected && !s.settings.vertical_tabs {
                    ink()
                } else {
                    muted()
                }))
                .truncate()
                .grow(1.)
                .shrink(),
        );
    if let Some((word, tone, needing)) = s.session_word(index, id) {
        e = e.child(
            text(word)
                .key(format!("session-word:{index}:{id}"))
                .font(medium(10.5))
                .color(rgb(tone)),
        );
        if needing >= 2 {
            e = e.child(
                text(needing.to_string())
                    .key(format!("session-count:{index}:{id}"))
                    .font(medium(10.5))
                    .color(rgb(warning())),
            );
        }
    } else if let Some(count) = count {
        e = e.child(
            text(count.to_string())
                .font(medium(10.5))
                .color(rgb(muted())),
        );
    }
    if let Some(progress) = s.session_progress(index, id) {
        e = e.child(progress_bar(
            format!("session-progress:{index}:{id}"),
            progress,
            10.,
        ));
    }
    if targeted {
        // An overlay, so the outline never shifts the row; above the selected fill.
        e = e.child(
            el().key(format!("session-target:{index}:{id}"))
                .pin(Some(0.), Some(0.), Some(0.), Some(0.))
                .radius(9.)
                .continuous_corners()
                .border(1.5, rgb(accent())),
        );
    }
    e
}

fn window_row(s: &Workspace, index: usize, session: &str, tab: &rexrs::api::session::Window) -> El {
    let Some(id) = tab.window_id.as_deref() else {
        return el();
    };
    let model = &s.hosts[index].model;
    let active = index == s.active
        && model.selected.as_deref() == Some(session)
        && model
            .views
            .get(session)
            .is_some_and(|v| v.active_window_id.as_deref() == Some(id));
    let activity = tab_activity(model, tab);
    let label = tab_name(model, tab);
    let (target_session, target_window) = (session.to_owned(), id.to_owned());
    let mut row = row()
        .key(format!("sidebar-tab:{index}:{session}:{id}"))
        .role("tab")
        .label(label.clone())
        .h(32.)
        .ml(12.)
        .pl(10.)
        .pr(8.)
        .gap(8.)
        .center_y()
        .radius(10.)
        .continuous_corners()
        .cursor(Cursor::Pointer)
        .preserve_focus_on_press(true)
        .help_when_truncated(label.clone())
        .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
            s.select_window(index, target_session.clone(), target_window.clone(), tx);
            tx.blur();
        })
        .child(glyph(
            program_icon(activity.program.as_deref()),
            15.,
            rgb(if active { ink() } else { muted() }),
        ))
        .child(
            text(label)
                .font(if active { medium(12.5) } else { ui(12.5) })
                .color(rgb(if active { ink() } else { muted() }))
                .truncate()
                .grow(1.)
                .min_w(0.),
        );
    if active {
        row = row
            .fill(white(0.11))
            .border_gradient(1., white(0.15), white(0.04));
    } else {
        row = row.hover(white(0.05));
    }
    if s.settings.notifications.show_in_tabs
        && let Some((word, tone)) =
            tab_word(&activity, s.unseen_on(index, id), s.bell_on(index, id))
    {
        row = row.child(text(word).font(medium(10.5)).color(rgb(tone)));
    }
    if s.settings.notifications.show_in_tabs
        && let Some(progress) = tab_progress(&activity)
    {
        row = row.child(progress_bar(format!("tab-progress:{id}"), progress, 10.));
    }
    row
}

fn note(text_: &str, color: u32) -> El {
    row()
        .h(NOTE_ROW)
        .px(12.)
        .center_y()
        .child(text(text_.to_owned()).font(ui(12.5)).color(rgb(color)))
}

fn new_session(s: &Workspace, index: usize) -> El {
    let help = format!("New session on {}", s.settings.hosts[index].label);
    row()
        .key(format!("new-session-on:{index}"))
        .role("button")
        .label(help.clone())
        .help(help)
        .h(SESSION_ROW)
        .px(10.)
        .gap(10.)
        .center_y()
        .radius(9.)
        .continuous_corners()
        .fill(white(0.04))
        .box_shadow(inset(1., white(0.06)))
        .hover(white(0.08))
        .press(white(0.12))
        .cursor(Cursor::Pointer)
        .preserve_focus_on_press(true)
        .tab_index(-1)
        .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
            s.perform(Intent::CreateOn(index, None), tx);
            tx.blur();
        })
        .child(glyph(&PLUS, 14., rgb(accent())))
        .child(
            text("New session")
                .font(medium(12.5))
                .color(rgb(accent()))
                .truncate(),
        )
}

/// One segment of a two-way switch: the active one on the tab pill's fill and hairline.
fn segment(
    key: &str,
    label: &'static str,
    active: bool,
    f: impl Fn(&mut Workspace) + 'static,
) -> El {
    let e = row()
        .key(key.to_owned())
        .role("tab")
        .label(label)
        .h(SEGMENT)
        .px(10.)
        .center()
        .radius(8.)
        .continuous_corners()
        .cursor(Cursor::Pointer)
        .preserve_focus_on_press(true)
        .tab_index(-1)
        .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
            f(s);
            tx.blur();
        })
        .child(
            text(label)
                .font(if active { medium(12.) } else { ui(12.) })
                .color(rgb(if active { ink() } else { muted() })),
        );
    if active {
        e.fill(white(0.11))
            .border_gradient(1., white(0.15), white(0.04))
    } else {
        e.hover(white(0.05))
    }
}

fn mode_switch(s: &Workspace) -> El {
    let mode = s.settings.sidebar_mode;
    row()
        .role("tablist")
        .gap(2.)
        .center_y()
        .child(segment(
            "sidebar-mode:sessions",
            "Sessions",
            mode == SidebarMode::Sessions,
            |s| s.set_sidebar_mode(SidebarMode::Sessions),
        ))
        .child(segment(
            "sidebar-mode:activity",
            "Activity",
            mode == SidebarMode::Activity,
            |s| s.set_sidebar_mode(SidebarMode::Activity),
        ))
}

/// The word a subtask shows: `activity_word` for its own record alone.
fn task_word(task: &TaskLine) -> (std::borrow::Cow<'static, str>, u32) {
    let r = &task.record;
    activity_word(&TabActivity {
        state: r.state,
        kind: r.kind,
        msg: r.msg.clone(),
        progress: r
            .progress
            .filter(|_| r.state == ProgramState::Working)
            .map(|n| TerminalProgress::Set(Some(n))),
        ..TabActivity::default()
    })
}

fn task_line(index: usize, block: &str, task: &TaskLine) -> El {
    let (word, tone) = task_word(task);
    row()
        .key(format!("activity-task:{index}:{block}:{}", task.id))
        .h(TASK_ROW)
        .ml(12. + 12. * task.depth as f32)
        .pl(10.)
        .pr(8.)
        .gap(8.)
        .center_y()
        .child(
            text(task.title.clone())
                .font(ui(11.5))
                .color(rgb(muted()))
                .truncate()
                .help_when_truncated(task.title.clone())
                .grow(1.)
                .min_w(0.),
        )
        .child(text(word).font(medium(10.5)).color(rgb(tone)))
}

fn activity_row(index: usize, row_: &ActivityRow) -> El {
    let (word, tone) = activity_word(&row_.activity);
    let target = row_.target.clone();
    let mut line = row()
        .gap(8.)
        .center_y()
        .child(
            text(row_.name.clone())
                .font(medium(12.5))
                .color(rgb(ink()))
                .truncate()
                .help_when_truncated(row_.name.clone())
                .grow(1.)
                .min_w(0.),
        )
        .child(text(word.clone()).font(medium(10.5)).color(rgb(tone)));
    if let Some(msg) = &row_.activity.msg {
        line = line.help(msg.clone());
    }
    let mut body = col().grow(1.).min_w(0.).gap(2.).child(line);
    if !row_.detail.is_empty() {
        body = body.child(
            text(row_.detail.clone())
                .font(ui(11.))
                .color(rgb(muted()))
                .truncate()
                .help_when_truncated(row_.detail.clone()),
        );
    }
    row()
        .key(format!("activity-row:{index}:{}", row_.target.block))
        .role("button")
        .label(format!("{} · {word}", row_.name))
        .h(ACTIVITY_ROW)
        .px(10.)
        .gap(10.)
        .center_y()
        .radius(9.)
        .continuous_corners()
        .hover(white(0.05))
        .press(white(0.09))
        .cursor(Cursor::Pointer)
        .preserve_focus_on_press(true)
        .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
            s.open_pane(&target, tx);
            tx.blur();
        })
        .child(glyph(program_icon(row_.icon_program()), 15., rgb(muted())))
        .child(body)
}

fn activity_group(s: &Workspace, n: usize, group: &ActivityGroup) -> (El, f32) {
    let mut height = GROUP_LABEL_ROW;
    let mut e = col().gap(2.).child(
        row().h(GROUP_LABEL_ROW).px(10.).center_y().child(
            text(group.title.clone())
                .key(format!("activity-group:{n}"))
                .font(label_caps())
                .text_case(rune::TextCase::Uppercase)
                .color(rgb(muted()))
                .truncate()
                .help_when_truncated(group.title.clone())
                .grow(1.)
                .min_w(0.),
        ),
    );
    for row_ in &group.rows {
        let Some(index) = s.host_index(row_.target.host) else {
            continue;
        };
        e = e.child(activity_row(index, row_));
        height += ACTIVITY_ROW + 2.;
        for task in &row_.tasks {
            e = e.child(task_line(index, &row_.target.block, task));
            height += TASK_ROW + 2.;
        }
    }
    (e.pb(GROUP_GAP), height + GROUP_GAP)
}

/// The panes reporting activity on every connected host, grouped as the user chose.
fn activity_list(s: &Workspace) -> (El, f32) {
    let hosts: Vec<_> = s
        .hosts
        .iter()
        .zip(&s.settings.hosts)
        .map(|(h, config)| (h.id, config.label.as_str(), &h.model))
        .collect();
    let groups = activity_groups(
        &hosts,
        &s.unseen_panes,
        s.settings.activity_grouping,
        s.settings.activity_show_idle,
    );
    let mut list = col().key("sidebar-content").px(8.);
    if groups.is_empty() {
        let empty = row().key("activity-empty").px(10.).py(6.).child(
            text("No programs are reporting activity.")
                .font(ui(12.))
                .color(rgb(muted()))
                .wrap()
                .grow(1.)
                .min_w(0.),
        );
        return (list.child(empty), NOTE_ROW);
    }
    let mut height = 0.;
    for (n, group) in groups.iter().enumerate() {
        let (e, h) = activity_group(s, n, group);
        list = list.child(e);
        height += h;
    }
    (list, height)
}

/// The Activity list's bottom bar: grouping and whether idle agents are listed.
fn activity_bar(s: &Workspace) -> El {
    let grouping = s.settings.activity_grouping;
    let idle = s.settings.activity_show_idle;
    let set = |g: ActivityGrouping| {
        move |s: &mut Workspace| {
            s.settings.activity_grouping = g;
            s.save();
        }
    };
    let mut toggle = row()
        .key("activity-idle")
        .role("button")
        .label(if idle {
            "Hide idle agents"
        } else {
            "Show idle agents"
        })
        .help(if idle {
            "Hide idle agents"
        } else {
            "Show idle agents"
        })
        .h(SEGMENT)
        .px(10.)
        .center()
        .radius(8.)
        .continuous_corners()
        .cursor(Cursor::Pointer)
        .preserve_focus_on_press(true)
        .tab_index(-1)
        .on_press_tx(|s: &mut Workspace, tx: &mut Tx| {
            s.settings.activity_show_idle = !s.settings.activity_show_idle;
            s.save();
            tx.blur();
        })
        .child(
            text("Idle")
                .font(if idle { medium(12.) } else { ui(12.) })
                .color(rgb(if idle { ink() } else { muted() })),
        );
    toggle = if idle {
        toggle
            .fill(white(0.11))
            .border_gradient(1., white(0.15), white(0.04))
    } else {
        toggle.hover(white(0.05))
    };
    row()
        .h(40.)
        .px(8.)
        .center_y()
        .justify_between()
        .child(
            row()
                .role("tablist")
                .gap(2.)
                .center_y()
                .child(segment(
                    "activity-grouping:priority",
                    "Priority",
                    grouping == ActivityGrouping::Priority,
                    set(ActivityGrouping::Priority),
                ))
                .child(segment(
                    "activity-grouping:session",
                    "Session",
                    grouping == ActivityGrouping::Session,
                    set(ActivityGrouping::Session),
                )),
        )
        .child(toggle)
}

/// Hosts and their sessions (and with Sidebar tab position, their tabs).
fn session_list(s: &Workspace) -> (El, f32) {
    let mut list = col().key("sidebar-content").px(8.);
    let mut height = 0.;
    for (index, host) in s.hosts.iter().enumerate() {
        let config = &s.settings.hosts[index];
        let mut group = col()
            .key(format!("group:{index}"))
            .gap(2.)
            .child(host_header(s, index));
        height += HOST_ROW;
        if !config.collapsed {
            for session in &host.model.sessions {
                group = group.child(session_row(s, index, &session.session_id, &session.label));
                height += if s.settings.vertical_tabs {
                    26.
                } else {
                    SESSION_ROW + 2.
                };
                if s.settings.vertical_tabs
                    && let Some(view) = host.model.views.get(&session.session_id)
                {
                    for tab in &view.windows {
                        if tab.window_id.is_some() {
                            group = group.child(window_row(s, index, &session.session_id, tab));
                            height += 34.;
                        }
                    }
                }
            }
            if host.model.sessions.is_empty() {
                if host.model.connected {
                    group = group.child(new_session(s, index));
                    height += SESSION_ROW + 2.;
                } else {
                    group = group.child(note("Waiting for connection…", muted()));
                    height += NOTE_ROW;
                }
            }
            if !host.model.connected {
                let status = host.model.status.clone();
                group = group.child(
                    note("Retry connection", accent())
                        .key(format!("retry-host:{index}"))
                        .role("button")
                        .radius(9.)
                        .hover(white(0.05))
                        .cursor(Cursor::Pointer)
                        .preserve_focus_on_press(true)
                        .help(status)
                        .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| {
                            s.send(index, HostCommand::Reconnect);
                            tx.blur();
                        }),
                );
                height += NOTE_ROW;
            }
        }
        list = list.child(group.pb(GROUP_GAP));
        height += GROUP_GAP;
    }
    (list, height)
}

pub fn sidebar(s: &Workspace, cx: &mut Cx<Workspace>) -> El {
    let activity = s.settings.sidebar_mode == SidebarMode::Activity;
    let (list, height) = if activity {
        activity_list(s)
    } else {
        session_list(s)
    };
    let viewport = cx.layout_size("sidebar-list").map_or(0., |[_, h]| h);
    let overflow = viewport > 0. && height > viewport + 1.;
    let width = s.sidebar.width();
    let fx = &s.sidebar_fx;
    let mut scroller = col()
        .key("sidebar-list")
        .grow(1.)
        .min_h(0.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .scroll_edge_fade(12.)
        .child(list);
    if overflow {
        scroller =
            scroller.on_scroll_offset(fx.watch(width, viewport, move |offset, max| {
                match (activity, offset < max - 1.) {
                    (false, true) => "More sessions and hosts below".into(),
                    (false, false) => "Earlier sessions and hosts above".into(),
                    (true, true) => "More activity below".into(),
                    (true, false) => "Earlier activity above".into(),
                }
            }));
    }
    let total: usize = s.hosts.iter().map(|h| h.model.sessions.len()).sum();
    let mut area = col().grow(1.).min_h(0.).child(scroller);
    if overflow {
        area = area.child(fx.thumb_el());
    }
    let find = chip(
        row()
            .key("session-search")
            .role("button")
            .label("Find a session")
            .h(32.)
            .mx(8.)
            .px(10.)
            .gap(8.)
            .center_y()
            .hover(white(0.08))
            .cursor(Cursor::Pointer)
            .preserve_focus_on_press(true)
            .on_press_tx(|s: &mut Workspace, tx: &mut Tx| {
                s.picker(true);
                tx.blur();
            })
            .child(glyph(&SEARCH, 14., rgb(muted())))
            .child(
                text("Find a session")
                    .font(ui(12.5))
                    .color(rgb(muted()))
                    .grow(1.),
            )
            .child(text("⌃K").font(medium(10.5)).color(rgb(muted()))),
    );
    let mut panel = col()
        .w_full()
        .grow(1.)
        .min_h(0.)
        .radius(CARD_RADIUS)
        .continuous_corners()
        .fill(white(0.025))
        .border_gradient(1., white(0.07), white(0.02))
        .pt(8.)
        .gap(8.)
        .child(
            row()
                .h(30.)
                .pl(8.)
                .pr(6.)
                .center_y()
                .justify_between()
                .child(mode_switch(s))
                .child(if activity {
                    el()
                } else {
                    chrome_button(
                        "new-session",
                        &PLUS,
                        "New session… · choose a host",
                        28.,
                        |s, tx| s.perform(Intent::ChooseHost(String::new()), tx),
                    )
                }),
        )
        .child(find)
        .child(area);
    if overflow {
        panel = panel.child(fx.hint_el());
    }
    panel = panel.child(if activity {
        activity_bar(s)
    } else {
        row()
            .h(40.)
            .px(8.)
            .center_y()
            .justify_between()
            .child(
                row()
                    .key("add-host")
                    .role("button")
                    .label("Add host")
                    .h(28.)
                    .px(8.)
                    .gap(6.)
                    .center_y()
                    .radius(8.)
                    .hover(white(0.06))
                    .cursor(Cursor::Pointer)
                    .preserve_focus_on_press(true)
                    .on_press_tx(|s: &mut Workspace, tx: &mut Tx| s.perform(Intent::AddHost, tx))
                    .child(glyph(&PLUS, 14., rgb(muted())))
                    .child(text("Add host").font(ui(12.)).color(rgb(muted()))),
            )
            .child(
                text(format!("{total} sessions"))
                    .font(ui(11.))
                    .color(rgb(muted()))
                    .pr(6.),
            )
    });
    col()
        .key("sidebar")
        .w(width)
        .h_full()
        .pb(MARGIN)
        .pl(MARGIN)
        .transition(
            Transition::move_edge(Edge::Leading)
                .combined(Transition::opacity())
                .springs(SpringPair::new(SNAPPY, SNAPPY)),
        )
        .child(panel)
}

/// The sidebar's resize edge, in the gutter beside it.
pub fn sidebar_edge(s: &Workspace) -> El {
    divider_handle(
        el().key("sidebar-edge").w(10.).ml(-1.).mr(-9.).h_full(),
        true,
        s.sidebar_drag_start.is_some(),
    )
    .on_drag(|s: &mut Workspace, d, tx| {
        let start = *s.sidebar_drag_start.get_or_insert(s.sidebar.width());
        s.sidebar.drag_to(start + d.dx / s.ui_scale());
        tx.instant();
        if d.phase == rune::Phase::Up {
            s.sidebar_drag_start = None;
            s.save();
        }
    })
    .on_double_press(|s: &mut Workspace| {
        s.sidebar.reset_width();
        s.save();
    })
}
