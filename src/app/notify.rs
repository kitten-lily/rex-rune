//! Desktop notifications through the freedesktop notification service, and the unread count
//! as a launcher badge (`com.canonical.Unity.LauncherEntry`), on a worker thread so D-Bus
//! never blocks the UI. Clicking one raises the window (with the activation token
//! the server sends just before the click, so Wayland lets it take focus) and focuses its pane.
use super::workspace::{HostId, Workspace};
use futures_util::StreamExt;
use rune::AppHandle;
use std::collections::{HashMap, VecDeque};
use zbus::{Connection, Proxy, Variant};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// Clicks are remembered for this many recent notifications.
const REMEMBERED: usize = 64;
/// The desktop entry docks match a launcher badge to, as `application://<id>.desktop`.
pub const DESKTOP_ID: &str = "dev.shuv.rex-linux";
const LAUNCHER_PATH: &str = "/dev/shuv/rex_linux/LauncherEntry";
const LAUNCHER_INTERFACE: &str = "com.canonical.Unity.LauncherEntry";

#[derive(Clone, Debug, PartialEq)]
pub struct NoteTarget {
    pub host: HostId,
    pub session: String,
    pub window: Option<String>,
    /// The pane the notification came from, focused when it is clicked.
    pub block: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub summary: String,
    pub body: String,
    pub target: NoteTarget,
}

enum Message {
    Note(Note),
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
                if let Err(error) = runtime.block_on(run(receiver, handle)) {
                    eprintln!("rex-linux: desktop notifications unavailable: {error}");
                }
            });
        Self { notes }
    }

    pub fn send(&self, note: Note) {
        let _ = self.notes.send(Message::Note(note));
    }

    /// Show `count` on the app's launcher icon, or hide the badge at zero.
    pub fn badge(&self, count: usize) {
        let _ = self.notes.send(Message::Badge(count));
    }
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

async fn run(notes: flume::Receiver<Message>, handle: AppHandle<Workspace>) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    let proxy = Proxy::new(&conn, DEST, PATH, DEST).await?;
    // One stream keeps ActivationToken ahead of the ActionInvoked it belongs to.
    let mut signals = proxy.receive_all_signals().await?;
    let mut targets: HashMap<u32, NoteTarget> = HashMap::new();
    let mut order: VecDeque<u32> = VecDeque::new();
    let mut tokens: HashMap<u32, String> = HashMap::new();
    loop {
        tokio::select! {
            message = notes.recv_async() => {
                let note = match message {
                    Ok(Message::Note(note)) => note,
                    Ok(Message::Badge(count)) => {
                        // Nobody may be listening; a dock that starts later asks no one.
                        let _ = conn.emit_signal(
                            None::<()>,
                            LAUNCHER_PATH,
                            LAUNCHER_INTERFACE,
                            "Update",
                            &(format!("application://{DESKTOP_ID}.desktop"), badge_properties(count)),
                        )
                        .await;
                        continue;
                    }
                    Err(_) => return Ok(()),
                };
                let hints = HashMap::from([("desktop-entry", Variant::from(DESKTOP_ID))]);
                // Without a notification service the badge still works, so go on.
                let id: u32 = match proxy
                    .call(
                        "Notify",
                        &("Rex", 0u32, DESKTOP_ID, &note.summary, &note.body,
                          vec!["default", "Show"], hints, -1i32),
                    )
                    .await
                {
                    Ok(id) => id,
                    Err(error) => {
                        eprintln!("rex-linux: desktop notification not shown: {error}");
                        continue;
                    }
                };
                targets.insert(id, note.target);
                order.push_back(id);
                while order.len() > REMEMBERED {
                    if let Some(old) = order.pop_front() {
                        targets.remove(&old);
                        tokens.remove(&old);
                    }
                }
            }
            signal = signals.next() => {
                let Some(signal) = signal else { return Ok(()) };
                let header = signal.header();
                match header.member().map(|m| m.as_str()) {
                    Some("ActivationToken") => {
                        if let Ok((id, token)) = signal.body().deserialize::<(u32, String)>()
                            && targets.contains_key(&id)
                        {
                            tokens.insert(id, token);
                        }
                    }
                    Some("ActionInvoked") => {
                        if let Ok((id, _action)) = signal.body().deserialize::<(u32, String)>()
                            && let Some(target) = targets.remove(&id)
                        {
                            let token = tokens.remove(&id);
                            handle.update(move |s, tx| s.open_note(target, token, tx));
                        }
                    }
                    Some("NotificationClosed") => {
                        if let Ok((id, _reason)) = signal.body().deserialize::<(u32, u32)>() {
                            targets.remove(&id);
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

    #[test]
    fn the_badge_shows_a_count_and_hides_at_zero() {
        let three = badge_properties(3);
        assert_eq!(three["count"], Variant::from(3i64));
        assert_eq!(three["count-visible"], Variant::from(true));
        assert_eq!(badge_properties(0)["count-visible"], Variant::from(false));
    }
}
