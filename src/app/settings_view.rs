//! Settings: a section list on the left and the chosen section's page on the right. Pages are
//! built from `settings_kit` (sections, cards, rows and their controls).
use super::{
    dialogs::modal_layer,
    keycaps::{Chords, keycaps, spell},
    panel::{Intent, SettingsSection},
    settings_kit::{
        NAV_WIDTH, PANEL_HEIGHT, PANEL_WIDTH, Segment, button, card, disclosure, footnote,
        mono_footnote, page, row_frame, section, segmented, select_row, setting, stepper,
        unavailable, value, value_popup, words,
    },
    theme::*,
    widgets::{Pill, glyph},
    workspace::Workspace,
};
use rex_linux::settings::{
    Channels, FocusCentering, NoticePosition, NotificationsConfig, PaneDensity, PaneHeader,
    SessionList, TabLayout,
};
use rune::{Cursor, El, ScrollPos, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Checkbox, IconButton, Switch};

/// A setting's explanation, in `muted()` or, for a problem, `warning()`.
pub(super) fn detail_text(detail: &str, color: u32) -> El {
    text(detail.to_owned())
        .font(ui(11.5))
        .color(rgb(color))
        .wrap()
}

/// A card row like [`setting`], with an explanation built by the caller.
pub(super) fn setting_with(label: &str, detail: Option<El>, control: El) -> El {
    let helper = detail.is_some();
    let words = col()
        .grow(1.)
        .shrink()
        .min_w(0.)
        .gap(1.)
        .child(
            text(label.to_owned())
                .font(ui(13.))
                .color(rgb(ink()))
                .truncate()
                .help_when_truncated(label.to_owned()),
        )
        .children(detail);
    row_frame(helper).child(words).child(control)
}

/// The page's header row, above its scroll region.
const HEADER: f32 = 56.;
/// The least space between the dialog and the toolbar above it, and the window's bottom edge.
const CLEARANCE: f32 = 12.;
/// The page's inset from the section list and from the dialog's trailing edge.
const PAGE_INSET: f32 = 20.;

/// The Pane headers choices, in the order the control lists them.
const PANE_HEADERS: [PaneHeader; 4] = [
    PaneHeader::Auto,
    PaneHeader::Always,
    PaneHeader::OnHover,
    PaneHeader::Hidden,
];

fn interface(s: &Workspace) -> El {
    let k = kit();
    let percent = (s.settings.ui_scale * 100.).round() as i32;
    let corner = match s.settings.notice_position {
        NoticePosition::TopRight => 0,
        NoticePosition::BottomRight => 1,
    };
    let layout = match s.settings.tab_layout {
        TabLayout::Tiled => 0,
        TabLayout::Scrolling => 1,
    };
    let centering = match s.settings.focus_centering {
        FocusCentering::Never => 0,
        FocusCentering::OnOverflow => 1,
        FocusCentering::Always => 2,
    };
    page([
        section(
            "Appearance",
            [card([
                setting(
                    "Theme",
                    "Terminals take the matching theme until you choose one.",
                    value_popup(
                        "chrome-theme",
                        s.settings.chrome_theme.name(),
                        Intent::ChooseChrome,
                    ),
                ),
                setting(
                    "Interface zoom",
                    "Chrome and terminals together, from 70% to 150%.",
                    row()
                        .gap(8.)
                        .center_y()
                        .child(stepper(
                            "zoom",
                            format!("{percent}%"),
                            |s, tx| s.change_ui_scale(-1, tx),
                            |s, tx| s.change_ui_scale(1, tx),
                        ))
                        .child(button("zoom-reset", "Reset", Pill::Neutral, |s, tx| {
                            s.change_ui_scale(0, tx)
                        })),
                ),
            ])],
        ),
        section(
            "Panes",
            [card([
                setting(
                    "Pane density",
                    "Compact joins panes edge to edge, with no gaps.",
                    segmented(
                        "pane-density",
                        &[
                            Segment::new("Compact").icon(&PANES_JOINED),
                            Segment::new("Comfortable").icon(&PANES_APART),
                        ],
                        Some(usize::from(
                            s.settings.pane_density == PaneDensity::Comfortable,
                        )),
                        |s, i, tx| {
                            s.settings.pane_density = if i == 0 {
                                PaneDensity::Compact
                            } else {
                                PaneDensity::Comfortable
                            };
                            s.save();
                            tx.instant();
                        },
                    ),
                ),
                select_row(
                    "pane-header",
                    "Pane headers",
                    "Auto shows them on split panes in Comfortable density.",
                    &["Auto", "Always", "On hover", "Hidden"],
                    PANE_HEADERS
                        .iter()
                        .position(|h| *h == s.settings.pane_header)
                        .unwrap_or_default(),
                    |s, i, tx| {
                        s.settings.pane_header = PANE_HEADERS[i.min(PANE_HEADERS.len() - 1)];
                        // A header that was brought out by hand goes back with the old choice.
                        s.header_reveal = Default::default();
                        s.save();
                        tx.instant();
                    },
                ),
            ])],
        ),
        section(
            "Sidebar and tabs",
            [card([
                setting(
                    "Session sidebar",
                    "Hosts and their sessions beside the terminals.",
                    Switch::new(s.sidebar.is_open())
                        .label("Show session sidebar")
                        .hide_label()
                        .on_toggle_tx(|s: &mut Workspace, tx: &mut Tx| {
                            s.perform(Intent::Sidebar, tx)
                        })
                        .build(k)
                        .key("sidebar-switch"),
                ),
                setting(
                    "Tab position",
                    "Sidebar tabs show while the sidebar is open.",
                    segmented(
                        "tab-position",
                        &[
                            Segment::new("Top").icon(&TABS_TOP),
                            Segment::new("Sidebar").icon(&SIDEBAR),
                        ],
                        Some(usize::from(s.settings.vertical_tabs)),
                        |s, i, _| s.set_vertical_tabs(i == 1),
                    ),
                ),
                setting(
                    "Session list",
                    "One list names each session's host while two or more are connected.",
                    segmented(
                        "session-list",
                        &[Segment::new("By host"), Segment::new("Flat")],
                        Some(usize::from(
                            s.settings.sidebar_sessions == SessionList::Flat,
                        )),
                        |s, i, _| {
                            s.set_session_list(if i == 1 {
                                SessionList::Flat
                            } else {
                                SessionList::ByHost
                            })
                        },
                    ),
                ),
            ])],
        ),
        section(
            "Notices",
            [card([select_row(
                "notice-position",
                "Position",
                "Where in-app notices appear.",
                &["Top right", "Bottom right"],
                corner,
                |s, i, _| {
                    s.set_notice_position(if i == 0 {
                        NoticePosition::TopRight
                    } else {
                        NoticePosition::BottomRight
                    })
                },
            )])],
        ),
        section(
            "Tab layout",
            [card([
                setting(
                    "Default layout",
                    "Tiled to the window, or columns that scroll sideways. A tab's menu can change it.",
                    segmented(
                        "tab-layout",
                        &[Segment::new("Tiled"), Segment::new("Scrolling")],
                        Some(layout),
                        |s, i, _| {
                            s.set_tab_layout(if i == 0 {
                                TabLayout::Tiled
                            } else {
                                TabLayout::Scrolling
                            })
                        },
                    ),
                ),
                select_row(
                    "focus-centering",
                    "Focus centering",
                    "When focus moves in a scrolling tab. On overflow centers when both columns can't fit.",
                    &["Never", "On overflow", "Always"],
                    centering,
                    |s, i, _| {
                        s.set_focus_centering(match i {
                            1 => FocusCentering::OnOverflow,
                            2 => FocusCentering::Always,
                            _ => FocusCentering::Never,
                        })
                    },
                ),
            ])],
        ),
    ])
}

/// Where a notification checkbox keeps its value.
type Field = fn(&mut NotificationsConfig) -> &mut bool;

/// The channels the table offers, as (key, caption).
const CHANNELS: [(&str, &str); 4] = [
    ("tab", "Tabs"),
    ("desktop", "Desktop"),
    ("badge", "Count"),
    ("sound", "Sound"),
];

/// Width of one channel column of the notifications table.
const CHANNEL_COLUMN: f32 = 64.;

/// One cell of the notifications table: a checkbox saved as soon as it flips, or a dash
/// where the channel doesn't apply to the row, or needs another channel (`why`) first.
fn channel_cell(key: String, label: String, cell: Option<(bool, bool, Field)>) -> El {
    let dash = || text("—").font(ui(12.)).color(rgb(muted())).key(key.clone());
    let inner = match cell {
        Some((on, false, field)) => Checkbox::new(on)
            .label(label)
            .hide_label()
            .on_toggle(move |s: &mut Workspace| {
                let value = field(&mut s.settings.notifications);
                *value = !*value;
                s.sync_prefetch();
                s.update_badge();
                s.save();
            })
            .build(kit())
            .key(key.clone()),
        Some((_, true, _)) => dash().help("Plays with a desktop notification"),
        None => dash(),
    };
    row().w(CHANNEL_COLUMN).center().child(inner)
}

/// The cells one row offers, by channel key, value, whether it waits on another channel,
/// and its field; a channel missing from it doesn't apply.
type Cells = Vec<(&'static str, bool, bool, Field)>;

fn channel_row(state: &str, label: &str, detail: &str, cells: Cells) -> El {
    let mut controls = row().center_y();
    for (channel, caption) in CHANNELS {
        let cell = cells
            .iter()
            .find(|(c, ..)| *c == channel)
            .map(|&(_, on, disabled, field)| (on, disabled, field));
        controls = controls.child(channel_cell(
            format!("notify-{state}-{channel}"),
            format!("{label}: {caption}"),
            cell,
        ));
    }
    setting(label, detail, controls)
}

fn notifications(s: &Workspace) -> El {
    let n = &s.settings.notifications;
    let mut header = row_frame(false).child(el().grow(1.));
    let mut columns = row().center_y();
    for (channel, caption) in CHANNELS {
        columns = columns.child(
            row().w(CHANNEL_COLUMN).center().child(
                text(caption)
                    .key(format!("notify-column-{channel}"))
                    .font(label_caps())
                    .text_case(rune::TextCase::Uppercase)
                    .color(rgb(muted())),
            ),
        );
    }
    header = header.child(columns);
    let states: [(&str, &str, &str, Channels, [Field; 4]); 4] = [
        (
            "blocked",
            "Needs you",
            "Waiting for approval, an answer or a sign-in.",
            n.blocked,
            [
                |n| &mut n.blocked.tab,
                |n| &mut n.blocked.desktop,
                |n| &mut n.blocked.badge,
                |n| &mut n.blocked.sound,
            ],
        ),
        (
            "error",
            "Fails",
            "Reports an error.",
            n.error,
            [
                |n| &mut n.error.tab,
                |n| &mut n.error.desktop,
                |n| &mut n.error.badge,
                |n| &mut n.error.sound,
            ],
        ),
        (
            "done",
            "Finishes",
            "Stops working, or reports that it's done. It counts until you see it.",
            n.done,
            [
                |n| &mut n.done.tab,
                |n| &mut n.done.desktop,
                |n| &mut n.done.badge,
                |n| &mut n.done.sound,
            ],
        ),
        (
            "working",
            "Works",
            "Starts working. Tabs draw its progress as a bar; it never counts.",
            n.working,
            [
                |n| &mut n.working.tab,
                |n| &mut n.working.desktop,
                |n| &mut n.working.badge,
                |n| &mut n.working.sound,
            ],
        ),
    ];
    let mut rows = vec![header];
    for (key, label, detail, c, [tab, desktop, badge, sound]) in states {
        let mut cells: Cells = vec![
            ("tab", c.tab, false, tab),
            ("desktop", c.desktop, false, desktop),
        ];
        if key != "working" {
            cells.push(("badge", c.badge, false, badge));
        }
        // Sound plays with a desktop notification, so it needs one.
        cells.push(("sound", c.sound, !c.desktop, sound));
        rows.push(channel_row(key, label, detail, cells));
    }
    let m = n.messages;
    page([
        section(
            "Notify when a program",
            [
                card(rows),
                footnote(
                    "In a pane you aren't looking at. Desktop notifications go through your desktop's notification service; clicking one shows its pane.",
                ),
            ],
        ),
        section(
            "Programs",
            [card([channel_row(
                "messages",
                "Messages from programs",
                "Notifications programs send themselves with OSC 9 or OSC 777.",
                vec![
                    ("desktop", m.desktop, false, |n| &mut n.messages.desktop),
                    ("sound", m.sound, !m.desktop, |n| &mut n.messages.sound),
                ],
            )])],
        ),
    ])
}

fn terminal(s: &Workspace) -> El {
    let saved = &s.settings.font_family;
    let active = s.terminal_family.unwrap_or("IBM Plex Mono (bundled)");
    // The row's helper is one sentence. What terminals are drawn in now goes under the card,
    // and only while that isn't the family the row names.
    let drawn = if *saved != s.launch_saved_font {
        Some(format!(
            "{saved} is saved and applies when you reopen the client. Until then terminals use {active}."
        ))
    } else if s.terminal_family.is_none() {
        Some(format!(
            "{} isn't installed, so terminals use {active}.",
            s.launch_font
        ))
    } else {
        None
    };
    let fallbacks = if s.settings.font_fallbacks.is_empty() {
        "Automatic".to_owned()
    } else {
        s.settings.font_fallbacks.join(", ")
    };
    let shells = s.settings.hosts.iter().enumerate().map(|(i, host)| {
        disclosure(
            &format!("host-shell:{i}"),
            &host.label,
            "",
            host.shell.as_deref().unwrap_or("Server default"),
            move |s, tx| s.perform(Intent::HostShell(i), tx),
        )
    });
    page([
        section(
            "Typography",
            std::iter::once(card([
                disclosure(
                    "font-change",
                    "Font family",
                    "Applies when you reopen the client.",
                    saved,
                    |s, tx| s.perform(Intent::Fonts, tx),
                ),
                setting(
                    "Font size",
                    "From 8 to 48 pt, for every terminal.",
                    stepper(
                        "font",
                        format!("{} pt", s.settings.font_size),
                        |s, _| s.change_font(-1.),
                        |s, _| s.change_font(1.),
                    ),
                ),
                setting(
                    "Fallback fonts",
                    "For glyphs the font lacks, before the system's own fallbacks.",
                    value(fallbacks).truncate().max_w(260.),
                ),
            ]))
            .chain(drawn.map(|words| footnote(words).key("font-drawn"))),
        ),
        section(
            "Layout",
            [card([setting(
                "Pane padding",
                "Space around each terminal's content, from 0 to 32 pt.",
                stepper(
                    "pane-padding",
                    format!("{} pt", s.settings.pane_padding),
                    |s, _| {
                        s.settings.pane_padding = (s.settings.pane_padding - 1.).max(0.);
                        s.save();
                    },
                    |s, _| {
                        s.settings.pane_padding = (s.settings.pane_padding + 1.).min(32.);
                        s.save();
                    },
                ),
            )])],
        ),
        section(
            "Colours",
            [
                card([setting(
                    "Theme",
                    "",
                    value_popup(
                        "terminal-theme",
                        Workspace::theme_label(&s.settings.terminal_theme),
                        Intent::ChooseTheme,
                    ),
                )]),
                footnote(
                    "Declared to Rex for panes this client owns or nobody owns. Follow server declares nothing until this client has declared a theme once; from then on it resets those panes to Rex's colours, once each per connection.",
                ),
            ],
        ),
        section(
            "Default shell",
            [
                card(shells),
                footnote(
                    "What new sessions, tabs and splits start on each host. Running terminals keep theirs.",
                ),
            ],
        ),
        section(
            "",
            [
                footnote("These settings are saved in"),
                mono_footnote(s.settings_path.display().to_string()).mt(-6.),
            ],
        ),
    ])
}

fn host_card(s: &Workspace, i: usize, last: bool) -> El {
    let host = &s.settings.hosts[i];
    let connected = s.hosts.get(i).is_some_and(|h| h.model.connected);
    let mut actions = row().gap(6.).center_y();
    // Quiet when healthy: only trouble gets words, beside the action that fixes it.
    if !connected {
        actions = actions
            .child(text("Offline").font(medium(12.)).color(rgb(warning())))
            .child(button(
                &format!("host-retry:{i}"),
                "Retry",
                Pill::Warning,
                move |s, tx| s.perform(Intent::RetryHost(i), tx),
            ));
    }
    actions = actions.child(button(
        &format!("host-edit:{i}"),
        "Edit…",
        Pill::Neutral,
        move |s, tx| s.perform(Intent::EditHost(i), tx),
    ));
    actions = actions.child(if last {
        unavailable(
            &format!("host-remove:{i}"),
            "Remove…",
            "The workspace needs at least one host",
        )
    } else {
        button(
            &format!("host-remove:{i}"),
            "Remove…",
            Pill::Danger,
            move |s, tx| s.perform(Intent::RemoveHost(i), tx),
        )
    });
    let mut address = row().gap(6.).center_y().min_w(0.).child(
        text(host.endpoint.clone())
            .font(mono(11.))
            .color(rgb(muted()))
            .truncate()
            .selectable(true),
    );
    if let Some(shell) = &host.shell {
        address = address.child(
            text(format!("· {shell}"))
                .font(mono(11.))
                .color(rgb(muted())),
        );
    }
    card(
        [
            row_frame(true)
                .key(format!("host-row:{i}"))
                .gap(12.)
                .child(glyph(s.host_icon(i), 16., rgb(muted())))
                .child(
                    col()
                        .grow(1.)
                        .min_w(0.)
                        .gap(2.)
                        .child(
                            text(host.label.clone())
                                .font(medium(13.))
                                .color(rgb(ink()))
                                .truncate(),
                        )
                        .child(address),
                )
                .child(actions),
            setting(
                "Remote control",
                "Let rex do and other clients on this host change the view in this window.",
                Switch::new(host.remote_control)
                    .label(format!("Remote control for {}", host.label))
                    .hide_label()
                    .on_toggle(move |s: &mut Workspace| s.toggle_remote_control(i))
                    .build(kit())
                    .key(format!("remote-control:{i}")),
            ),
        ]
        .into_iter()
        .chain(super::tailscale::rows(s, i)),
    )
}

fn hosts(s: &Workspace) -> El {
    let last = s.settings.hosts.len() == 1;
    let cards = (0..s.settings.hosts.len()).map(|i| host_card(s, i, last));
    page([
        section(
            "This client",
            [card([disclosure(
                "client-label",
                "Client label",
                "Its name on every host. Takes effect when you reopen the app.",
                &s.settings.client_label(),
                |s, tx| s.perform(Intent::ClientLabel, tx),
            )])],
        ),
        section(
            "Hosts",
            cards.chain([
                row().px(2.).child(button(
                    "settings-add-host",
                    "Add host…",
                    Pill::Neutral,
                    |s, tx| s.perform(Intent::AddHost, tx),
                )),
                footnote(
                    "Removing a host only stops this app connecting to it; its sessions keep running.",
                ),
            ]),
        ),
    ])
}

/// Presentation only: preserve dispatch priority within each action/argument group.
struct ShortcutRow<'a> {
    binding: &'a rex_linux::keymap::ResolvedBinding,
    also_numpad: bool,
    arrows: bool,
}

fn shortcut_rows<'a>(
    effective: &[&'a rex_linux::keymap::ResolvedBinding],
    category: &str,
) -> Vec<ShortcutRow<'a>> {
    use super::actions::{SPECS, find};
    use rex_linux::keymap::Key;

    let mut list: Vec<_> = effective
        .iter()
        .copied()
        .filter(|b| {
            find(&b.binding.action)
                .map(|a| a.category())
                .unwrap_or("Server")
                == category
        })
        .collect();
    list.sort_by_cached_key(|b| {
        (
            SPECS
                .iter()
                .position(|s| s.name == b.binding.action)
                .unwrap_or(SPECS.len()),
            b.binding.action.clone(),
            b.binding
                .args
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        )
    });
    let default_focus = |b: &&rex_linux::keymap::ResolvedBinding, direction: &str| {
        b.binding.action == "pane.focus"
            && b.binding.source.as_deref() == Some("linux")
            && b.binding.keys == [format!("ctrl+alt+{direction}")]
            && b.binding.args == Some(serde_json::json!({"direction": direction}))
    };
    let arrows = ["left", "right", "up", "down"]
        .iter()
        .all(|d| list.iter().any(|b| default_focus(b, d)));
    let mut rows: Vec<_> = list
        .iter()
        .map(|b| ShortcutRow {
            binding: b,
            also_numpad: false,
            arrows: false,
        })
        .collect();
    let mut hidden = std::collections::HashSet::new();
    for (i, b) in list.iter().enumerate() {
        if arrows
            && ["left", "right", "up", "down"]
                .iter()
                .any(|d| default_focus(b, d))
        {
            if default_focus(b, "down") {
                rows[i].arrows = true;
            } else {
                hidden.insert(i);
            }
        }
        let Some([chord]) = b.chords.as_deref() else {
            continue;
        };
        let Key::Physical(code) = &chord.key else {
            continue;
        };
        let equivalent = match *code {
            "NumpadAdd" => '+',
            "NumpadSubtract" => '-',
            "Numpad0" => '0',
            _ => continue,
        };
        if let Some(j) = list.iter().position(|other| {
            other.binding.action == b.binding.action
                && other.binding.args == b.binding.args
                && other.binding.source == b.binding.source
                && other.chords.as_deref().is_some_and(|chords| matches!(chords,
                    [other] if other.modifiers == chord.modifiers && other.key == Key::Character(equivalent)))
        }) {
            rows[j].also_numpad = true;
            hidden.insert(i);
        }
    }
    rows.into_iter()
        .enumerate()
        .filter_map(|(i, row)| (!hidden.contains(&i)).then_some(row))
        .collect()
}

/// The headings of the Shortcuts page, in order. Every action's `category()` is one of them
/// (a test checks), and an action this client does not know is listed under Server.
pub(super) const SHORTCUT_CATEGORIES: [&str; 6] = [
    "Navigate",
    "Create and close",
    "Panes",
    "Text and view",
    "Interface",
    "Server",
];

fn shortcuts(s: &Workspace) -> El {
    let mut sections = Vec::new();
    let effective = s.effective_bindings();
    for name in SHORTCUT_CATEGORIES {
        let list = shortcut_rows(&effective, name);
        if list.is_empty() {
            continue;
        }
        // A row reads as a menu row does: what it runs on the left, in the name Commands and
        // the menus use, and its keys on the right as the same keycaps.
        let rows = list.iter().enumerate().map(|(index, display)| {
            let binding = display.binding;
            let chords: Chords = if display.arrows {
                vec![vec!["Ctrl".into(), "Alt".into(), "Arrows".into()]]
            } else {
                let chords = binding.chords.as_ref().unwrap();
                chords.iter().map(|chord| chord.keycaps()).collect()
            };
            let source = match binding.binding.source.as_deref() {
                Some("linux") => None,
                Some("settings") => Some("shortcuts.json"),
                Some(source) => Some(source),
                None => Some("Server"),
            };
            let title = if display.arrows {
                "Focus adjacent pane".into()
            } else {
                super::actions::binding_title(&binding.binding)
            };
            let numpad = display.also_numpad.then_some("Also on the numpad");
            let detail: Vec<&str> = source.into_iter().chain(numpad).collect();
            let detail = detail.join(" · ");
            row_frame(!detail.is_empty())
                .key(format!("shortcut-{name}-{index}"))
                .child(words(&title, &detail))
                .child(
                    keycaps(&chords, false)
                        .key(format!("shortcut-keys-{name}-{index}"))
                        .label(spell(&chords)),
                )
        });
        sections.push(section(name, [card(rows)]));
    }
    // The pointer's buttons are not keys: plain words, as in Commands.
    sections.push(section(
        "Pointer",
        [card([setting(
            "Back and forward in history",
            "",
            value("Mouse Back / Forward"),
        )])],
    ));
    let diagnostics = &s.effective_keymap().diagnostics;
    if !diagnostics.is_empty() || s.model().keymap_error.is_some() {
        let problem = |words: String| {
            text(words)
                .font(ui(11.5))
                .color(rgb(warning()))
                .wrap()
                .grow(1.)
                .shrink()
        };
        let mut rows = Vec::new();
        if let Some(error) = &s.model().keymap_error {
            rows.push(row_frame(false).child(problem(error.clone())));
        }
        for (index, found) in diagnostics.iter().enumerate() {
            rows.push(
                row_frame(false).child(
                    problem(format!(
                        "{}: {}",
                        found.source.as_deref().unwrap_or("keymap"),
                        found.message
                    ))
                    .key(format!("shortcut-problem-{index}")),
                ),
            );
        }
        sections.push(section("Problems", [card(rows)]));
    }
    let path = |key: &str, words: String| {
        row_frame(false).child(
            text(words)
                .key(key.to_owned())
                .font(mono(11.))
                .color(rgb(muted()))
                .selectable(true)
                .wrap()
                .grow(1.)
                .shrink(),
        )
    };
    let mut files = Vec::new();
    if let Some(snapshot) = s.keymap_snapshot() {
        files.push(path(
            "server-shortcuts-path",
            format!(
                "Server: {}{}",
                snapshot.reply.config.path,
                if snapshot.reply.config.exists {
                    ""
                } else {
                    " (not created)"
                }
            ),
        ));
    }
    files.push(path(
        "shortcuts-path",
        rex_linux::settings::shortcuts::Shortcuts::path(&s.settings_path)
            .display()
            .to_string(),
    ));
    sections.push(section(
        "Files",
        [
            card(files),
            footnote(
                "Edit the overrides file, then return to Rex to reload. Escape cancels a sequence; unbound keys reach the terminal.",
            ),
        ],
    ));
    page(sections)
}

/// The section list, in gapped groups: the window, what runs in it, and the app itself.
fn nav(s: &Workspace, width: f32) -> El {
    let mut list = col()
        .key("settings-sections")
        .role("tablist")
        .focus_group()
        .gap(2.);
    for section in SettingsSection::ALL {
        let selected = s.settings_section == section;
        let mut item = row()
            .key(format!("settings-section:{}", section.key()))
            .role("tab")
            .label(section.title())
            .w_full()
            .h(32.)
            .px(10.)
            .center_y()
            .radius(8.)
            .continuous_corners()
            .cursor(Cursor::Pointer)
            .focusable(true)
            .focus_selected(selected)
            .focus_ring(2., 0., rgb(accent()))
            .on_press(move |s: &mut Workspace| s.open_section(section))
            .child(
                text(section.title())
                    .font(if selected { medium(13.) } else { ui(13.) })
                    .color(rgb(if selected { ink() } else { muted() })),
            );
        if matches!(
            section,
            SettingsSection::Terminal | SettingsSection::Updates
        ) {
            item = item.mt(12.);
        }
        item = if selected {
            item.fill(white(0.09))
        } else {
            item.hover(white(0.05))
        };
        list = list.child(item);
    }
    col()
        .w(width)
        .h_full()
        .padding(14.)
        .gap(12.)
        .child(
            text("Settings")
                .font(strong(15.))
                .color(rgb(ink()))
                .px(10.)
                .pt(6.),
        )
        .child(list)
}

/// The dialog's surface: the section list, and the page under its header. The page scrolls
/// with a fade at whichever edge hides content and a thumb while there is more than fits.
fn surface(s: &Workspace, viewport: [f32; 2]) -> El {
    let k = kit();
    let section = s.settings_section;
    let body = match section {
        SettingsSection::Interface => interface(s),
        SettingsSection::Terminal => terminal(s),
        SettingsSection::Notifications => notifications(s),
        SettingsSection::Integrations => super::integrations::section(s),
        SettingsSection::Hosts => hosts(s),
        SettingsSection::Shortcuts => shortcuts(s),
        SettingsSection::Updates => super::updates::section(s),
        SettingsSection::About => super::about::section(s),
    };
    let width = PANEL_WIDTH.min(viewport[0] * 0.92);
    // Under the toolbar, which stays whole above it, with a margin at both ends.
    let height = PANEL_HEIGHT.min((viewport[1] - TOOLBAR - 2. * CLEARANCE).max(0.));
    let nav_width = NAV_WIDTH.min(width * 0.3);
    let (page_width, page_height) = (width - nav_width - 1., height - HEADER);
    let thumb = s.settings_fx.thumb.clone();
    let scroller = col()
        .key(format!("settings-scroll:{}", section.key()))
        .grow(1.)
        .min_h(0.)
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .scroll_edge_fade(16.)
        .on_scroll_offset(move |at: ScrollPos| {
            let hidden = at.max[1];
            if hidden < 1. {
                thumb.set([0.; 4]);
                return;
            }
            let length = (page_height * page_height / (page_height + hidden)).max(24.);
            let top = at.offset[1] / hidden * (page_height - length - 8.);
            thumb.set([page_width - 7., top + 4., 4., length]);
        })
        .child(body.px(PAGE_INSET).pt(2.).pb(24.));
    let content = col()
        .grow(1.)
        .min_w(0.)
        .h_full()
        .child(
            row()
                .h(HEADER)
                .pl(PAGE_INSET + super::settings_kit::INSET)
                .pr(PAGE_INSET)
                .center_y()
                .justify_between()
                .child(text(section.title()).font(strong(17.)).color(rgb(ink())))
                .child(
                    IconButton::new(&CLOSE)
                        .label("Close")
                        .on_press_tx(|s: &mut Workspace, tx: &mut Tx| s.close_panel(tx))
                        .build(k)
                        .key("close-settings"),
                ),
        )
        .child(
            col()
                .grow(1.)
                .min_h(0.)
                .w_full()
                .child(scroller)
                .child(s.settings_fx.thumb_el()),
        );
    row()
        .w(width)
        .h(height)
        .child(nav(s, nav_width))
        .child(el().w(1.).h_full().fill(white(0.06)))
        .child(content)
}

pub fn settings(s: &Workspace, viewport: [f32; 2]) -> El {
    modal_layer(s, surface(s, viewport), false, TOOLBAR)
}

/// Settings under a list opened from one of its controls: drawn where it was, taking no input.
pub fn settings_under(s: &Workspace, viewport: [f32; 2]) -> El {
    modal_layer(s, surface(s, viewport), true, TOOLBAR)
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;
    use crate::app::actions;
    use rex_linux::keymap::Overrides;
    use serde_json::json;

    #[test]
    fn defaults_are_compact_and_in_registry_order() {
        let map = actions::resolve(&Overrides::default());
        let effective = actions::effective(&map, &[]);
        let panes = shortcut_rows(&effective, "Panes");
        let focus: Vec<_> = panes
            .iter()
            .filter(|r| r.binding.binding.action == "pane.focus")
            .collect();
        assert_eq!(focus.len(), 1);
        assert!(focus[0].arrows);
        let interface = shortcut_rows(&effective, "Interface");
        assert_eq!(interface.iter().filter(|r| r.also_numpad).count(), 3);
        assert!(
            !interface
                .iter()
                .any(|r| r.binding.binding.keys[0].contains("Numpad"))
        );
        for category in [
            "Navigate",
            "Create and close",
            "Panes",
            "Text and view",
            "Interface",
        ] {
            let rows = shortcut_rows(&effective, category);
            let positions: Vec<_> = rows
                .iter()
                .map(|r| {
                    actions::SPECS
                        .iter()
                        .position(|s| s.name == r.binding.binding.action)
                        .unwrap()
                })
                .collect();
            assert!(positions.windows(2).all(|w| w[0] <= w[1]));
        }
        let scroll: Vec<_> = effective
            .iter()
            .filter(|b| b.binding.action == "pane.scroll")
            .map(|b| actions::binding_title(&b.binding))
            .collect();
        assert!(scroll.contains(&"Scroll up · 20 lines".into()));
        assert!(scroll.contains(&"Scroll down · 20 lines".into()));
    }

    #[test]
    fn custom_focus_is_not_folded_and_try_order_is_retained() {
        let map = actions::resolve(&Overrides::parse(&json!([
            {"keys":"ctrl+alt+left","action":"pane.focus","args":{"direction":"right"}},
            {"keys":"ctrl+j","action":"client.tab.new"},
            {"keys":"ctrl+y","action":"client.tab.new"},
            {"keys":"ctrl+[Numpad0]","action":"client.zoom.in"}
        ])));
        let effective = actions::effective(&map, &[]);
        assert!(shortcut_rows(&effective, "Panes").iter().all(|r| !r.arrows));
        let expected: Vec<_> = effective
            .iter()
            .filter(|b| b.binding.action == "client.tab.new")
            .map(|b| &b.binding.keys)
            .collect();
        let rows = shortcut_rows(&effective, "Create and close");
        let actual: Vec<_> = rows
            .iter()
            .filter(|r| r.binding.binding.action == "client.tab.new")
            .map(|r| &r.binding.binding.keys)
            .collect();
        assert_eq!(actual, expected);
        assert!(
            shortcut_rows(&effective, "Interface")
                .iter()
                .any(|r| r.binding.binding.keys == ["ctrl+[Numpad0]"] && !r.also_numpad)
        );
    }
}
