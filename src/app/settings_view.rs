//! Settings: a section list on the left and the chosen section's page on the right. Pages are
//! built from `settings_kit` (sections, cards, rows and their controls).
use super::settings_nav::{HEADER, PAGE_INSET};
use super::{
    dialogs::modal_layer,
    panel::{Intent, SettingsSection},
    settings_kit::{
        NAV_WIDTH, PANEL_HEIGHT, PANEL_WIDTH, Segment, button, card, disclosure, footnote, page,
        row_frame, section, segmented, select_row, setting, stepper, unavailable, value_popup,
    },
    theme::*,
    widgets::{Pill, glyph},
    workspace::Workspace,
};
use rex_linux::settings::{
    Channels, FocusCentering, NoticePosition, NotificationsConfig, PaneDensity, PaneHeader,
    SessionList, TabLayout,
    interface::{InterfaceConfig, ToolbarBehavior},
};
use rune::{El, KeyEvent, PointerButton, ScrollPos, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Checkbox, Switch};

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

/// The least space between the dialog and the toolbar above it, and the window's bottom edge.
const CLEARANCE: f32 = 12.;

/// The Pane headers choices, in the order the control lists them.
const PANE_HEADERS: [PaneHeader; 4] = [
    PaneHeader::Auto,
    PaneHeader::Always,
    PaneHeader::OnHover,
    PaneHeader::Hidden,
];

const TOOLBARS: [ToolbarBehavior; 3] = [
    ToolbarBehavior::Always,
    ToolbarBehavior::OnHover,
    ToolbarBehavior::Hidden,
];

fn header_fade_switch(
    key: &str,
    label: &str,
    on: bool,
    field: fn(&mut InterfaceConfig) -> &mut bool,
) -> El {
    Switch::new(on)
        .label(label.to_owned())
        .hide_label()
        .on_toggle(move |s: &mut Workspace| {
            let value = field(&mut s.settings.interface);
            *value = !*value;
            s.save();
        })
        .build(kit())
        .key(key.to_owned())
}

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
            "Toolbar",
            [
                card([select_row(
                    "toolbar-behavior",
                    "Toolbar visibility",
                    "Show on hover and Hidden give its row to the terminals.",
                    &["Always show", "Show on hover", "Hidden"],
                    TOOLBARS
                        .iter()
                        .position(|t| *t == s.settings.interface.toolbar)
                        .unwrap_or_default(),
                    |s, i, tx| s.set_toolbar(TOOLBARS[i.min(TOOLBARS.len() - 1)], tx),
                )]),
                {
                    let keys =
                        s.shortcut_detail(super::toolbar_reveal::TOGGLE, &serde_json::Value::Null);
                    let reveal = if keys.is_empty() {
                        "Reveal toolbar, in Commands,".to_owned()
                    } else {
                        format!("Reveal toolbar ({keys}, and in Commands)")
                    };
                    footnote(format!("{reveal} floats the toolbar until run again. Window-manager Move and Close still work."))
                        .key("toolbar-reveal-note")
                },
            ],
        ),
        section(
            "Panes",
            [
                card([
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
                    select_row(
                        "active-pane-outline",
                        "Active pane outline",
                        "Auto follows density. A single edge-to-edge pane has no outline.",
                        &["Auto", "On", "Off"],
                        match s.settings.interface.highlight_active_pane {
                            None => 0,
                            Some(true) => 1,
                            Some(false) => 2,
                        },
                        |s, i, tx| {
                            s.settings.interface.highlight_active_pane = match i {
                                1 => Some(true),
                                2 => Some(false),
                                _ => None,
                            };
                            s.save();
                            tx.instant();
                        },
                    ),
                    setting(
                        "Fade inactive pane headers",
                        "For panes without the keyboard.",
                        header_fade_switch(
                            "fade-inactive-headers",
                            "Fade inactive pane headers",
                            s.settings.interface.fade_inactive_pane_headers,
                            |i| &mut i.fade_inactive_pane_headers,
                        ),
                    ),
                    setting(
                        "Fade headers in inactive windows",
                        "While this window is not focused.",
                        header_fade_switch(
                            "fade-headers-inactive-window",
                            "Fade headers in inactive windows",
                            s.settings.interface.fade_pane_headers_in_inactive_window,
                            |i| &mut i.fade_pane_headers_in_inactive_window,
                        ),
                    ),
                ]),
                footnote(
                    "Only visible header names and available buttons fade. Activity, progress, exceptions and terminal content stay unchanged.",
                ),
            ],
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
                setting(
                    "Tabs fill available space",
                    "Only toolbar tabs share the available width.",
                    Switch::new(s.settings.interface.tabs_fill_available_space)
                        .label("Tabs fill available space")
                        .hide_label()
                        .on_toggle(|s: &mut Workspace| {
                            s.settings.interface.tabs_fill_available_space =
                                !s.settings.interface.tabs_fill_available_space;
                            s.reveal_tab_again = true;
                            s.save();
                        })
                        .build(k)
                        .key("tabs-fill"),
                ),
            ])],
        ),
        section(
            "Downloads",
            [card([disclosure(
                "download-folder",
                "Download folder",
                "Where Download file… saves.",
                &super::downloads::folder_text(
                    &s.download_folder(),
                    super::downloads::FOLDER_CHARS,
                ),
                |s, tx| s.perform(Intent::DownloadFolder, tx),
            )])],
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
    super::terminal_page::terminal(s)
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

/// Build only the page content. Search reads these same labelled Settings-kit rows.
pub(super) fn body(s: &Workspace, page: super::settings_nav::Page) -> El {
    let section = match page {
        super::settings_nav::Page::Section(section) => section,
        super::settings_nav::Page::Sub(super::settings_nav::SubPage::AdvancedFont) => {
            return super::font_page::advanced(s);
        }
    };
    match section {
        SettingsSection::Interface => interface(s),
        SettingsSection::Terminal => terminal(s),
        SettingsSection::Notifications => notifications(s),
        SettingsSection::Integrations => super::integrations::section(s),
        SettingsSection::Hosts => hosts(s),
        SettingsSection::Shortcuts => super::shortcuts_page::page(s),
        SettingsSection::Updates => super::updates::section(s),
        SettingsSection::About => super::about::section(s),
    }
}

/// The dialog's surface: the section list, and the page under its header. The page scrolls
/// with a fade at whichever edge hides content and a thumb while there is more than fits.
fn surface(s: &Workspace, viewport: [f32; 2]) -> El {
    let section = s.settings_page();
    let body = super::settings_nav::draw(s, section);
    let width = PANEL_WIDTH.min(viewport[0] * 0.92);
    // Under the toolbar, which stays whole above it, with a margin at both ends.
    let height = PANEL_HEIGHT
        .min((viewport[1] - super::toolbar_reveal::row_height(s) - 2. * CLEARANCE).max(0.));
    let nav_width = NAV_WIDTH.min(width * 0.3);
    let pinned = (section == SettingsSection::Shortcuts)
        .then(|| super::shortcuts_page::filter_field(s, PAGE_INSET));
    let above = HEADER
        + pinned
            .as_ref()
            .map_or(0., |_| super::shortcuts_page::FILTER);
    let (page_width, page_height) = (width - nav_width - 1., height - above);
    let thumb = s.settings_fx.thumb.clone();
    let offset = s.settings_nav.offset();
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
            offset.set(at.offset[1]);
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
        .child(super::settings_shell::header(s, section))
        .children(pinned)
        .child(
            col()
                .grow(1.)
                .min_h(0.)
                .w_full()
                .child(scroller)
                .child(s.settings_fx.thumb_el()),
        );
    let frame = row()
        .key("settings-frame")
        .w_full()
        .h_full()
        .pointer_buttons([PointerButton::Back, PointerButton::Forward])
        .preserve_focus_on_press(true)
        .on_press_tx(|s: &mut Workspace, tx: &mut Tx| {
            let direction = match tx.button() {
                PointerButton::Back => -1,
                PointerButton::Forward => 1,
                _ => return,
            };
            s.settings_history(direction, tx);
        })
        .child(super::settings_shell::sidebar(s, nav_width))
        .child(el().w(1.).h_full().fill(white(0.06)))
        .child(content);
    row()
        .w(width)
        .h(height)
        .on_key(|s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx| s.settings_key(ev, tx))
        .child(frame)
}

pub fn settings(s: &Workspace, viewport: [f32; 2]) -> El {
    modal_layer(
        s,
        surface(s, viewport),
        false,
        super::toolbar_reveal::row_height(s),
    )
}

/// Settings under a list opened from one of its controls: drawn where it was, taking no input.
pub fn settings_under(s: &Workspace, viewport: [f32; 2]) -> El {
    modal_layer(
        s,
        surface(s, viewport),
        true,
        super::toolbar_reveal::row_height(s),
    )
}
