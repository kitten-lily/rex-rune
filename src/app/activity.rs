//! Program activity per tab: one state folded from every record of every block in a Rex
//! window, the program its pill shows and the word trouble carries; and per pane, which
//! changes become desktop notifications and which panes finished out of sight.
use super::{
    activity_list::PaneRef,
    notify::{Note, NoteTarget, Notifier, Sound},
    theme::*,
    workspace::{HostId, Workspace},
};
use rex_linux::connection::{
    DEFAULT_TAB_LABEL, HostModel,
    activity::{
        BlockedKind, ProgramState, StatusRecord, TERMINAL as TERMINAL_CREATOR, TerminalProgress,
    },
};
use rex_linux::settings::NotificationsConfig;
use rexrs::api::session;
use rune::{El, Tx, el, row};
use std::{borrow::Cow, collections::HashMap};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabActivity {
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    /// The deciding record's message, for the tooltip; plain text from the program.
    pub msg: Option<String>,
    /// The deciding record arrived live rather than from a query.
    pub live: bool,
    /// The program behind the deciding record.
    pub source: Option<String>,
    /// The focused block's program: the one reporting, else the foreground process.
    pub program: Option<String>,
    /// The tab's OSC 9;4 progress: an error, else a pause, else progress, across its panes.
    pub progress: Option<TerminalProgress>,
}

fn blocks(window: &session::Window) -> impl Iterator<Item = &str> + Clone {
    window
        .layers
        .iter()
        .flat_map(|l| &l.blocks)
        .filter_map(|b| b.block_id.as_deref())
}

fn focused_block(window: &session::Window) -> Option<&str> {
    window
        .focused_block_id
        .as_deref()
        .filter(|f| blocks(window).any(|b| b == *f))
        .or_else(|| blocks(window).next())
}

/// What a tab is called: its chosen name, or for a default label, its focused program's
/// terminal title when there is one.
pub fn tab_name(model: &HostModel, window: &session::Window) -> String {
    let label = window.label.as_deref().filter(|l| !l.is_empty());
    // The server's default uses the last eight ID characters. Match this window's
    // own ID so an explicitly named "Window ..." on another window stays intact.
    let generated = label
        .and_then(|l| l.strip_prefix("Window "))
        .is_some_and(|suffix| {
            suffix.len() == 8
                && window
                    .window_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with(suffix))
        });
    let title = (generated || label.is_none_or(|l| l == DEFAULT_TAB_LABEL))
        .then(|| focused_block(window).and_then(|b| model.title(b)))
        .flatten();
    title.or(label).unwrap_or("Terminal").to_owned()
}

/// A terminal pane's label is still the server's default: missing, or its flavor's last word.
pub fn label_is_default(block: &session::LayerBlock) -> bool {
    let label = block.label.as_deref().filter(|l| !l.is_empty());
    block.creator_name.as_deref() == Some(TERMINAL_CREATOR)
        && label
            .is_none_or(|l| block.flavor.as_deref().and_then(|f| f.rsplit('.').next()) == Some(l))
}

/// What a pane is called: its server label, or while that is still the default its terminal
/// gets (its flavor's last word, such as `shell`), its program's terminal title. Display only:
/// the server label is never changed.
pub fn pane_name(model: &HostModel, block: &session::LayerBlock) -> String {
    let label = block.label.as_deref().filter(|l| !l.is_empty());
    let title = label_is_default(block)
        .then(|| block.block_id.as_deref().and_then(|b| model.title(b)))
        .flatten();
    title.or(label).unwrap_or("Terminal").to_owned()
}

/// Fold a tab's records with Blocked > Error > Working > Done > Idle.
pub fn tab_activity(model: &HostModel, window: &session::Window) -> TabActivity {
    fold_activity(model, blocks(window), focused_block(window))
}

/// A session's activity: the fold over every pane of every tab in its view.
pub fn view_activity(model: &HostModel, view: &session::ViewResponse) -> TabActivity {
    let blocks = view.windows.iter().flat_map(blocks);
    let focused = view
        .windows
        .iter()
        .find(|w| w.window_id == view.active_window_id)
        .and_then(focused_block);
    fold_activity(model, blocks, focused)
}

/// One pane's activity: the same fold as its tab's, over its own records.
pub fn pane_activity(model: &HostModel, block: &str) -> TabActivity {
    fold_activity(model, std::iter::once(block), Some(block))
}

/// The record that speaks for a set: the highest state, and among equals one that reports
/// progress, then the latest report, so neither depends on record or pane order. A set is
/// one host's, so its report times all come from one server clock.
fn deciding<'a>(records: impl Iterator<Item = &'a StatusRecord>) -> Option<&'a StatusRecord> {
    records.max_by_key(|r| (r.state, r.progress.is_some(), r.updated_at))
}

fn fold_activity<'a>(
    model: &'a HostModel,
    blocks: impl Iterator<Item = &'a str> + Clone,
    focused: Option<&str>,
) -> TabActivity {
    let top = deciding(
        blocks
            .clone()
            .filter_map(|b| model.activity.get(b))
            .flat_map(|records| records.values()),
    );
    let program = focused.and_then(|b| {
        model
            .activity
            .get(b)
            .and_then(|records| deciding(records.values()))
            .and_then(|r| r.program().map(str::to_owned))
            .or_else(|| model.programs.get(b).cloned())
    });
    // A program's own OSC 9;4 report wins over the percentage in its status record.
    let progress = blocks
        .filter_map(|b| model.progress.get(b).copied())
        .max_by_key(|p| p.rank())
        .or_else(|| {
            top.filter(|r| r.state == ProgramState::Working)
                .and_then(|r| r.progress)
                .map(|n| TerminalProgress::Set(Some(n)))
        });
    TabActivity {
        state: top.map(|r| r.state).unwrap_or_default(),
        kind: top.and_then(|r| r.kind),
        msg: top.and_then(|r| r.msg.clone().or_else(|| r.title.clone())),
        live: top.is_some_and(|r| r.live),
        source: top.and_then(|r| r.program().map(str::to_owned)),
        program,
        progress,
    }
}

/// The icon for a program; shells and anything unknown keep the terminal glyph.
pub fn program_icon(program: Option<&str>) -> &'static rune::Icon {
    match program.map(str::to_ascii_lowercase).as_deref() {
        Some("claude" | "claude-code") => &CLAUDE,
        Some("codex") => &CODEX,
        Some("opencode") => &OPENCODE,
        // OMP is built on Pi and shares its skill.
        Some("pi" | "omp") => &PI,
        Some("nvim" | "vim" | "vi") => &VIM,
        Some("git" | "lazygit" | "tig") => &GIT,
        Some("cargo" | "rustc" | "rustup") => &CRATE,
        Some("ssh" | "mosh") => &KEY,
        _ => &TERMINAL,
    }
}

/// The word a pill shows after its label, with its colour: trouble always, then a bell that
/// rang while the tab was out of sight, then a paused or failed progress report, and Done
/// until the tab is seen. Idle and Working records show nothing: a program's progress while
/// it works is a bar ([`tab_progress`]), not a word.
pub fn tab_word(
    activity: &TabActivity,
    unseen_done: bool,
    unseen_bell: bool,
) -> Option<(Cow<'static, str>, u32)> {
    use TerminalProgress::{Error, Paused};
    match activity.state {
        ProgramState::Blocked => Some((blocked_word(activity.kind).into(), warning())),
        ProgramState::Error => Some(("Error".into(), danger())),
        _ if unseen_bell => Some(("Bell".into(), warning())),
        _ => match activity.progress {
            Some(Error(_)) => Some(("Error".into(), danger())),
            Some(Paused(Some(n))) => Some((format!("Paused {n}%").into(), warning())),
            Some(Paused(None)) => Some(("Paused".into(), warning())),
            _ if unseen_done => Some(("Done".into(), accent())),
            _ => None,
        },
    }
}

/// What tabs show of `activity` under the user's channels: a state whose tab channel is off
/// shows nothing, and progress bars follow Working's.
pub fn in_tabs(config: &NotificationsConfig, activity: &TabActivity) -> TabActivity {
    let mut shown = activity.clone();
    if config.channels(activity.state).is_some_and(|c| !c.tab) {
        shown.state = ProgramState::Idle;
        shown.kind = None;
        shown.msg = None;
    }
    if !config.working.tab {
        shown.progress = None;
    }
    shown
}

/// [`tab_word`] under the user's channels: unseen Done follows Done's tab channel and a bell
/// shows while any state does.
pub fn shown_word(
    config: &NotificationsConfig,
    activity: &TabActivity,
    unseen_done: bool,
    unseen_bell: bool,
) -> Option<(Cow<'static, str>, u32)> {
    tab_word(
        &in_tabs(config, activity),
        unseen_done && config.done.tab,
        unseen_bell && config.any_tab(),
    )
}

/// [`tab_progress`] under the user's channels.
pub fn shown_progress(
    config: &NotificationsConfig,
    activity: &TabActivity,
) -> Option<(Option<u8>, u32)> {
    tab_progress(&in_tabs(config, activity))
}

fn progress_word(progress: TerminalProgress) -> (Cow<'static, str>, u32) {
    use TerminalProgress::{Error, Indeterminate, Paused, Set};
    match progress {
        Error(_) => ("Error".into(), danger()),
        Paused(Some(n)) => (format!("Paused {n}%").into(), warning()),
        Paused(None) => ("Paused".into(), warning()),
        Set(Some(n)) => (format!("{n}%").into(), accent()),
        Set(None) | Indeterminate => ("Working".into(), accent()),
    }
}

/// The word a pane's activity row shows: trouble, then a program's progress report, then its
/// state. Unlike a tab's pill, Working, Done and Idle always have a word.
pub fn activity_word(activity: &TabActivity) -> (Cow<'static, str>, u32) {
    match activity.state {
        ProgramState::Blocked => (blocked_word(activity.kind).into(), warning()),
        ProgramState::Error => ("Error".into(), danger()),
        state => match (activity.progress, state) {
            (Some(progress), _) => progress_word(progress),
            (None, ProgramState::Done) => ("Done".into(), accent()),
            (None, ProgramState::Working) => ("Working".into(), accent()),
            (None, _) => ("Idle".into(), muted()),
        },
    }
}

/// The bar a pill draws under its label for a program's progress report: how far along, when
/// the program said, and its colour (`ACCENT` while it works, `WARNING` paused, `DANGER`
/// failed). None without a report, or while OSC 7501 trouble has the pill's attention.
pub fn tab_progress(activity: &TabActivity) -> Option<(Option<u8>, u32)> {
    use TerminalProgress::{Error, Indeterminate, Paused, Set};
    match activity.state {
        ProgramState::Blocked | ProgramState::Error => None,
        _ => match activity.progress? {
            Set(n) => Some((n, accent())),
            Indeterminate => Some((None, accent())),
            Paused(n) => Some((n, warning())),
            Error(n) => Some((n, danger())),
        },
    }
}

/// A 2 pt progress bar pinned along the bottom of a pill, inset to its text. A known
/// percentage fills that much of a faint track; an unknown one is the whole track in the
/// report's colour, dimmed. It never animates.
pub fn progress_bar(key: String, (percent, tone): (Option<u8>, u32), inset: f32) -> El {
    let bar = row()
        .key(key)
        .pin(Some(inset), None, Some(inset), Some(3.))
        .h(2.)
        .radius(1.);
    match percent {
        Some(percent) => {
            let fraction = (f32::from(percent.min(100)) / 100.).max(0.02);
            bar.fill(white(0.08))
                .child(el().grow(fraction).h_full().radius(1.).fill(rgb(tone)))
                .child(el().grow(1. - fraction))
        }
        None => bar.fill(rgb(tone)).opacity(0.35),
    }
}

fn blocked_word(kind: Option<BlockedKind>) -> &'static str {
    match kind {
        Some(BlockedKind::Permission) => "Needs approval",
        Some(BlockedKind::Question) => "Question",
        Some(BlockedKind::Auth) => "Needs sign-in",
        None => "Needs input",
    }
}

/// The freedesktop sound theme name a state's notification plays: attention for a program
/// that needs you, an error for one that failed, completion for one that finished.
fn state_sound(state: ProgramState) -> &'static str {
    match state {
        ProgramState::Blocked => "message-new-instant",
        ProgramState::Error => "dialog-error",
        ProgramState::Done => "complete",
        ProgramState::Working | ProgramState::Idle => "message",
    }
}

fn finished(before: ProgramState, now: ProgramState) -> bool {
    matches!(before, ProgramState::Working | ProgramState::Blocked)
        && matches!(now, ProgramState::Done | ProgramState::Idle)
}

impl Workspace {
    /// The tab is in front of the user: this window has focus and shows that tab.
    pub fn tab_seen(&self, index: usize, session: &str, window: &str) -> bool {
        let model = &self.hosts[index].model;
        self.window_focused
            && index == self.active
            && model.selected.as_deref() == Some(session)
            && model
                .views
                .get(session)
                .is_some_and(|v| v.active_window_id.as_deref() == Some(window))
    }

    /// Compare each pane of host `index` with its last state: latch unseen Done, clear what
    /// the user now sees, and notify about live changes in panes they can't see. A pane seen
    /// for the first time, including one that closed and came back, is only a baseline.
    pub fn track_activity(&mut self, index: usize) {
        let id = self.hosts[index].id;
        let epoch = self.hosts[index].model.activity_epoch;
        if self.activity_epochs.insert(id, epoch) != Some(epoch) {
            self.pane_states.retain(|(host, _), _| *host != id);
        }
        // Each pane present now: its track, whether its tab is in front, and its tab.
        let mut now: HashMap<(HostId, String), (PaneTrack, bool, String)> = HashMap::new();
        let mut notes = Vec::new();
        let mut latched = Vec::new();
        // Every tab present now, and whether it is in front.
        let mut tabs = HashMap::new();
        let model = &self.hosts[index].model;
        for (session, view) in &model.views {
            for window in &view.windows {
                let Some(window_id) = window.window_id.clone() else {
                    continue;
                };
                let seen = self.tab_seen(index, session, &window_id);
                tabs.insert(window_id.clone(), seen);
                for block in blocks(window) {
                    let key = (id, block.to_owned());
                    let activity = pane_activity(model, block);
                    let state = activity.state;
                    let Some(before) = self.pane_states.get(&key).copied() else {
                        let track = PaneTrack {
                            state,
                            finished: state == ProgramState::Done,
                        };
                        now.insert(key, (track, seen, window_id.clone()));
                        continue;
                    };
                    let done = finished(before.state, state);
                    let shown = if done { ProgramState::Done } else { state };
                    // Queries only restore what was already true; only live reports are news,
                    // and a completion already announced is not announced again.
                    let news = before.state != state
                        && (done || (activity.live && state != ProgramState::Idle))
                        && !(shown == ProgramState::Done && before.finished);
                    let finished = match state {
                        ProgramState::Working | ProgramState::Blocked | ProgramState::Error => {
                            false
                        }
                        _ => before.finished || shown == ProgramState::Done,
                    };
                    if news && !seen && shown == ProgramState::Done {
                        latched.push(key.clone());
                    }
                    if news && !seen {
                        notes.push((
                            shown,
                            session.clone(),
                            window_id.clone(),
                            block.to_owned(),
                            activity,
                        ));
                    }
                    now.insert(
                        key,
                        (PaneTrack { state, finished }, seen, window_id.clone()),
                    );
                }
            }
        }
        self.unseen_panes.extend(latched);
        self.unseen_panes.retain(|key| {
            key.0 != id
                || now.get(key).is_some_and(|(track, seen, _)| {
                    !seen && !matches!(track.state, ProgramState::Working | ProgramState::Blocked)
                })
        });
        self.unseen_done.retain(|key| key.0 != id);
        for (key, (.., window)) in &now {
            if self.unseen_panes.contains(key) {
                self.unseen_done.insert((id, window.clone()));
            }
        }
        // A bell shows until its tab is in front, and goes with its tab.
        self.unseen_bells
            .retain(|key| key.0 != id || tabs.get(&key.1).is_some_and(|seen| !seen));
        self.pane_states.retain(|(host, _), _| *host != id);
        self.pane_states
            .extend(now.into_iter().map(|(k, (track, ..))| (k, track)));
        for (state, session, window, block, activity) in notes {
            self.notify_state(index, state, session, window, block, &activity);
        }
        self.update_badge();
    }

    /// Panes that want the user, across connected hosts: Blocked or Error, or Done they
    /// haven't seen, each only while its state's Count channel is on. Working never counts.
    pub fn unread_count(&self) -> usize {
        self.unread().0
    }

    /// The unread count, and whether any of it is a pane that needs you or failed.
    pub fn unread(&self) -> (usize, bool) {
        let config = &self.settings.notifications;
        let (mut count, mut trouble) = (0, false);
        for host in self.hosts.iter().filter(|h| h.model.connected) {
            let model = &host.model;
            let panes: std::collections::HashSet<&str> = model
                .views
                .values()
                .flat_map(|v| &v.windows)
                .flat_map(blocks)
                .collect();
            for block in panes {
                let state = pane_activity(model, block).state;
                let needs = match state {
                    ProgramState::Blocked => config.blocked.badge,
                    ProgramState::Error => config.error.badge,
                    _ => false,
                };
                let done =
                    config.done.badge && self.unseen_panes.contains(&(host.id, block.to_owned()));
                if needs || done {
                    count += 1;
                    trouble |= needs;
                }
            }
        }
        (count, trouble)
    }

    /// Send the unread count to the desktop's launcher badge when it changed.
    pub fn update_badge(&mut self) {
        let count = self.unread_count();
        if count == self.badge {
            return;
        }
        self.badge = count;
        if cfg!(test) {
            self.badges.push(count);
            return;
        }
        let handle = self.handle.clone();
        self.notifier
            .get_or_insert_with(|| Notifier::spawn(handle))
            .badge(count);
    }

    fn notify_state(
        &mut self,
        index: usize,
        state: ProgramState,
        session: String,
        window: String,
        block: String,
        activity: &TabActivity,
    ) {
        let Some(channels) = self.settings.notifications.channels(state) else {
            return;
        };
        if !channels.desktop {
            return;
        }
        let sound = if channels.sound {
            Sound::Named(state_sound(state))
        } else {
            Sound::Silent
        };
        let word = match state {
            ProgramState::Blocked => blocked_word(activity.kind),
            ProgramState::Error => "Error",
            ProgramState::Done => "Done",
            _ => "Working",
        };
        let (tab, place) = self.describe_tab(index, &session, Some(&window));
        let summary = match activity.source.as_ref().or(activity.program.as_ref()) {
            Some(program) => format!("{program} · {word}"),
            None => format!("{tab} · {word}"),
        };
        let body = match &activity.msg {
            Some(msg) => format!("{msg}\n{place}"),
            None => place,
        };
        let host = self.hosts[index].id;
        self.deliver(Note {
            summary,
            body,
            target: NoteTarget {
                host,
                session,
                window: Some(window),
                block: Some(block),
            },
            sound,
        });
    }

    /// Pass on what programs asked to tell the user, unless their tab is in front.
    pub fn deliver_program_notices(
        &mut self,
        index: usize,
        notices: Vec<rex_linux::connection::activity::DesktopNotice>,
    ) {
        let messages = self.settings.notifications.messages;
        if !messages.desktop {
            return;
        }
        let sound = if messages.sound {
            Sound::Named("message")
        } else {
            Sound::Silent
        };
        for notice in notices {
            let window = self.hosts[index]
                .model
                .views
                .get(&notice.session)
                .and_then(|v| {
                    v.windows
                        .iter()
                        .find(|w| blocks(w).any(|b| b == notice.block))
                        .and_then(|w| w.window_id.clone())
                });
            if window
                .as_deref()
                .is_some_and(|w| self.tab_seen(index, &notice.session, w))
            {
                continue;
            }
            let (tab, place) = self.describe_tab(index, &notice.session, window.as_deref());
            let summary = if notice.title.is_empty() {
                tab
            } else {
                notice.title
            };
            let body = if notice.body.is_empty() {
                place
            } else {
                format!("{}\n{place}", notice.body)
            };
            let host = self.hosts[index].id;
            self.deliver(Note {
                summary,
                body,
                target: NoteTarget {
                    host,
                    session: notice.session,
                    window,
                    block: Some(notice.block),
                },
                sound,
            });
        }
    }

    /// A tab's label, and "session · host" for a notification body.
    fn describe_tab(&self, index: usize, session: &str, window: Option<&str>) -> (String, String) {
        let model = &self.hosts[index].model;
        let view = model.views.get(session);
        let tab = view
            .and_then(|v| v.windows.iter().find(|w| w.window_id.as_deref() == window))
            .map_or_else(|| "Terminal".into(), |w| tab_name(model, w));
        let session = model
            .sessions
            .iter()
            .find(|s| s.session_id == session)
            .map(|s| s.label.clone())
            .or_else(|| view.and_then(|v| v.label.clone()))
            .unwrap_or_else(|| "session".into());
        (
            tab,
            format!("{session} · {}", self.settings.hosts[index].label),
        )
    }

    fn deliver(&mut self, note: Note) {
        if cfg!(test) {
            self.outbox.push(note);
            return;
        }
        let handle = self.handle.clone();
        self.notifier
            .get_or_insert_with(|| Notifier::spawn(handle))
            .send(note);
    }

    /// A notification was clicked: raise the window and focus the pane it came from, found
    /// again by its host, session and block IDs. A pane that has closed is never swapped for
    /// whatever now sits where it was: its tab is shown if it still exists, with a notice.
    pub fn open_note(&mut self, target: NoteTarget, token: Option<String>, tx: &mut Tx) {
        self.handle.activate(token.unwrap_or_default());
        let Some(index) = self.host_index(target.host) else {
            self.inform("That pane has closed.");
            return;
        };
        if let Some(block) = &target.block {
            if let Some(window) = self.pane_window(index, &target.session, block) {
                self.focus_pane(index, target.session, window, block.clone(), tx);
                return;
            }
            if let Some(window) = target
                .window
                .filter(|w| self.window_exists(index, &target.session, w))
            {
                self.select_window(index, target.session, window, tx);
            }
            self.inform("That pane has closed.");
            return;
        }
        match target.window {
            Some(window) => self.select_window(index, target.session, window, tx),
            None => self.select(index, target.session, tx),
        }
    }

    /// The tab that holds `block` in a session host `index` lists, if any.
    fn pane_window(&self, index: usize, session: &str, block: &str) -> Option<String> {
        let model = &self.hosts[index].model;
        if !model.sessions.iter().any(|s| s.session_id == session) {
            return None;
        }
        model
            .views
            .get(session)?
            .windows
            .iter()
            .find(|w| blocks(w).any(|b| b == block))
            .and_then(|w| w.window_id.clone())
    }

    fn window_exists(&self, index: usize, session: &str, window: &str) -> bool {
        let model = &self.hosts[index].model;
        model.sessions.iter().any(|s| s.session_id == session)
            && model.views.get(session).is_some_and(|v| {
                v.windows
                    .iter()
                    .any(|w| w.window_id.as_deref() == Some(window))
            })
    }

    /// Show a session's tab and focus one of its panes.
    fn focus_pane(
        &mut self,
        index: usize,
        session: String,
        window: String,
        block: String,
        tx: &mut Tx,
    ) {
        self.select_window(index, session.clone(), window, tx);
        self.send(
            index,
            rex_linux::connection::HostCommand::Request {
                method: "session.focus_block".into(),
                payload: serde_json::json!({
                    "session_id": session,
                    "block_id": block,
                }),
            },
        );
    }

    /// An Activity row was pressed: show its session and focus the exact pane. The tab is
    /// found again from the block id, so a pane that has closed is never swapped for
    /// whatever now sits at its old position.
    pub fn open_pane(&mut self, target: &PaneRef, tx: &mut Tx) {
        // A host that just disconnected would only refuse the focus request.
        if let Some(index) = self.host_index(target.host)
            && !self.hosts[index].model.connected
        {
            let label = self.settings.hosts[index].label.clone();
            self.inform(format!("{label} is offline."));
            return;
        }
        let live = self.host_index(target.host).and_then(|index| {
            self.pane_window(index, &target.session, &target.block)
                .map(|window| (index, window))
        });
        let Some((index, window)) = live else {
            self.inform("That pane has closed.");
            return;
        };
        self.focus_pane(
            index,
            target.session.clone(),
            window,
            target.block.clone(),
            tx,
        );
    }

    /// Latch the bells programs rang, as (session, block), on tabs the user can't see.
    pub fn ring_bells(&mut self, index: usize, bells: Vec<(String, String)>) {
        let id = self.hosts[index].id;
        for (session, block) in bells {
            let window = self.hosts[index]
                .model
                .views
                .get(&session)
                .and_then(|v| v.windows.iter().find(|w| blocks(w).any(|b| b == block)))
                .and_then(|w| w.window_id.clone());
            if let Some(window) = window
                && !self.tab_seen(index, &session, &window)
            {
                self.unseen_bells.insert((id, window));
            }
        }
    }

    /// Whether `window` on the active host shows "Done" until seen.
    pub fn unseen(&self, window: &str) -> bool {
        self.unseen_on(self.active, window)
    }

    /// The word, tone and needing-you count a session shows, or None when it has nothing to
    /// say or tab activity is hidden. Unseen Done and Bell count from any of its tabs.
    pub fn session_word(
        &self,
        host: usize,
        session: &str,
    ) -> Option<(Cow<'static, str>, u32, usize)> {
        let config = &self.settings.notifications;
        let model = &self.hosts[host].model;
        let (fold, needing) = super::activity_list::session_activity(model, session)?;
        let windows = || {
            model
                .views
                .get(session)
                .into_iter()
                .flat_map(|v| &v.windows)
                .filter_map(|w| w.window_id.as_deref())
        };
        let done = windows().any(|w| self.unseen_on(host, w));
        let bell = windows().any(|w| self.bell_on(host, w));
        let (word, tone) = shown_word(config, &fold, done, bell)?;
        Some((word, tone, needing))
    }

    /// The progress bar a session's row draws, as a tab's pill does ([`tab_progress`]), or
    /// None when it has none or tab activity is hidden.
    pub fn session_progress(&self, host: usize, session: &str) -> Option<(Option<u8>, u32)> {
        let (fold, _) = super::activity_list::session_activity(&self.hosts[host].model, session)?;
        shown_progress(&self.settings.notifications, &fold)
    }

    pub fn unseen_on(&self, host: usize, window: &str) -> bool {
        self.unseen_done
            .contains(&(self.hosts[host].id, window.to_owned()))
    }

    /// Whether `window` on host `host` rang its bell while out of sight.
    pub fn bell_on(&self, host: usize, window: &str) -> bool {
        self.unseen_bells
            .contains(&(self.hosts[host].id, window.to_owned()))
    }
}

/// What a pane's activity was when last synced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneTrack {
    pub state: ProgramState,
    /// Its completion has been announced (or was already true at its baseline), so another
    /// Done before new work is the same completion.
    pub finished: bool,
}

/// Each pane's activity when last synced, by host and block.
pub type PaneStates = HashMap<(HostId, String), PaneTrack>;

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str, label: Option<&str>) -> (HostModel, session::Window) {
        let mut model = HostModel::default();
        model
            .titles
            .insert("block".into(), ("dotfiles".into(), None));
        let window = session::Window {
            window_id: Some(id.into()),
            label: label.map(str::to_owned),
            focused_block_id: Some("block".into()),
            layers: vec![session::Layer {
                blocks: vec![session::LayerBlock {
                    block_id: Some("block".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        (model, window)
    }

    #[test]
    fn generated_window_labels_use_current_terminal_titles() {
        for (id, label, title) in [
            (
                "window:01a1aa89-4009-757a-a7b9-1ce89dcd0a4c",
                "Window 9dcd0a4c",
                "dotfiles",
            ),
            (
                "window:01a1ad21-2a48-7290-8bd0-6c4d624e75b5",
                "Window 624e75b5",
                "bedcontrol",
            ),
        ] {
            let (mut model, window) = tab(id, Some(label));
            model.titles.insert("block".into(), (title.into(), None));
            assert_eq!(tab_name(&model, &window), title);
        }
    }

    #[test]
    fn explicit_window_names_keep_their_labels() {
        for label in ["Work", "Window 624e75b5", "Window notes", "Shell commands"] {
            let (model, window) = tab("window:01a1aa89-4009-757a-a7b9-1ce89dcd0a4c", Some(label));
            assert_eq!(tab_name(&model, &window), label);
        }
        let (model, mut window) = tab("short", Some("Window 9dcd0a4c"));
        assert_eq!(tab_name(&model, &window), "Window 9dcd0a4c");
        window.window_id = None;
        assert_eq!(tab_name(&model, &window), "Window 9dcd0a4c");
    }

    #[test]
    fn generated_window_labels_survive_missing_and_stale_titles() {
        let (mut model, window) = tab(
            "window:01a1aa89-4009-757a-a7b9-1ce89dcd0a4c",
            Some("Window 9dcd0a4c"),
        );
        model.titles.clear();
        assert_eq!(tab_name(&model, &window), "Window 9dcd0a4c");
        model
            .titles
            .insert("block".into(), ("stale".into(), Some(42)));
        model.foreground.insert("block".into(), vec![99]);
        assert_eq!(tab_name(&model, &window), "Window 9dcd0a4c");
    }

    fn pane(label: Option<&str>) -> (HostModel, session::LayerBlock) {
        let mut model = HostModel::default();
        model
            .titles
            .insert("block".into(), ("Claude Code".into(), Some(7)));
        model.foreground.insert("block".into(), vec![7, 1]);
        let block = session::LayerBlock {
            block_id: Some("block".into()),
            label: label.map(str::to_owned),
            creator_name: Some("com.superlogical.terminal".into()),
            flavor: Some("com.superlogical.terminal.shell".into()),
            ..Default::default()
        };
        (model, block)
    }

    #[test]
    fn default_pane_labels_show_the_current_terminal_title() {
        for label in [Some("shell"), None, Some("")] {
            let (model, block) = pane(label);
            assert_eq!(pane_name(&model, &block), "Claude Code");
        }
    }

    #[test]
    fn renamed_panes_keep_their_labels() {
        for label in ["build", "Shell", "server", "Claude"] {
            let (model, block) = pane(Some(label));
            assert_eq!(pane_name(&model, &block), label);
        }
    }

    #[test]
    fn pane_titles_go_stale_with_their_program() {
        let (mut model, block) = pane(Some("shell"));
        model.foreground.insert("block".into(), vec![1]);
        assert_eq!(pane_name(&model, &block), "shell");
        model.titles.clear();
        let (_, mut bare) = pane(None);
        bare.flavor = None;
        assert_eq!(pane_name(&model, &bare), "Terminal");
    }

    #[test]
    fn only_terminal_blocks_take_titles() {
        let (model, mut block) = pane(Some("shell"));
        block.creator_name = Some("com.example.other".into());
        assert_eq!(pane_name(&model, &block), "shell");
    }

    #[test]
    fn among_equal_records_the_latest_report_speaks() {
        let report = |at: &str, msg: &str| {
            StatusRecord::parse(&serde_json::json!({"state":"blocked","msg":msg,"updated_at":at}))
        };
        let mut model = HostModel::default();
        // "a" sorts first but was reported later, by the server's clock in another offset.
        model.activity.insert(
            "block".into(),
            [
                ("a".into(), report("2026-11-02T00:45:00-08:00", "later")),
                ("b".into(), report("2026-11-02T08:30:00Z", "earlier")),
            ]
            .into(),
        );
        assert_eq!(pane_activity(&model, "block").msg.as_deref(), Some("later"));
    }

    #[test]
    fn a_record_without_an_app_is_named_by_its_owner() {
        let mut model = HostModel::default();
        let record = StatusRecord::parse(&serde_json::json!({
            "state": "done", "updated_at": "2026-11-02T08:30:00Z",
            "owner": {"pid": 7, "name": "claude"},
        }));
        model
            .activity
            .insert("block".into(), [(String::new(), record)].into());
        // The program has since left the foreground.
        model.programs.insert("block".into(), "bash".into());
        let activity = pane_activity(&model, "block");
        assert_eq!(activity.source.as_deref(), Some("claude"));
        assert_eq!(activity.program.as_deref(), Some("claude"));
    }

    #[test]
    fn shell_and_unlabelled_windows_keep_their_title_fallback() {
        for label in [Some(DEFAULT_TAB_LABEL), None, Some("")] {
            let (model, window) = tab("window", label);
            assert_eq!(tab_name(&model, &window), "dotfiles");
        }
    }
}
