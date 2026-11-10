//! Settings: a section list on the left and the chosen section's controls on the right.
use super::{
    dialogs::modal_frame,
    panel::{Intent, SettingsSection},
    theme::*,
    widgets::{Pill, glyph, pill},
    workspace::Workspace,
};
use rex_linux::settings::{
    Channels, FocusCentering, NoticePosition, NotificationsConfig, SessionList, TabLayout,
};
use rune::{Cursor, El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Checkbox, IconButton, Keybind, SegmentedControl, Switch};

/// A labelled setting: name and optional explanation on the left, its control on the right.
pub(super) fn setting(label: &str, detail: &str, control: El) -> El {
    let detail = (!detail.is_empty()).then(|| detail_text(detail, muted()));
    setting_with(label, detail, control)
}

/// A setting's explanation, in `muted()` or, for a problem, `warning()`.
pub(super) fn detail_text(detail: &str, color: u32) -> El {
    text(detail.to_owned())
        .font(ui(11.5))
        .color(rgb(color))
        .wrap()
}

/// [`setting`] with an explanation built by the caller.
pub(super) fn setting_with(label: &str, detail: Option<El>, control: El) -> El {
    let mut words = col()
        .grow(1.)
        .min_w(0.)
        .gap(2.)
        .child(text(label.to_owned()).font(ui(13.)).color(rgb(ink())));
    if let Some(detail) = detail {
        words = words.child(detail);
    }
    row()
        .w_full()
        .min_h(44.)
        .gap(16.)
        .center_y()
        .child(words)
        .child(control)
}

pub(super) fn group(name: &str) -> El {
    text(name.to_owned())
        .font(label_caps())
        .text_case(rune::TextCase::Uppercase)
        .color(rgb(muted()))
        .mt(10.)
}

pub(super) fn divider() -> El {
    el().w_full().h(1.).fill(white(0.06))
}

/// A stepper: − value +, with Return acting on the focused button as Space does.
fn stepper(
    key: &str,
    value: String,
    down: impl Fn(&mut Workspace, &mut Tx) + Clone + 'static,
    up: impl Fn(&mut Workspace, &mut Tx) + Clone + 'static,
) -> El {
    let k = kit();
    let (down2, up2) = (down.clone(), up.clone());
    row()
        .gap(8.)
        .center_y()
        .child(
            IconButton::new(&MINUS)
                .label("Smaller")
                .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| down(s, tx))
                .build(k)
                .key(format!("{key}-smaller"))
                .on_key(move |s: &mut Workspace, ev: &rune::KeyEvent, tx: &mut Tx| {
                    (ev.key == rune::Key::Enter).then(|| down2(s, tx)).is_some()
                }),
        )
        .child(
            row()
                .w(52.)
                .center()
                .child(text(value).font(medium(13.)).color(rgb(ink()))),
        )
        .child(
            IconButton::new(&PLUS)
                .label("Larger")
                .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| up(s, tx))
                .build(k)
                .key(format!("{key}-larger"))
                .on_key(move |s: &mut Workspace, ev: &rune::KeyEvent, tx: &mut Tx| {
                    (ev.key == rune::Key::Enter).then(|| up2(s, tx)).is_some()
                }),
        )
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
    col()
        .gap(4.)
        .child(setting(
            "Theme",
            "The window's colours. Until you pick a terminal theme, terminals take the matching one (Follow chrome).",
            row()
                .gap(10.)
                .center_y()
                .child(
                    text(s.settings.chrome_theme.name())
                        .font(medium(13.))
                        .color(rgb(ink()))
                        .truncate()
                        .max_w(160.),
                )
                .child(pill("chrome-theme", "Change…", Pill::Neutral, 26., |s, tx| {
                    s.perform(Intent::ChooseChrome, tx)
                })),
        ))
        .child(divider())
        .child(setting(
            "Interface zoom",
            "Chrome and terminals together, 70–150%. Ctrl + and Ctrl − also work.",
            row()
                .gap(8.)
                .center_y()
                .child(stepper(
                    "zoom",
                    format!("{percent}%"),
                    |s, tx| s.change_ui_scale(-1, tx),
                    |s, tx| s.change_ui_scale(1, tx),
                ))
                .child(pill("zoom-reset", "Reset", Pill::Neutral, 26., |s, tx| {
                    s.change_ui_scale(0, tx)
                })),
        ))
        .child(divider())
        .child(setting(
            "Session sidebar",
            "Hosts and their sessions beside the terminals. Ctrl Shift B.",
            Switch::new(s.sidebar.is_open())
                .label("Show session sidebar")
                .hide_label()
                .on_toggle_tx(|s: &mut Workspace, tx: &mut Tx| s.perform(Intent::Sidebar, tx))
                .build(k)
                .key("sidebar-switch"),
        ))
        .child(setting(
            "Tab position",
            "In the toolbar, or under each session in the sidebar while it's open.",
            SegmentedControl::new(
                &["Top", "Sidebar"],
                Some(usize::from(s.settings.vertical_tabs)),
            )
            .fit()
            .on_pick(|s: &mut Workspace, i, _| s.set_vertical_tabs(i == 1))
            .build(k)
            .key("tab-position"),
        ))
        .child(setting(
            "Session list",
            "Under a header per host, or one list naming each session's host while two or more are connected.",
            SegmentedControl::new(
                &["By host", "Flat"],
                Some(usize::from(
                    s.settings.sidebar_sessions == SessionList::Flat,
                )),
            )
            .fit()
            .on_pick(|s: &mut Workspace, i, _| {
                s.set_session_list(if i == 1 {
                    SessionList::Flat
                } else {
                    SessionList::ByHost
                })
            })
            .build(k)
            .key("session-list"),
        ))
        .child(setting(
            "Tab layout",
            "Panes tiled to the window, or half-width columns that scroll sideways. A tab's menu can change it.",
            SegmentedControl::new(&["Tiled", "Scrolling"], Some(layout))
                .fit()
                .on_pick(|s: &mut Workspace, i, _| {
                    s.set_tab_layout(if i == 0 {
                        TabLayout::Tiled
                    } else {
                        TabLayout::Scrolling
                    })
                })
                .build(k)
                .key("tab-layout"),
        ))
        .child(divider())
        .child(setting(
            "Focus centering",
            "When moving focus in a scrolling tab. On overflow centers when both columns cannot fit together.",
            SegmentedControl::new(&["Never", "On overflow", "Always"], Some(centering))
                .fit()
                .on_pick(|s: &mut Workspace, i, _| {
                    s.set_focus_centering(match i {
                        1 => FocusCentering::OnOverflow,
                        2 => FocusCentering::Always,
                        _ => FocusCentering::Never,
                    })
                })
                .build(k)
                .key("focus-centering"),
        ))
        .child(divider())
        .child(setting(
            "Notices",
            "Where in-app notices appear.",
            SegmentedControl::new(&["Top right", "Bottom right"], Some(corner))
                .fit()
                .on_pick(|s: &mut Workspace, i, _| {
                    s.set_notice_position(if i == 0 {
                        NoticePosition::TopRight
                    } else {
                        NoticePosition::BottomRight
                    })
                })
                .build(k)
                .key("notice-position"),
        ))
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
    let mut header = row().w_full().mt(8.).center_y().child(el().grow(1.));
    for (channel, caption) in CHANNELS {
        header = header.child(
            row().w(CHANNEL_COLUMN).center().child(
                text(caption)
                    .key(format!("notify-column-{channel}"))
                    .font(label_caps())
                    .text_case(rune::TextCase::Uppercase)
                    .color(rgb(muted())),
            ),
        );
    }
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
    let mut body = col()
        .gap(4.)
        .child(
            text("When a program in a pane you aren't looking at changes state. Desktop notifications go through your desktop's notification service; clicking one shows its pane.")
                .font(ui(11.5))
                .color(rgb(muted()))
                .wrap(),
        )
        .child(header);
    for (i, (key, label, detail, c, [tab, desktop, badge, sound])) in states.into_iter().enumerate()
    {
        if i > 0 {
            body = body.child(divider());
        }
        let mut cells: Cells = vec![
            ("tab", c.tab, false, tab),
            ("desktop", c.desktop, false, desktop),
        ];
        if key != "working" {
            cells.push(("badge", c.badge, false, badge));
        }
        // Sound plays with a desktop notification, so it needs one.
        cells.push(("sound", c.sound, !c.desktop, sound));
        body = body.child(channel_row(key, label, detail, cells));
    }
    let m = n.messages;
    body.child(divider()).child(channel_row(
        "messages",
        "Messages from programs",
        "Notifications programs send themselves with OSC 9 or OSC 777.",
        vec![
            ("desktop", m.desktop, false, |n| &mut n.messages.desktop),
            ("sound", m.sound, !m.desktop, |n| &mut n.messages.sound),
        ],
    ))
}

fn terminal(s: &Workspace) -> El {
    let saved = &s.settings.font_family;
    let active = s.terminal_family.unwrap_or("IBM Plex Mono (bundled)");
    let font = if *saved != s.launch_saved_font {
        format!("{saved} is saved and applies when you reopen the client. Active: {active}.")
    } else if s.terminal_family.is_none() {
        format!(
            "{} isn't installed, so terminals use {active}.",
            s.launch_font
        )
    } else {
        format!("Active: {active}. Changes apply when you reopen the client.")
    };
    let mut body = col()
        .gap(4.)
        .child(setting(
            "Font family",
            &font,
            pill("font-change", "Change…", Pill::Neutral, 26., |s, tx| s.perform(Intent::Fonts, tx)),
        ))
        .child(divider())
        .child(setting(
            "Font size",
            "8–48 pt, for every terminal.",
            stepper(
                "font",
                format!("{} pt", s.settings.font_size),
                |s, _| s.change_font(-1.),
                |s, _| s.change_font(1.),
            ),
        ))
        .child(divider())
        .child(setting(
            "Fallback fonts",
            "Used for missing glyphs, followed by installed system fallbacks. Changes apply when you reopen the client.",
            text(if s.settings.font_fallbacks.is_empty() { "Automatic".into() } else { s.settings.font_fallbacks.join(", ") })
                .font(medium(13.)).color(rgb(ink())).wrap().max_w(220.),
        ))
        .child(divider())
        .child(setting(
            "Theme",
            "Declared to Rex for panes this client owns or nobody owns. Follow server declares nothing until this client has declared a theme once; from then on it resets those panes to Rex's colours, once each per connection.",
            row()
                .gap(10.)
                .center_y()
                .child(
                    text(Workspace::theme_label(&s.settings.terminal_theme).to_owned())
                        .font(medium(13.))
                        .color(rgb(ink()))
                        .truncate()
                        .max_w(160.),
                )
                .child(pill("terminal-theme", "Change…", Pill::Neutral, 26., |s, tx| {
                    s.perform(Intent::ChooseTheme, tx)
                })),
        ))
        .child(group("Default shell"));
    for (i, host) in s.settings.hosts.iter().enumerate() {
        let detail = match &host.shell {
            Some(shell) => format!("New sessions, tabs and splits start {shell}"),
            None => "New sessions, tabs and splits use the server's default shell".into(),
        };
        body = body.child(setting(
            &host.label,
            &detail,
            pill(
                &format!("host-shell:{i}"),
                "Change…",
                Pill::Neutral,
                26.,
                move |s, tx| s.perform(Intent::HostShell(i), tx),
            ),
        ));
    }
    body.child(
        text(format!("Saved in {}", s.settings_path.display()))
            .font(ui(11.))
            .color(rgb(muted()))
            .wrap()
            .selectable(true)
            .mt(12.),
    )
}

fn hosts(s: &Workspace) -> El {
    let last = s.settings.hosts.len() == 1;
    let mut body = col()
        .gap(4.)
        .child(setting(
            "Client label",
            &format!(
                "{}\nVisible on every host; reopen the app after editing.",
                s.settings.client_label()
            ),
            pill("client-label", "Edit…", Pill::Neutral, 26., |s, tx| {
                s.perform(Intent::ClientLabel, tx)
            }),
        ))
        .child(divider());
    for (i, host) in s.settings.hosts.iter().enumerate() {
        let connected = s.hosts.get(i).is_some_and(|h| h.model.connected);
        let mut actions = row().gap(6.).center_y();
        // Quiet when healthy: only trouble gets words, beside the action that fixes it.
        if !connected {
            actions = actions
                .child(text("Offline").font(medium(12.)).color(rgb(warning())))
                .child(pill(
                    &format!("host-retry:{i}"),
                    "Retry",
                    Pill::Warning,
                    26.,
                    move |s, tx| s.perform(Intent::RetryHost(i), tx),
                ));
        }
        actions = actions.child(pill(
            &format!("host-edit:{i}"),
            "Edit…",
            Pill::Neutral,
            26.,
            move |s, tx| s.perform(Intent::EditHost(i), tx),
        ));
        let remove = if last {
            pill(
                &format!("host-remove:{i}"),
                "Remove…",
                Pill::Neutral,
                26.,
                |_, _| {},
            )
            .opacity(0.4)
            .focusable(false)
            .help("The workspace needs at least one host")
        } else {
            pill(
                &format!("host-remove:{i}"),
                "Remove…",
                Pill::Danger,
                26.,
                move |s, tx| s.perform(Intent::RemoveHost(i), tx),
            )
        };
        let shell = host
            .shell
            .as_deref()
            .map(|sh| format!(" · {sh}"))
            .unwrap_or_default();
        body = body
            .child(
                row()
                    .key(format!("host-row:{i}"))
                    .w_full()
                    .min_h(52.)
                    .gap(12.)
                    .center_y()
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
                            .child(
                                text(format!("{}{shell}", host.endpoint))
                                    .font(ui(11.5))
                                    .color(rgb(muted()))
                                    .truncate()
                                    .selectable(true),
                            ),
                    )
                    .child(actions.child(remove)),
            )
            .child(setting(
                "Remote control",
                "Let rex do and other clients on this host change the view in this window.",
                rune_kit::Switch::new(host.remote_control)
                    .label(format!("Remote control for {}", host.label))
                    .hide_label()
                    .on_toggle(move |s: &mut Workspace| s.toggle_remote_control(i))
                    .build(kit())
                    .key(format!("remote-control:{i}")),
            ))
            .child(super::tailscale::rows(s, i))
            .child(divider());
    }
    body.child(row().mt(8.).child(pill(
        "settings-add-host",
        "Add host…",
        Pill::Neutral,
        30.,
        |s, tx| s.perform(Intent::AddHost, tx),
    )))
    .child(
        text("Removing a host only stops this app connecting to it; its sessions keep running.")
            .font(ui(11.5))
            .color(rgb(muted()))
            .wrap()
            .mt(8.),
    )
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

fn shortcuts(s: &Workspace) -> El {
    let k = kit();
    let mut body = col().gap(4.);
    let effective = s.effective_bindings();
    for name in [
        "Navigate",
        "Create and close",
        "Panes",
        "Text and view",
        "Interface",
        "Server",
    ] {
        let list = shortcut_rows(&effective, name);
        if list.is_empty() {
            continue;
        }
        body = body.child(group(name));
        for (index, display) in list.iter().enumerate() {
            let binding = display.binding;
            let mut caps = col().gap(3.);
            for (depth, chord) in binding.chords.as_ref().unwrap().iter().enumerate() {
                if depth > 0 {
                    caps = caps.child(text("then").font(ui(10.5)).color(rgb(muted())));
                }
                let names = if display.arrows {
                    vec!["Ctrl".into(), "Alt".into(), "Arrows".into()]
                } else {
                    chord.keycaps()
                };
                let keys: Vec<_> = names.iter().map(String::as_str).collect();
                caps = caps.child(Keybind::new(&keys).build(k));
            }
            if display.also_numpad {
                caps = caps.child(text("also numpad").font(ui(10.5)).color(rgb(muted())));
            }
            let source = match binding.binding.source.as_deref() {
                Some("linux") => None,
                Some("settings") => Some("shortcuts.json"),
                Some(source) => Some(source),
                None => Some("Server"),
            };
            let mut words = col().grow(1.).min_w(0.).gap(2.).child(
                text(if display.arrows {
                    "Focus adjacent pane".into()
                } else {
                    super::actions::binding_title(&binding.binding)
                })
                .font(ui(12.5))
                .color(rgb(ink()))
                .wrap(),
            );
            if let Some(source) = source {
                words = words.child(text(source).font(ui(10.5)).color(rgb(muted())).wrap());
            }
            body = body.child(
                row()
                    .key(format!("shortcut-{name}-{index}"))
                    .min_h(38.)
                    .py(3.)
                    .gap(12.)
                    .center_y()
                    .child(el().w(190.).child(caps))
                    .child(words),
            );
        }
    }
    body = body.child(group("Pointer")).child(
        row()
            .min_h(30.)
            .gap(12.)
            .center_y()
            .child(
                el().w(190.)
                    .child(Keybind::new(&["Mouse", "Back / Forward"]).build(k)),
            )
            .child(text("Navigation history").font(ui(12.5)).color(rgb(ink()))),
    );
    let diagnostics = &s.effective_keymap().diagnostics;
    if !diagnostics.is_empty() || s.model().keymap_error.is_some() {
        body = body.child(group("Problems"));
        if let Some(error) = &s.model().keymap_error {
            body = body.child(text(error).font(ui(11.5)).color(rgb(warning())).wrap());
        }
        for (index, problem) in diagnostics.iter().enumerate() {
            body = body.child(
                text(format!(
                    "{}: {}",
                    problem.source.as_deref().unwrap_or("keymap"),
                    problem.message
                ))
                .key(format!("shortcut-problem-{index}"))
                .font(ui(11.5))
                .color(rgb(warning()))
                .wrap(),
            );
        }
    }
    body = body.child(group("Files"));
    if let Some(snapshot) = s.keymap_snapshot() {
        body = body.child(
            text(format!(
                "Server: {}{}",
                snapshot.reply.config.path,
                if snapshot.reply.config.exists {
                    ""
                } else {
                    " (not created)"
                }
            ))
            .key("server-shortcuts-path")
            .font(ui(11.5))
            .color(rgb(muted()))
            .selectable(true)
            .wrap(),
        );
    }
    body = body.child(
        text(
            rex_linux::settings::shortcuts::Shortcuts::path(&s.settings_path)
                .display()
                .to_string(),
        )
        .key("shortcuts-path")
        .font(ui(11.5))
        .color(rgb(muted()))
        .selectable(true)
        .wrap()
        .mt(12.),
    );
    body.child(
        text("Edit the overrides file, then return to Rex to reload. Escape cancels a sequence; unbound keys reach the terminal.")
            .font(ui(11.5))
            .color(rgb(muted()))
            .wrap()
            .mt(12.),
    )
}

fn nav(s: &Workspace) -> El {
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
        item = if selected {
            item.fill(white(0.09))
        } else {
            item.hover(white(0.05))
        };
        list = list.child(item);
    }
    col()
        .w(190.)
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

pub fn settings(s: &Workspace, viewport: [f32; 2]) -> El {
    let k = kit();
    let section = s.settings_section;
    let body = match section {
        SettingsSection::Interface => interface(s),
        SettingsSection::Terminal => terminal(s),
        SettingsSection::Notifications => notifications(s),
        SettingsSection::Integrations => super::integrations::section(s),
        SettingsSection::Hosts => hosts(s),
        SettingsSection::Shortcuts => shortcuts(s),
        SettingsSection::About => super::about::section(s),
    };
    let content = col()
        .grow(1.)
        .min_w(0.)
        .h_full()
        .child(
            row()
                .h(56.)
                .px(24.)
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
                .key(format!("settings-scroll:{}", section.key()))
                .grow(1.)
                .min_h(0.)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(body.px(24.).pb(20.)),
        );
    modal_frame(
        s,
        row()
            .w(760f32.min(viewport[0] * 0.92))
            .h(520f32.min(viewport[1] * 0.88))
            .child(nav(s))
            .child(el().w(1.).h_full().fill(white(0.06)))
            .child(content),
    )
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
