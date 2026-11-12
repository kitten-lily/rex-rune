//! Desktop notifications through the freedesktop notification service, and the unread count
//! as a launcher badge (`com.canonical.Unity.LauncherEntry`), on a worker thread so D-Bus
//! never blocks the UI. Clicking one raises the window (with the activation token
//! the server sends just before the click, so Wayland lets it take focus) and focuses its pane.
use super::workspace::{HostId, Workspace};
use futures_util::StreamExt;
use rune::AppHandle;
use std::collections::HashMap;

#[cfg(test)]
#[path = "notify/tests.rs"]
mod decision_tests;
mod worker;
#[cfg(test)]
pub use worker::Outbox;
use worker::{Backend, Ledger};
pub use worker::{Command, Health, Service, TestResult};
use zbus::{Connection, Proxy, Variant};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// The desktop entry docks match a launcher badge to, as `application://<id>.desktop`.
pub const DESKTOP_ID: &str = "dev.shuv.rex-linux";
const LAUNCHER_PATH: &str = "/dev/shuv/rex_linux/LauncherEntry";
const LAUNCHER_INTERFACE: &str = "com.canonical.Unity.LauncherEntry";
/// The picture each notification shows. GNOME draws only a small, plain app icon without one.
const IMAGE: &[u8] = include_bytes!("../../resources/icons/128x128/apps/dev.shuv.rex-linux.png");

/// `IMAGE` as the spec's `image-data` hint, `(iiibiiay)`: width, height, row stride, alpha,
/// bits per sample, channels and RGBA bytes. Sent with every notification, so it needs no
/// installed icon file.
fn image_data() -> Option<Variant<'static>> {
    let image = image::load_from_memory_with_format(IMAGE, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    let (width, height) = (width as i32, height as i32);
    Some(Variant::from((
        width,
        height,
        width * 4,
        true,
        8i32,
        4i32,
        image.into_raw(),
    )))
}

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

/// The hints a notification carries: the desktop entry it belongs to, its urgency and its
/// sound.
pub fn note_hints(note: &Note) -> HashMap<&'static str, Variant<'static>> {
    let mut hints = HashMap::from([
        ("desktop-entry", Variant::from(DESKTOP_ID)),
        ("urgency", Variant::from(note.kind.urgency())),
    ]);
    match note.sound {
        Sound::Named(name) => hints.insert("sound-name", Variant::from(name)),
        Sound::Silent => hints.insert("suppress-sound", Variant::from(true)),
    };
    hints
}

/// The LauncherEntry `Update` properties for an unread count.
pub fn badge_properties(count: usize) -> HashMap<&'static str, Variant<'static>> {
    HashMap::from([
        (
            "count",
            Variant::from(i64::try_from(count).unwrap_or(i64::MAX)),
        ),
        ("count-visible", Variant::from(count > 0)),
    ])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Activity(rex_linux::connection::activity::ProgramState),
    Program,
    Test,
}

impl Kind {
    pub fn urgency(self) -> u8 {
        use rex_linux::connection::activity::ProgramState::{Blocked, Error};
        if matches!(self, Self::Activity(Blocked | Error)) {
            2
        } else {
            1
        }
    }
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
        // Merely viewing tabs must not start a desktop worker or connect to D-Bus.
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
/// Send one critical, persistent notification before exiting (also for desktop launches).
pub fn notify_restart_failure(message: &str) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let conn = Connection::session().await?;
            let proxy = Proxy::new(&conn, DEST, PATH, DEST).await?;
            let hints = HashMap::from([
                ("desktop-entry", Variant::from(DESKTOP_ID)),
                ("urgency", Variant::from(2u8)),
            ]);
            let _: u32 = proxy
                .call(
                    "Notify",
                    &(
                        "Rex",
                        0u32,
                        DESKTOP_ID,
                        "Rex restart failed",
                        message,
                        Vec::<String>::new(),
                        hints,
                        0i32,
                    ),
                )
                .await?;
            Ok::<_, zbus::Error>(())
        })
        .await??;
        Ok(())
    })
}

struct Bus {
    proxy: Proxy<'static>,
    signals: zbus::proxy::SignalStream<'static>,
    identity: String,
}

struct Desktop {
    bus: Option<Bus>,
    /// Decoded once for the worker's life.
    image: Option<Variant<'static>>,
}

const DISCONNECTED: &str = "Session bus disconnected";

/// The service answered a call on its pinned connection with an error: the connection
/// and its signals are still good. Only asked of `Notify` and `CloseNotification`, never
/// of connecting: an error while finding or starting the service comes from the bus.
fn answered(error: &zbus::Error) -> bool {
    use zbus::fdo::Error::*;
    match error {
        zbus::Error::MethodError(..) => true,
        zbus::Error::FDO(error) => !matches!(
            **error,
            ServiceUnknown(_)
                | NameHasNoOwner(_)
                | NoReply(_)
                | Timeout(_)
                | TimedOut(_)
                | NoServer(_)
                | Disconnected(_)
                | SpawnExecFailed(_)
                | SpawnForkFailed(_)
                | SpawnChildExited(_)
                | SpawnChildSignaled(_)
                | SpawnFailed(_)
                | SpawnFailedToSetup(_)
                | SpawnConfigInvalid(_)
                | SpawnServiceNotValid(_)
        ),
        _ => false,
    }
}

/// A call to the pinned service, bounded. `Rejected` is its refusal; the flag says the
/// connection can no longer be trusted.
async fn pinned<T>(
    future: impl std::future::Future<Output = zbus::Result<T>>,
) -> Result<T, (Service, bool)> {
    match tokio::time::timeout(std::time::Duration::from_secs(3), future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) if answered(&error) => Err((Service::Rejected, false)),
        Ok(Err(error)) => Err((failure(error), true)),
        Err(_) => Err((Service::Unreachable("Service did not respond".into()), true)),
    }
}

fn failure(error: zbus::Error) -> Service {
    // Short, stable reasons; don't put daemon-controlled diagnostics in Settings.
    let reason = match error {
        zbus::Error::FDO(error) => match *error {
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_) => {
                return Service::NotFound;
            }
            zbus::fdo::Error::NoReply(_) | zbus::fdo::Error::Timeout(_) => {
                "Service did not respond"
            }
            _ => "Could not reach the service",
        },
        zbus::Error::InputOutput(_) => DISCONNECTED,
        _ => "Could not reach the service",
    };
    Service::Unreachable(reason.into())
}

async fn bounded<T>(
    future: impl std::future::Future<Output = zbus::Result<T>>,
) -> Result<T, Service> {
    tokio::time::timeout(std::time::Duration::from_secs(3), future)
        .await
        .map_err(|_| Service::Unreachable("Service did not respond".into()))?
        .map_err(failure)
}

impl Desktop {
    async fn connect() -> zbus::Result<Bus> {
        let conn = Connection::session().await?;
        let service = Proxy::new(&conn, DEST, PATH, DEST).await?;
        // A real answer, not merely a connection to the session bus. Also allows D-Bus
        // activation of an installed notification daemon.
        let _: (String, String, String, String) = service.call("GetServerInformation", &()).await?;
        let owner = zbus::fdo::DBusProxy::new(&conn)
            .await?
            .get_name_owner(DEST.try_into().expect("constant bus name"))
            .await?;
        let identity = format!("{}:{owner}", conn.server_guid());
        // Pin calls/signals to this daemon: ids from a replaced service are never reused.
        let proxy = Proxy::new_owned(conn, owner.to_string(), PATH, DEST).await?;
        let signals = proxy.receive_all_signals().await?;
        Ok(Bus {
            proxy,
            signals,
            identity,
        })
    }
}

impl Backend for Desktop {
    async fn ready(&mut self) -> Result<String, Service> {
        if let Some(bus) = &self.bus {
            let conn = bus.proxy.connection();
            let owner = bounded(async {
                Ok(zbus::fdo::DBusProxy::new(conn)
                    .await?
                    .get_name_owner(DEST.try_into().expect("constant bus name"))
                    .await?)
            })
            .await;
            match owner {
                Ok(owner) if format!("{}:{owner}", conn.server_guid()) == bus.identity => {
                    let _: (String, String, String, String) =
                        bounded(bus.proxy.call("GetServerInformation", &())).await?;
                    return Ok(bus.identity.clone());
                }
                _ => self.bus = None,
            }
        }
        let bus = bounded(Self::connect())
            .await
            .map_err(|error| match error {
                // There was no connection to lose.
                Service::Unreachable(reason) if reason == DISCONNECTED => {
                    Service::Unreachable("Could not connect to the session bus".into())
                }
                error => error,
            })?;
        let identity = bus.identity.clone();
        self.bus = Some(bus);
        Ok(identity)
    }

    async fn send(&mut self, note: &Note, replaces: u32) -> Result<u32, Service> {
        let bus = self.bus.as_ref().expect("ready before send");
        let mut hints = note_hints(note);
        if let Some(image) = self.image.as_ref().and_then(|i| i.try_clone().ok()) {
            hints.insert("image-data", image);
        }
        let actions = if note.target.is_some() {
            vec!["default", "Show"]
        } else {
            vec![]
        };
        let body = (
            "Rex",
            replaces,
            DESKTOP_ID,
            &note.summary,
            &note.body,
            actions,
            hints,
            -1i32,
        );
        // A refusal keeps the connection: reconnecting would take a new bus name and
        // lose the clicks and closes of notifications already sent.
        let result = pinned(bus.proxy.call::<_, _, u32>("Notify", &body)).await;
        result.map_err(|(error, lost)| {
            if lost {
                self.bus = None;
            }
            error
        })
    }

    async fn close(&mut self, id: u32) -> Result<(), Service> {
        let Some(bus) = &self.bus else {
            return Err(Service::Unreachable(DISCONNECTED.into()));
        };
        pinned(bus.proxy.call("CloseNotification", &(id,)))
            .await
            .map_err(|(error, _)| error)
    }
}

/// Emit the launcher badge's `Update` signal on the session bus. Nobody may be listening:
/// a dock that starts later asks no one. Needs no notification service, so a desktop without
/// one still shows the count.
async fn emit_badge(conn: &mut Option<Connection>, count: usize) {
    if conn.is_none() {
        *conn = bounded(Connection::session()).await.ok();
    }
    let Some(bus) = conn.as_ref() else { return };
    let sent = bounded(bus.emit_signal(
        None::<()>,
        LAUNCHER_PATH,
        LAUNCHER_INTERFACE,
        "Update",
        &(
            format!("application://{DESKTOP_ID}.desktop"),
            badge_properties(count),
        ),
    ))
    .await;
    if sent.is_err() {
        // Connect again for the next count.
        *conn = None;
    }
}

async fn run(notes: flume::Receiver<Message>, handle: AppHandle<Workspace>) {
    let mut desktop = Desktop {
        bus: None,
        image: image_data(),
    };
    let mut launcher: Option<Connection> = None;
    let mut ledger = Ledger::default();
    let mut tokens = HashMap::new();
    // Seeing a tab sends a command on every activity update; wake the UI only for news.
    let mut shown = ledger.health.clone();
    loop {
        tokio::select! {
            command = notes.recv_async() => {
                let command = match command {
                    Ok(Message::Command(command)) => command,
                    Ok(Message::Badge(count)) => {
                        emit_badge(&mut launcher, count).await;
                        continue;
                    }
                    Err(_) => return,
                };
                // A test always answers: the UI is showing "Sending test…".
                let test = matches!(command, Command::Test);
                let identity = desktop.bus.as_ref().map(|bus| bus.identity.clone());
                ledger.execute(command, &mut desktop).await;
                if desktop.bus.as_ref().map(|bus| &bus.identity) != identity.as_ref() {
                    tokens.clear();
                }
                tokens.retain(|id, _| ledger.target(*id).is_some());
                if ledger.health != shown || test {
                    shown = ledger.health.clone();
                    let health = shown.clone();
                    handle.update(move |s, _| s.notification_health_posted(health, test));
                }
            }
            signal = async {
                match &mut desktop.bus {
                    Some(bus) => bus.signals.next().await,
                    None => std::future::pending().await,
                }
            } => {
                let Some(signal) = signal else {
                    desktop.bus = None;
                    tokens.clear();
                    ledger.health.service = Service::Unreachable(DISCONNECTED.into());
                    shown = ledger.health.clone();
                    let health = shown.clone();
                    handle.update(move |s, _| s.notification_health_posted(health, false));
                    continue;
                };
                match signal.header().member().map(|m| m.as_str()) {
                    Some("ActivationToken") => {
                        if let Ok((id, token)) = signal.body().deserialize::<(u32, String)>()
                            && ledger.target(id).is_some()
                        {
                            tokens.insert(id, token);
                        }
                    }
                    Some("ActionInvoked") => {
                        if let Ok((id, action)) = signal.body().deserialize::<(u32, String)>()
                            && action == "default"
                            && let Some(target) = ledger.click(id)
                        {
                            let token = tokens.remove(&id);
                            // Keep the id until Seen closes it (or NotificationClosed arrives).
                            handle.update(move |s, tx| s.open_note(target, token, tx));
                        }
                    }
                    Some("NotificationClosed") => {
                        if let Ok((id, _reason)) = signal.body().deserialize::<(u32, u32)>() {
                            ledger.closed(id);
                            tokens.remove(&id);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(kind: Kind, sound: Sound) -> Note {
        Note {
            summary: String::new(),
            body: String::new(),
            target: None,
            kind,
            sound,
        }
    }

    #[test]
    fn a_sound_is_named_or_suppressed() {
        let named = note_hints(&note(Kind::Program, Sound::Named("complete")));
        assert_eq!(named["sound-name"], Variant::from("complete"));
        assert!(!named.contains_key("suppress-sound"));
        let silent = note_hints(&note(Kind::Program, Sound::Silent));
        assert_eq!(silent["suppress-sound"], Variant::from(true));
        assert!(!silent.contains_key("sound-name"));
        for hints in [named, silent] {
            assert_eq!(hints["desktop-entry"], Variant::from(DESKTOP_ID));
        }
    }

    #[test]
    fn the_badge_shows_a_count_and_hides_at_zero() {
        let three = badge_properties(3);
        assert_eq!(three["count"], Variant::from(3i64));
        assert_eq!(three["count-visible"], Variant::from(true));
        assert_eq!(badge_properties(0)["count-visible"], Variant::from(false));
    }

    #[test]
    fn the_notification_picture_matches_the_freedesktop_image_data_format() {
        let image = super::image_data().expect("embedded Rex icon decodes");
        let (width, height, stride, alpha, bits, channels, pixels): (
            i32,
            i32,
            i32,
            bool,
            i32,
            i32,
            Vec<u8>,
        ) = image.try_into().expect("image-data is (iiibiiay)");
        assert_eq!((width, height, stride), (128, 128, 128 * 4));
        assert_eq!((alpha, bits, channels), (true, 8, 4));
        assert_eq!(pixels.len(), (stride * height) as usize);
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }
}
