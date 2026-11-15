//! Desktop notifications, and the unread count as a launcher badge, on a worker thread, so
//! asking the desktop never blocks the UI. Each platform's service lives in
//! `platform::notify`; the decisions about replacing, closing and clicking are the same
//! everywhere. Clicking one raises the window (with the activation token some desktops send
//! just before the click, so Wayland lets it take focus) and focuses its pane.
use super::platform::notify::Desktop;
use super::workspace::{HostId, Workspace};
use rune::AppHandle;
use std::collections::HashMap;

#[cfg(test)]
#[path = "notify/tests.rs"]
mod decision_tests;
mod worker;
use worker::Ledger;
#[cfg(test)]
pub use worker::Outbox;
pub(crate) use worker::{Backend, Event, Session};
pub use worker::{Command, Health, Service, TestResult};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteTarget {
    pub host: HostId,
    pub session: String,
    pub window: Option<String>,
    /// The pane the notification came from, focused when it is clicked. Replacing and
    /// closing go by tab and ignore it.
    pub block: Option<String>,
}

impl NoteTarget {
    /// The tab this target is in, without its pane.
    pub fn tab(&self) -> Self {
        Self {
            block: None,
            ..self.clone()
        }
    }

    pub fn same_tab(&self, other: &Self) -> bool {
        self.host == other.host && self.session == other.session && self.window == other.window
    }
}

/// What a notification asks the server to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// A sound from the desktop's sound theme, by its freedesktop name, as `sound-name`.
    Named(&'static str),
    /// `suppress-sound`, so a server that plays a sound for every notification stays quiet.
    Silent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub summary: String,
    pub body: String,
    pub target: Option<NoteTarget>,
    pub kind: Kind,
    pub sound: Sound,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Activity(rex_rune::connection::activity::ProgramState),
    Program,
    Test,
}

impl Note {
    fn test() -> Self {
        Self {
            summary: "Rex test notification".into(),
            body: "Desktop notifications can reach this desktop.".into(),
            target: None,
            kind: Kind::Test,
            sound: Sound::Named("message"),
        }
    }
}

/// What the worker is asked to do: a notification command, or a new launcher badge count.
enum Message {
    Command(Command),
    Badge(usize),
}

pub struct Notifier {
    notes: flume::Sender<Message>,
}

impl Notifier {
    pub fn spawn(handle: AppHandle<Workspace>) -> Self {
        let (notes, receiver) = flume::unbounded();
        let _ = std::thread::Builder::new()
            .name("rex-notifications".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(run(receiver, handle));
            });
        Self { notes }
    }

    pub fn send(&self, command: Command) -> bool {
        self.notes.send(Message::Command(command)).is_ok()
    }

    /// Show `count` on the app's launcher icon, or hide the badge at zero.
    pub fn badge(&self, count: usize) -> bool {
        self.notes.send(Message::Badge(count)).is_ok()
    }
}

impl Workspace {
    pub fn notification_command(&mut self, command: Command) {
        #[cfg(test)]
        {
            if cfg!(test) {
                self.notification_health = self.notification_outbox.execute(command);
                return;
            }
        }
        {
            if matches!(command, Command::Test) {
                self.notification_health.test = TestResult::Sending;
            }
            let handle = self.handle.clone();
            if !self
                .notifier
                .get_or_insert_with(|| Notifier::spawn(handle))
                .send(command)
            {
                self.notifier = None;
                self.notification_health.service =
                    Service::Unreachable("Notification worker stopped".into());
                if self.notification_health.test == TestResult::Sending {
                    self.notification_health.test =
                        TestResult::Failed("Notification worker stopped".into());
                }
            }
        }
    }

    /// The tab is on screen, or its notification was clicked. Asked once for the notes sent
    /// since it was last seen, never on every update: a daemon that does not answer costs
    /// one bounded attempt, and a close that failed is not tried again.
    pub fn close_tab_notification(&mut self, target: NoteTarget) {
        if self.notified_tabs.remove(&target.tab()) {
            self.close_notifications(target);
        }
    }

    /// Close what this client sent about `target`'s tab, whether or not it was a tab's.
    pub fn close_notifications(&mut self, target: NoteTarget) {
        let target = target.tab();
        self.notified_tabs.remove(&target);
        // Merely viewing tabs must not start a desktop worker or ask the desktop anything.
        if self.notifier.is_some() || cfg!(test) {
            self.notification_command(Command::Seen(target));
        }
    }

    /// The worker's news. A check or a note that finishes while a test is still on its way
    /// leaves "Sending test…" showing.
    pub(super) fn notification_health_posted(&mut self, health: Health, answers_test: bool) {
        let sending = !answers_test && self.notification_health.test == TestResult::Sending;
        self.notification_health = health;
        if sending {
            self.notification_health.test = TestResult::Sending;
        }
    }
}

/// The window and its notification worker are already gone after a failed re-exec.
/// Post one critical notification before exiting (also for desktop launches), through the
/// platform's service where it has one.
pub fn notify_restart_failure(message: &str) -> anyhow::Result<()> {
    super::platform::notify::restart_failure(message)
}

/// The worker: run commands, and follow what the desktop does with the notes it was given.
/// A lost service reports one health update and waits for a later command to reconnect.
async fn run(notes: flume::Receiver<Message>, handle: AppHandle<Workspace>) {
    let mut desktop = Desktop::new();
    let mut ledger = Ledger::default();
    // Activation tokens for live notifications, where the desktop sends them.
    let mut tokens = HashMap::new();
    // Seeing a tab sends a command on every activity update; wake the UI only for news.
    let mut shown = ledger.health.clone();
    loop {
        tokio::select! {
            command = notes.recv_async() => {
                let command = match command {
                    Ok(Message::Command(command)) => command,
                    Ok(Message::Badge(count)) => {
                        desktop.badge(count).await;
                        continue;
                    }
                    Err(_) => return,
                };
                // A test always answers: the UI is showing "Sending test…".
                let test = matches!(command, Command::Test);
                let identity = desktop.identity();
                ledger.execute(command, &mut desktop).await;
                if desktop.identity() != identity {
                    tokens.clear();
                }
                tokens.retain(|id, _| ledger.target(*id).is_some());
                if ledger.health != shown || test {
                    shown = ledger.health.clone();
                    let health = shown.clone();
                    handle.update(move |s, _| s.notification_health_posted(health, test));
                }
            }
            event = desktop.event() => match event {
                Event::ActivationToken(id, token) => {
                    if ledger.target(id).is_some() {
                        tokens.insert(id, token);
                    }
                }
                Event::Click(id) => {
                    if let Some(target) = ledger.click(id) {
                        let token = tokens.remove(&id);
                        // Keep the id until Seen closes it (or the desktop reports a close).
                        handle.update(move |s, tx| s.open_note(target, token, tx));
                    }
                }
                Event::Closed(id) => {
                    ledger.closed(id);
                    tokens.remove(&id);
                }
                Event::Lost(reason) => {
                    tokens.clear();
                    ledger.health.service = Service::Unreachable(reason);
                    shown = ledger.health.clone();
                    let health = shown.clone();
                    handle.update(move |s, _| s.notification_health_posted(health, false));
                }
            }
        }
    }
}
