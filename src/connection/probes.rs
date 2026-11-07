//! What each known terminal last reported before this client was listening: its program
//! status, title, foreground process and working directory. Every session whose view is known
//! is read, the selected one first, one request in flight at a time and each (block, probe)
//! once per connection, alongside control traffic rather than in its way. Live events keep the
//! answers current afterwards.
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
    collections::HashSet,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

/// The gap between probes, so reading a large server doesn't crowd out other requests.
pub const PACE: Duration = Duration::from_millis(25);
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct Probes {
    /// Every (block, probe) asked on this connection, answered or not: a failed read isn't
    /// retried until the next connection.
    started: HashSet<(String, Probe)>,
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
    /// The response's payload; none on an error or a timeout.
    pub reply: Option<Value>,
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

    /// The next probe to send, as (session, block, probe): none while one is in flight. The
    /// selected session comes first, then the others in list order; a session whose view isn't
    /// known yet waits. Blocks in `streamed` skip `Process`: their streams report it anyway.
    pub fn next(
        &self,
        model: &HostModel,
        streamed: &HashSet<String>,
    ) -> Option<(String, String, Probe)> {
        if self.in_flight.is_some() {
            return None;
        }
        let selected = model.selected.as_deref();
        let others = model
            .sessions
            .iter()
            .map(|s| s.session_id.as_str())
            .filter(|id| Some(*id) != selected);
        selected.into_iter().chain(others).find_map(|session| {
            view_blocks(model.views.get(session)?).find_map(|block| {
                let probe = Probe::ALL
                    .into_iter()
                    .filter(|p| *p != Probe::Process || !streamed.contains(&block))
                    .find(|p| !self.started.contains(&(block.clone(), *p)))?;
                Some((session.to_owned(), block, probe))
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
        self.started.insert((block.clone(), probe));
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

    /// Apply a finished probe to the model; whether it changed anything. It is dropped when it
    /// failed, its session or block is gone, or a live event of its kind arrived after it was
    /// sent, which is newer than the answer.
    pub fn apply(model: &mut HostModel, done: &Finished) -> bool {
        let Some(reply) = &done.reply else {
            return false;
        };
        let current = model.stamp(&done.block, done.probe) == done.stamp
            && model.sessions.iter().any(|s| s.session_id == done.session)
            && model
                .views
                .get(&done.session)
                .is_some_and(|view| view_blocks(view).any(|b| b == done.block));
        if !current {
            return false;
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
                return true;
            }
            Probe::Title => activity::parse_title(reply),
            Probe::Process => activity::parse_process(reply),
            Probe::Pwd => activity::parse_pwd(reply),
        };
        model.record_block_event(&done.session, block, event);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::activity::{BlockEvent, ProgramState, StatusRecord, TERMINAL};
    use rexrs::api::session;
    use serde_json::json;

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
        let (_, block, probe) = probes.next(model, streamed).expect("a probe due");
        probes.started.insert((block.clone(), probe));
        (block, probe)
    }

    fn in_flight(probes: &mut Probes, session: &str, block: &str, probe: Probe) {
        probes.started.insert((block.into(), probe));
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
    fn the_selected_session_is_probed_first() {
        let model = model("s1", &[("s2", &["b2"]), ("s1", &["b1"])]);
        let mut probes = Probes::default();
        let none = HashSet::new();
        let order: Vec<_> = (0..8).map(|_| take(&mut probes, &model, &none)).collect();
        let mut expected = sequence("b1", &Probe::ALL);
        expected.extend(sequence("b2", &Probe::ALL));
        assert_eq!(order, expected);
        assert_eq!(probes.next(&model, &none), None, "each probe runs once");
    }

    #[test]
    fn switching_sessions_never_repeats_a_probe() {
        let mut model = model("s1", &[("s1", &["b1"]), ("s2", &["b2"])]);
        let mut probes = Probes::default();
        probes.started.insert(("b2".into(), Probe::Status));
        model.selected = Some("s2".into());
        assert_eq!(
            probes.next(&model, &HashSet::new()),
            Some(("s2".into(), "b2".into(), Probe::Title))
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
        assert_eq!(probes.next(&model, &streamed), None);
    }

    #[test]
    fn only_one_probe_is_in_flight() {
        let model = model("s1", &[("s1", &["b1", "b2"])]);
        let mut probes = Probes::default();
        in_flight(&mut probes, "s1", "b1", Probe::Status);
        assert!(probes.in_flight());
        assert_eq!(probes.next(&model, &HashSet::new()), None);
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
        assert!(Probes::apply(&mut model, &status(0, "blocked")));
        assert_eq!(model.activity["b1"][""].state, ProgramState::Blocked);
        let title = Finished {
            probe: Probe::Title,
            reply: Some(json!({"title":"build"})),
            ..status(0, "")
        };
        assert!(Probes::apply(&mut model, &title));
        assert_eq!(model.title("b1"), Some("build"));
        let pwd = Finished {
            probe: Probe::Pwd,
            reply: Some(json!({"pwd":"file://h/srv"})),
            ..status(0, "")
        };
        assert!(Probes::apply(&mut model, &pwd));
        assert_eq!(model.cwds["b1"], "/srv");
        for probe in Probe::ALL {
            assert_eq!(model.stamp("b1", probe), 0, "{probe:?}");
        }
        assert!(Probes::apply(
            &mut model,
            &Finished {
                reply: Some(json!({"records":[]})),
                ..status(0, "")
            }
        ));
        assert!(!model.activity.contains_key("b1"), "no records, no entry");
        let failed = Finished {
            reply: None,
            ..status(0, "")
        };
        assert!(!Probes::apply(&mut model, &failed));
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
        assert!(!Probes::apply(&mut model, &status(stamp, "blocked")));
        assert_eq!(model.activity["b1"][""], record);
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
        assert!(!Probes::apply(&mut model, &status(stamp, "blocked")));
        assert!(!model.activity.contains_key("b1"));
    }

    #[test]
    fn a_snapshot_for_a_closed_block_is_dropped() {
        let mut model = model("s1", &[("s1", &["b1"])]);
        model.views.insert("s1".into(), view(&[]));
        assert!(!Probes::apply(&mut model, &status(0, "blocked")));
        assert!(!model.activity.contains_key("b1"));
        model.views.insert("s1".into(), view(&["b1"]));
        model.sessions.clear();
        assert!(
            !Probes::apply(&mut model, &status(0, "blocked")),
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
            probes.next(&model, &none),
            Some(("s1".into(), "b1".into(), Probe::Status))
        );
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
