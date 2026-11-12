//! What each known terminal last reported before this client was listening: its program
//! status, title, foreground process and working directory. Every session whose view is known
//! is read, the selected one first, and every block's status before any title, process or
//! directory, since a row needs only its status to appear. One request is in flight at a time
//! and each (block, probe) is asked once per connection, alongside control traffic rather than
//! in its way. Live events keep the answers current afterwards; a status read that a live
//! event outdated is asked again, while any other outdated read is dropped, since its event
//! already carried the whole value.
use crate::{
    connection::{
        HostModel,
        activity::{self, Probe},
        view_blocks,
    },
    transport::Client,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

/// The gap between probes, so reading a large server doesn't crowd out other requests.
pub const PACE: Duration = Duration::from_millis(25);
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct Probes {
    /// Each block's probes asked on this connection, by `Probe as usize`, answered or not: a
    /// failed read isn't retried until the next connection, only an outdated status read.
    started: HashMap<String, [bool; 4]>,
    in_flight: Option<InFlight>,
}

struct InFlight {
    session: String,
    block: String,
    probe: Probe,
    stamp: u64,
    reply: oneshot::Receiver<String>,
    deadline: Instant,
}

/// A probe that answered, failed or timed out.
#[derive(Debug)]
pub struct Finished {
    pub session: String,
    pub block: String,
    pub probe: Probe,
    /// The block's stamp for this probe when it was sent.
    pub stamp: u64,
    /// The response's payload; none on an error or a timeout. A response with neither a
    /// payload nor an error is a failed read too.
    pub reply: Option<Value>,
}

/// What became of a finished probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    /// A status read outdated by a live event; it will be asked again.
    Stale,
    /// Failed, outdated by a live event that carried the whole value, or its session or block
    /// is gone; it won't be asked again on this connection.
    Dropped,
}

#[derive(Deserialize)]
struct Response {
    payload: Option<Value>,
    error: Option<Value>,
}

impl Probes {
    /// A new connection starts over; a late answer for the old one is ignored.
    pub fn reset(&mut self) {
        self.started.clear();
        self.in_flight = None;
    }

    pub fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    fn started(&self, block: &str, probe: Probe) -> bool {
        self.started
            .get(block)
            .is_some_and(|asked| asked[probe as usize])
    }

    fn mark(&mut self, block: &str, probe: Probe) {
        match self.started.get_mut(block) {
            Some(asked) => asked[probe as usize] = true,
            None => {
                let mut asked = [false; 4];
                asked[probe as usize] = true;
                self.started.insert(block.to_owned(), asked);
            }
        }
    }

    /// The next probe to send, as (session, block, probe): none while one is in flight. Every
    /// block's `Status` comes before any other probe; within each pass the selected session
    /// comes first, then the others in list order. A session whose view isn't known yet waits,
    /// and a selected session that isn't listed is skipped. Blocks `streamed` reports skip
    /// `Process`: their streams report it anyway. Allocates only for the probe it returns.
    pub fn next(
        &self,
        model: &HostModel,
        streamed: impl Fn(&str) -> bool,
    ) -> Option<(String, String, Probe)> {
        if self.in_flight.is_some() {
            return None;
        }
        let selected = model
            .selected
            .as_deref()
            .filter(|id| model.sessions.iter().any(|s| s.session_id == *id));
        let sessions = || {
            let others = model
                .sessions
                .iter()
                .map(|s| s.session_id.as_str())
                .filter(move |id| Some(*id) != selected);
            selected.into_iter().chain(others)
        };
        const PASSES: [&[Probe]; 2] = [
            &[Probe::Status],
            &[Probe::Title, Probe::Process, Probe::Pwd],
        ];
        PASSES.into_iter().find_map(|pass| {
            sessions().find_map(|session| {
                view_blocks(model.views.get(session)?).find_map(|block| {
                    let probe = pass
                        .iter()
                        .copied()
                        .filter(|p| *p != Probe::Process || !streamed(block))
                        .find(|p| !self.started(block, *p))?;
                    Some((session.to_owned(), block.to_owned(), probe))
                })
            })
        })
    }

    /// Send a probe without waiting for it; `stamp` is the block's stamp for it now. Only a
    /// failed send is an error.
    pub async fn start(
        &mut self,
        control: &mut Client,
        session: String,
        block: String,
        probe: Probe,
        stamp: u64,
    ) -> rexrs::Result<()> {
        let reply = control
            .transport()
            .detach(
                &probe.method(),
                json!({"session_id": session, "block_id": block, "args": {}}),
            )
            .await?;
        self.mark(&block, probe);
        self.in_flight = Some(InFlight {
            session,
            block,
            probe,
            stamp,
            reply,
            deadline: Instant::now() + TIMEOUT,
        });
        Ok(())
    }

    /// Wait for the probe in flight to answer or time out. Cancel-safe: dropping this leaves
    /// the probe in flight.
    pub async fn finished(&mut self) -> Finished {
        let Some(f) = &mut self.in_flight else {
            return std::future::pending().await;
        };
        let deadline = tokio::time::Instant::from_std(f.deadline);
        let text = tokio::time::timeout_at(deadline, &mut f.reply)
            .await
            .ok()
            .and_then(Result::ok);
        let f = self.in_flight.take().expect("a probe in flight");
        let reply = text
            .and_then(|t| serde_json::from_str::<Response>(&t).ok())
            .filter(|r| r.error.is_none())
            .and_then(|r| r.payload);
        Finished {
            session: f.session,
            block: f.block,
            probe: f.probe,
            stamp: f.stamp,
            reply,
        }
    }

    /// Apply a finished probe to the model. It is dropped for good when it failed or its
    /// session or block is gone. When a live event of its kind arrived after it was sent, the
    /// event is newer than the answer, so the answer is dropped. A status read is asked again,
    /// since one record's event doesn't stand in for a snapshot of all of them; a title,
    /// process or directory event carries the whole value, so re-reading it would gain nothing.
    pub fn apply(&mut self, model: &mut HostModel, done: &Finished) -> Outcome {
        let Some(reply) = &done.reply else {
            return Outcome::Dropped;
        };
        let present = model.sessions.iter().any(|s| s.session_id == done.session)
            && model
                .views
                .get(&done.session)
                .is_some_and(|view| view_blocks(view).any(|b| b == done.block));
        if !present {
            return Outcome::Dropped;
        }
        if model.stamp(&done.block, done.probe) != done.stamp {
            if done.probe != Probe::Status {
                return Outcome::Dropped;
            }
            if let Some(asked) = self.started.get_mut(&done.block) {
                asked[Probe::Status as usize] = false;
            }
            return Outcome::Stale;
        }
        let block = done.block.clone();
        let event = match done.probe {
            Probe::Status => {
                let records = activity::parse_status(reply);
                if records.is_empty() {
                    model.activity.remove(&block);
                } else {
                    model.activity.insert(block, records.into_iter().collect());
                }
                return Outcome::Applied;
            }
            Probe::Title => activity::parse_title(reply),
            Probe::Process => activity::parse_process(reply),
            Probe::Pwd => activity::parse_pwd(reply),
        };
        model.record_block_event(&done.session, block, event);
        Outcome::Applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::activity::{BlockEvent, ProgramState, StatusRecord, TERMINAL};
    use rexrs::api::session;
    use serde_json::json;
    use std::collections::HashSet;

    /// Sessions in `model.sessions` order, each with the terminal blocks its one window lists.
    fn model(selected: &str, sessions: &[(&str, &[&str])]) -> HostModel {
        let mut model = HostModel {
            selected: Some(selected.into()),
            ..Default::default()
        };
        for (id, blocks) in sessions {
            model.sessions.push(session::Session {
                session_id: (*id).into(),
                label: (*id).into(),
            });
            model.views.insert((*id).into(), view(blocks));
        }
        model
    }

    fn view(blocks: &[&str]) -> session::ViewResponse {
        let blocks: Vec<_> = blocks
            .iter()
            .map(|b| json!({"block_id":b,"label":b,"creator_name":TERMINAL}))
            .collect();
        serde_json::from_value(json!({"revision":1,"windows":[{"window_id":"w",
            "layers":[{"kind":"tiled","blocks":blocks}]}]}))
        .unwrap()
    }

    /// The next probe, marked started as `start` would.
    fn take(probes: &mut Probes, model: &HostModel, streamed: &HashSet<String>) -> (String, Probe) {
        let (_, block, probe) = next(probes, model, streamed).expect("a probe due");
        probes.mark(&block, probe);
        (block, probe)
    }

    fn next(
        probes: &Probes,
        model: &HostModel,
        streamed: &HashSet<String>,
    ) -> Option<(String, String, Probe)> {
        probes.next(model, |b| streamed.contains(b))
    }

    fn in_flight(probes: &mut Probes, session: &str, block: &str, probe: Probe) {
        probes.mark(block, probe);
        probes.in_flight = Some(InFlight {
            session: session.into(),
            block: block.into(),
            probe,
            stamp: 0,
            reply: oneshot::channel().1,
            deadline: Instant::now() + TIMEOUT,
        });
    }

    fn sequence(block: &str, probes: &[Probe]) -> Vec<(String, Probe)> {
        probes.iter().map(|p| (block.to_owned(), *p)).collect()
    }

    #[test]
    fn every_status_comes_first_and_the_selected_session_leads() {
        let model = model("s1", &[("s2", &["b2", "b3"]), ("s1", &["b1"])]);
        let mut probes = Probes::default();
        let none = HashSet::new();
        let order: Vec<_> = (0..12).map(|_| take(&mut probes, &model, &none)).collect();
        let mut expected = vec![
            ("b1".to_owned(), Probe::Status),
            ("b2".to_owned(), Probe::Status),
            ("b3".to_owned(), Probe::Status),
        ];
        let rest = [Probe::Title, Probe::Process, Probe::Pwd];
        expected.extend(sequence("b1", &rest));
        expected.extend(sequence("b2", &rest));
        expected.extend(sequence("b3", &rest));
        assert_eq!(order, expected);
        assert_eq!(next(&probes, &model, &none), None, "each probe runs once");
    }

    #[test]
    fn switching_sessions_never_repeats_a_probe() {
        let mut model = model("s1", &[("s1", &["b1"]), ("s2", &["b2"])]);
        let mut probes = Probes::default();
        probes.mark("b1", Probe::Status);
        probes.mark("b2", Probe::Status);
        model.selected = Some("s2".into());
        assert_eq!(
            next(&probes, &model, &HashSet::new()),
            Some(("s2".into(), "b2".into(), Probe::Title))
        );
    }

    #[test]
    fn a_selected_session_that_is_not_listed_is_not_probed() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        model.views.insert("gone".into(), view(&["b9"]));
        model.selected = Some("gone".into());
        assert_eq!(
            next(&Probes::default(), &model, &HashSet::new()),
            Some(("s1".into(), "b1".into(), Probe::Status))
        );
    }

    #[test]
    fn streamed_blocks_skip_the_process_probe() {
        let model = model("s1", &[("s1", &["b1"])]);
        let mut probes = Probes::default();
        let streamed = HashSet::from(["b1".to_owned()]);
        let order: Vec<_> = (0..3)
            .map(|_| take(&mut probes, &model, &streamed))
            .collect();
        assert_eq!(
            order,
            sequence("b1", &[Probe::Status, Probe::Title, Probe::Pwd])
        );
        assert_eq!(next(&probes, &model, &streamed), None);
    }

    #[test]
    fn only_one_probe_is_in_flight() {
        let model = model("s1", &[("s1", &["b1", "b2"])]);
        let mut probes = Probes::default();
        in_flight(&mut probes, "s1", "b1", Probe::Status);
        assert!(probes.in_flight());
        assert_eq!(next(&probes, &model, &HashSet::new()), None);
    }

    fn status(stamp: u64, state: &str) -> Finished {
        Finished {
            session: "s1".into(),
            block: "b1".into(),
            probe: Probe::Status,
            stamp,
            reply: Some(json!({"records":[{"state":state}]})),
        }
    }

    #[test]
    fn a_reply_applies_without_counting_as_a_live_event() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        assert_eq!(
            Probes::default().apply(&mut model, &status(0, "blocked")),
            Outcome::Applied
        );
        assert_eq!(model.activity["b1"][""].state, ProgramState::Blocked);
        let title = Finished {
            probe: Probe::Title,
            reply: Some(json!({"title":"build"})),
            ..status(0, "")
        };
        assert_eq!(
            Probes::default().apply(&mut model, &title),
            Outcome::Applied
        );
        assert_eq!(model.title("b1"), Some("build"));
        let pwd = Finished {
            probe: Probe::Pwd,
            reply: Some(json!({"pwd":"file://h/srv"})),
            ..status(0, "")
        };
        assert_eq!(Probes::default().apply(&mut model, &pwd), Outcome::Applied);
        assert_eq!(model.cwds["b1"], "/srv");
        for probe in Probe::ALL {
            assert_eq!(model.stamp("b1", probe), 0, "{probe:?}");
        }
        let empty = Finished {
            reply: Some(json!({"records":[]})),
            ..status(0, "")
        };
        assert_eq!(
            Probes::default().apply(&mut model, &empty),
            Outcome::Applied
        );
        assert!(!model.activity.contains_key("b1"), "no records, no entry");
        let failed = Finished {
            reply: None,
            ..status(0, "")
        };
        assert_eq!(
            Probes::default().apply(&mut model, &failed),
            Outcome::Dropped
        );
    }

    #[test]
    fn a_snapshot_older_than_a_live_event_is_dropped() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        let stamp = model.stamp("b1", Probe::Status);
        assert_eq!(stamp, 0);
        let record = StatusRecord {
            state: ProgramState::Working,
            live: true,
            ..Default::default()
        };
        model.apply_block_event(
            "s1",
            "b1".into(),
            BlockEvent::StatusChanged {
                id: String::new(),
                record: record.clone(),
            },
        );
        assert_eq!(model.stamp("b1", Probe::Status), 1);
        assert_eq!(
            Probes::default().apply(&mut model, &status(stamp, "blocked")),
            Outcome::Stale
        );
        assert_eq!(model.activity["b1"][""], record);
    }

    #[test]
    fn a_snapshot_outdated_by_one_record_is_asked_again() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        let mut probes = Probes::default();
        let none = HashSet::new();
        assert_eq!(
            take(&mut probes, &model, &none),
            ("b1".into(), Probe::Status)
        );
        let stamp = model.stamp("b1", Probe::Status);
        model.apply_block_event(
            "s1",
            "b1".into(),
            BlockEvent::StatusChanged {
                id: "build".into(),
                record: StatusRecord {
                    state: ProgramState::Working,
                    live: true,
                    ..Default::default()
                },
            },
        );
        let both = |stamp| Finished {
            reply: Some(json!({"records":[{"id":"","state":"blocked"},
                {"id":"build","state":"working"}]})),
            ..status(stamp, "")
        };
        assert_eq!(probes.apply(&mut model, &both(stamp)), Outcome::Stale);
        assert!(!model.activity["b1"].contains_key(""), "not applied yet");
        let again = next(&probes, &model, &none);
        assert_eq!(again, Some(("s1".into(), "b1".into(), Probe::Status)));
        take(&mut probes, &model, &none);
        let stamp = model.stamp("b1", Probe::Status);
        assert_eq!(probes.apply(&mut model, &both(stamp)), Outcome::Applied);
        assert_eq!(model.activity["b1"][""].state, ProgramState::Blocked);
        assert_eq!(model.activity["b1"]["build"].state, ProgramState::Working);
    }

    #[test]
    fn a_snapshot_after_a_live_removal_is_dropped() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        let stamp = model.stamp("b1", Probe::Status);
        model.apply_block_event(
            "s1",
            "b1".into(),
            BlockEvent::StatusRemoved {
                ids: vec![String::new()],
            },
        );
        assert_eq!(
            Probes::default().apply(&mut model, &status(stamp, "blocked")),
            Outcome::Stale
        );
        assert!(!model.activity.contains_key("b1"));
    }

    #[test]
    fn a_snapshot_for_a_closed_block_is_dropped() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        model.views.insert("s1".into(), view(&[]));
        assert_eq!(
            Probes::default().apply(&mut model, &status(0, "blocked")),
            Outcome::Dropped
        );
        assert!(!model.activity.contains_key("b1"));
        model.views.insert("s1".into(), view(&["b1"]));
        model.sessions.clear();
        assert_eq!(
            Probes::default().apply(&mut model, &status(0, "blocked")),
            Outcome::Dropped,
            "the session is gone"
        );
        assert!(!model.activity.contains_key("b1"));
    }

    #[test]
    fn reset_probes_again() {
        let model = model("s1", &[("s1", &["b1"])]);
        let mut probes = Probes::default();
        let none = HashSet::new();
        for _ in 0..3 {
            take(&mut probes, &model, &none);
        }
        in_flight(&mut probes, "s1", "b1", Probe::Pwd);
        probes.reset();
        assert!(!probes.in_flight());
        assert_eq!(
            next(&probes, &model, &none),
            Some(("s1".into(), "b1".into(), Probe::Status))
        );
    }

    #[test]
    fn an_outdated_title_process_or_directory_is_dropped_for_good() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        let mut probes = Probes::default();
        let none = HashSet::new();
        for _ in Probe::ALL {
            take(&mut probes, &model, &none);
        }
        let events = [
            (
                Probe::Title,
                BlockEvent::Title {
                    text: "live".into(),
                    owner: None,
                },
                json!({"title":"old"}),
            ),
            (
                Probe::Process,
                BlockEvent::Process {
                    name: Some("live".into()),
                    pids: vec![],
                    place: Default::default(),
                },
                json!({"foreground":{"pid":1,"name":"old"}}),
            ),
            (
                Probe::Pwd,
                BlockEvent::Pwd(Some("/live".into())),
                json!({"pwd":"/old"}),
            ),
        ];
        for (probe, event, reply) in events {
            let stamp = model.stamp("b1", probe);
            model.apply_block_event("s1", "b1".into(), event);
            let done = Finished {
                probe,
                reply: Some(reply),
                ..status(stamp, "")
            };
            assert_eq!(
                probes.apply(&mut model, &done),
                Outcome::Dropped,
                "{probe:?}"
            );
        }
        assert_eq!(model.title("b1"), Some("live"));
        assert_eq!(model.cwds["b1"], "/live");
        assert_eq!(
            next(&probes, &model, &none),
            None,
            "the live events already carried the values"
        );
    }

    #[test]
    fn a_probe_wakes_the_ui_only_when_it_changed_something() {
        let (wake, woken) = flume::unbounded();
        let shared = crate::connection::Shared {
            model: std::sync::Arc::new(std::sync::Mutex::new(model("s1", &[("s1", &["b1"])]))),
            wake,
        };
        let mut probes = Probes::default();
        let failed = Finished {
            reply: None,
            ..status(0, "")
        };
        assert_eq!(
            crate::connection::apply_probe(&shared, &mut probes, &failed),
            Outcome::Dropped
        );
        assert!(woken.try_recv().is_err(), "a failed read changes nothing");
        assert_eq!(
            crate::connection::apply_probe(&shared, &mut probes, &status(0, "blocked")),
            Outcome::Applied
        );
        assert!(woken.try_recv().is_ok());
    }

    #[tokio::test]
    async fn a_reply_is_its_payload_and_an_error_is_nothing() {
        let mut probes = Probes::default();
        for (text, reply) in [
            (
                r#"{"type":"response","id":"x","seq":2,"payload":{"pwd":"/"}}"#,
                Some(json!({"pwd":"/"})),
            ),
            (
                r#"{"type":"response","id":"x","error":{"code":"block_not_found"}}"#,
                None,
            ),
            (r#"{"type":"response","id":"x"}"#, None),
            ("not json", None),
        ] {
            in_flight(&mut probes, "s1", "b1", Probe::Pwd);
            let (send, receive) = oneshot::channel();
            probes.in_flight.as_mut().unwrap().reply = receive;
            send.send(text.into()).unwrap();
            let done = probes.finished().await;
            assert_eq!((done.block.as_str(), done.probe), ("b1", Probe::Pwd));
            assert_eq!(done.reply, reply, "{text}");
            assert!(!probes.in_flight());
        }
    }
}
