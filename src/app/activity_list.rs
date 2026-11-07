//! The Activity view's model: one row per pane that has something to say, across hosts,
//! ranked by how much it needs the user, with the subtasks its program reports.
// Nothing renders these rows yet; the sidebar and session list take them next.
#![allow(dead_code)]
use super::{
    activity::{self, TabActivity},
    workspace::HostId,
};
use rex_linux::connection::{
    HostModel,
    activity::{ProgramState, StatusRecord, TERMINAL as TERMINAL_CREATOR},
};
use rex_linux::settings::ActivityGrouping;
use std::collections::HashSet;

/// Where a row leads: the pane to focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneRef {
    pub host: HostId,
    pub session: String,
    pub window: String,
    pub block: String,
}

/// One subtask a program reports, from a record whose ID is not empty.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskLine {
    pub id: String,
    pub depth: usize,
    pub title: String,
    pub record: StatusRecord,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivityRow {
    pub target: PaneRef,
    pub name: String,
    /// The program's identity, shown first in `detail` only beside a label the user chose.
    pub program: Option<String>,
    pub activity: TabActivity,
    pub rank: u8,
    /// "[program · ]message · location · directory", empty parts dropped.
    pub detail: String,
    pub tasks: Vec<TaskLine>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivityGroup {
    pub title: String,
    pub rows: Vec<ActivityRow>,
}

/// How urgent a pane is, lowest first, or None when it gets no row: Blocked 0, Error 1,
/// unseen Done 2, Working or progress alone 3, seen Done 4, Idle agents 5. A pane without
/// records and progress, a plain shell, never has a row.
pub fn activity_rank(a: &TabActivity, has_records: bool, unseen_done: bool) -> Option<u8> {
    match a.state {
        ProgramState::Blocked => Some(0),
        ProgramState::Error => Some(1),
        ProgramState::Done if unseen_done => Some(2),
        ProgramState::Done => Some(4),
        ProgramState::Working => Some(3),
        ProgramState::Idle if a.progress.is_some() => Some(3),
        ProgramState::Idle if has_records => Some(5),
        ProgramState::Idle => None,
    }
}

/// Each path's last component, with as many more as it takes to tell it from a different
/// path that ends the same way.
pub fn short_dirs(paths: &[&str]) -> Vec<String> {
    let parts: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| p.split('/').filter(|c| !c.is_empty()).collect())
        .collect();
    let tail = |p: &[&str], n: usize| p[p.len().saturating_sub(n)..].join("/");
    paths
        .iter()
        .zip(&parts)
        .map(|(path, mine)| {
            if mine.is_empty() {
                return "/".to_owned();
            }
            let mut n = 1;
            while n < mine.len()
                && paths
                    .iter()
                    .zip(&parts)
                    .any(|(other, p)| other != path && tail(p, n) == tail(mine, n))
            {
                n += 1;
            }
            tail(mine, n)
        })
        .collect()
}

fn priority_title(rank: u8) -> &'static str {
    match rank {
        0 | 1 => "Needs you",
        2 => "Done",
        3 => "Working",
        4 => "Finished",
        _ => "Idle",
    }
}

struct Draft {
    row: ActivityRow,
    cwd: Option<String>,
    host: usize,
    session: usize,
    group: String,
}

/// Rows for every connected host's panes, grouped by urgency or by session. `hosts` is in
/// host order; `unseen` is the workspace's unseen Done tabs.
pub fn activity_groups(
    hosts: &[(HostId, &str, &HostModel)],
    unseen: &HashSet<(HostId, String)>,
    grouping: ActivityGrouping,
    show_idle: bool,
) -> Vec<ActivityGroup> {
    let mut drafts = Vec::new();
    for (host, &(id, host_label, model)) in hosts.iter().enumerate() {
        if !model.connected {
            continue;
        }
        for (session, info) in model.sessions.iter().enumerate() {
            let Some(view) = model.views.get(&info.session_id) else {
                continue;
            };
            for window in &view.windows {
                let Some(window_id) = window.window_id.as_deref() else {
                    continue;
                };
                let tab = activity::tab_name(model, window);
                let done_unseen = unseen.contains(&(id, window_id.to_owned()));
                for block in window.layers.iter().flat_map(|l| &l.blocks) {
                    let Some(block_id) = block.block_id.as_deref() else {
                        continue;
                    };
                    if block.creator_name.as_deref() != Some(TERMINAL_CREATOR) {
                        continue;
                    }
                    let records = model.activity.get(block_id).filter(|r| !r.is_empty());
                    let a = activity::pane_activity(model, block_id);
                    let Some(rank) = activity_rank(&a, records.is_some(), done_unseen) else {
                        continue;
                    };
                    if rank == 5 && !show_idle {
                        continue;
                    }
                    let program = (!activity::label_is_default(block))
                        .then(|| {
                            a.source
                                .clone()
                                .or_else(|| model.title(block_id).map(str::to_owned))
                        })
                        .flatten();
                    let location = match grouping {
                        ActivityGrouping::Priority => {
                            format!("{tab} · {} · {host_label}", info.label)
                        }
                        ActivityGrouping::Session => tab.clone(),
                    };
                    let detail = join([program.as_deref(), a.msg.as_deref(), Some(&location)]);
                    let tasks = records
                        .into_iter()
                        .flatten()
                        .filter(|(task, _)| !task.is_empty())
                        .map(|(task, record)| TaskLine {
                            id: task.clone(),
                            depth: task.matches('/').count(),
                            title: record
                                .title
                                .clone()
                                .unwrap_or_else(|| task.rsplit('/').next().unwrap_or(task).into()),
                            record: record.clone(),
                        })
                        .collect();
                    drafts.push(Draft {
                        row: ActivityRow {
                            target: PaneRef {
                                host: id,
                                session: info.session_id.clone(),
                                window: window_id.to_owned(),
                                block: block_id.to_owned(),
                            },
                            name: activity::pane_name(model, block),
                            program,
                            activity: a,
                            rank,
                            detail,
                            tasks,
                        },
                        cwd: model.cwds.get(block_id).filter(|c| !c.is_empty()).cloned(),
                        host,
                        session,
                        group: format!("{} · {host_label}", info.label),
                    });
                }
            }
        }
    }
    let with_cwd: Vec<&str> = drafts.iter().filter_map(|d| d.cwd.as_deref()).collect();
    let mut short = short_dirs(&with_cwd).into_iter();
    for draft in &mut drafts {
        if draft.cwd.is_some() {
            let dir = short.next().unwrap_or_default();
            draft.row.detail = join([Some(draft.row.detail.as_str()), Some(dir.as_str())]);
        }
    }
    let mut groups: Vec<(u8, usize, usize, ActivityGroup)> = Vec::new();
    // The sort is stable, so ties keep host, session, tab and layout order.
    drafts.sort_by_key(|d| match grouping {
        ActivityGrouping::Priority => d.row.rank,
        ActivityGrouping::Session => 0,
    });
    for d in drafts {
        let title = match grouping {
            ActivityGrouping::Priority => priority_title(d.row.rank).to_owned(),
            ActivityGrouping::Session => d.group.clone(),
        };
        let at = groups
            .iter()
            .position(|(_, h, s, g)| g.title == title && (*h, *s) == (d.host, d.session))
            .or_else(|| {
                (grouping == ActivityGrouping::Priority)
                    .then(|| groups.iter().position(|(.., g)| g.title == title))
                    .flatten()
            });
        match at {
            Some(i) => {
                groups[i].0 = groups[i].0.min(d.row.rank);
                groups[i].3.rows.push(d.row);
            }
            None => groups.push((
                d.row.rank,
                d.host,
                d.session,
                ActivityGroup {
                    title,
                    rows: vec![d.row],
                },
            )),
        }
    }
    if grouping == ActivityGrouping::Session {
        groups.sort_by_key(|(rank, host, session, _)| (*rank, *host, *session));
    }
    groups.into_iter().map(|(.., g)| g).collect()
}

fn join<'a>(parts: impl IntoIterator<Item = Option<&'a str>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A session's activity across all its tabs, and how many panes need the user (Blocked or
/// Error). None until its view is known.
pub fn session_activity(model: &HostModel, session: &str) -> Option<(TabActivity, usize)> {
    let view = model.views.get(session)?;
    let needing = view
        .windows
        .iter()
        .flat_map(|w| &w.layers)
        .flat_map(|l| &l.blocks)
        .filter_map(|b| b.block_id.as_deref())
        .filter(|b| {
            matches!(
                activity::pane_activity(model, b).state,
                ProgramState::Blocked | ProgramState::Error
            )
        })
        .count();
    Some((activity::view_activity(model, view), needing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::activity::activity_word;
    use rex_linux::connection::activity::{BlockedKind, TerminalProgress};
    use rexrs::api::session;
    use std::collections::BTreeMap;

    fn record(state: ProgramState) -> StatusRecord {
        StatusRecord {
            state,
            ..Default::default()
        }
    }

    fn block(id: &str) -> session::LayerBlock {
        session::LayerBlock {
            block_id: Some(id.into()),
            creator_name: Some(TERMINAL_CREATOR.into()),
            flavor: Some(format!("{TERMINAL_CREATOR}.shell")),
            ..Default::default()
        }
    }

    fn layer(kind: &str, blocks: Vec<session::LayerBlock>) -> session::Layer {
        session::Layer {
            kind: kind.into(),
            blocks,
            ..Default::default()
        }
    }

    fn window(id: &str, layers: Vec<session::Layer>) -> session::Window {
        session::Window {
            window_id: Some(id.into()),
            label: Some(id.into()),
            layers,
            ..Default::default()
        }
    }

    /// A connected host with one session `session` holding `windows`.
    fn host(session: &str, windows: Vec<session::Window>) -> HostModel {
        let mut model = HostModel {
            connected: true,
            ..Default::default()
        };
        add_session(&mut model, session, windows);
        model
    }

    fn add_session(model: &mut HostModel, session: &str, windows: Vec<session::Window>) {
        model.sessions.push(session::Session {
            session_id: session.into(),
            label: session.into(),
        });
        model.views.insert(
            session.into(),
            session::ViewResponse {
                windows,
                ..Default::default()
            },
        );
    }

    fn tab(id: &str, blocks: &[&str]) -> session::Window {
        window(
            id,
            vec![layer("tiled", blocks.iter().map(|b| block(b)).collect())],
        )
    }

    fn report(model: &mut HostModel, block: &str, state: ProgramState) {
        model
            .activity
            .entry(block.into())
            .or_default()
            .insert(String::new(), record(state));
    }

    fn groups(
        hosts: &[(&str, &HostModel)],
        unseen: &[(u64, &str)],
        grouping: ActivityGrouping,
        show_idle: bool,
    ) -> Vec<ActivityGroup> {
        let hosts: Vec<_> = hosts
            .iter()
            .enumerate()
            .map(|(i, (label, m))| (HostId(i as u64), *label, *m))
            .collect();
        let unseen = unseen
            .iter()
            .map(|(h, w)| (HostId(*h), w.to_string()))
            .collect();
        activity_groups(&hosts, &unseen, grouping, show_idle)
    }

    fn names(groups: &[ActivityGroup]) -> Vec<(&str, Vec<&str>)> {
        groups
            .iter()
            .map(|g| {
                (
                    g.title.as_str(),
                    g.rows.iter().map(|r| r.name.as_str()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn rows_follow_priority_then_layout_order() {
        let mut a = host("dev", vec![tab("t1", &["a1", "a2"]), tab("t2", &["a3"])]);
        let mut b = host("ops", vec![tab("t3", &["b1", "b2"])]);
        let label = |m: &mut HostModel, id: &str, name: &str, state| {
            report(m, id, state);
            for blk in m
                .views
                .values_mut()
                .flat_map(|v| &mut v.windows)
                .flat_map(|w| &mut w.layers)
                .flat_map(|l| &mut l.blocks)
                .filter(|blk| blk.block_id.as_deref() == Some(id))
            {
                blk.label = Some(name.into());
            }
        };
        label(&mut a, "a1", "work1", ProgramState::Working);
        label(&mut a, "a2", "block1", ProgramState::Blocked);
        label(&mut a, "a3", "work2", ProgramState::Working);
        label(&mut b, "b1", "err", ProgramState::Error);
        label(&mut b, "b2", "block2", ProgramState::Blocked);
        let g = groups(
            &[("laptop", &a), ("server", &b)],
            &[],
            ActivityGrouping::Priority,
            false,
        );
        assert_eq!(
            names(&g),
            [
                ("Needs you", vec!["block1", "block2", "err"]),
                ("Working", vec!["work1", "work2"]),
            ]
        );
        assert_eq!(
            g[0].rows.iter().map(|r| r.rank).collect::<Vec<_>>(),
            [0, 0, 1]
        );
    }

    #[test]
    fn quiet_shells_and_idle_agents_stay_out_unless_asked() {
        let mut m = host("dev", vec![tab("t", &["shell", "agent"])]);
        report(&mut m, "agent", ProgramState::Idle);
        for show in [false, true] {
            let g = groups(&[("h", &m)], &[], ActivityGrouping::Priority, show);
            let rows: usize = g.iter().map(|g| g.rows.len()).sum();
            assert_eq!(rows, usize::from(show));
            if show {
                assert_eq!(g[0].title, "Idle");
                assert_eq!(g[0].rows[0].target.block, "agent");
            }
        }
    }

    #[test]
    fn seen_done_drops_below_working() {
        let mut m = host("dev", vec![tab("t1", &["done"]), tab("t2", &["busy"])]);
        report(&mut m, "done", ProgramState::Done);
        report(&mut m, "busy", ProgramState::Working);
        let unseen = groups(
            &[("h", &m)],
            &[(0, "t1")],
            ActivityGrouping::Priority,
            false,
        );
        assert_eq!(
            unseen.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(),
            ["Done", "Working"]
        );
        let seen = groups(&[("h", &m)], &[], ActivityGrouping::Priority, false);
        assert_eq!(
            seen.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(),
            ["Working", "Finished"]
        );
        assert_eq!(seen[1].rows[0].rank, 4);
    }

    #[test]
    fn rows_name_working_and_idle() {
        let word = |m: &HostModel| {
            activity_word(&activity::pane_activity(m, "b"))
                .0
                .into_owned()
        };
        let mut m = HostModel::default();
        report(&mut m, "b", ProgramState::Working);
        assert_eq!(word(&m), "Working");
        m.activity
            .get_mut("b")
            .unwrap()
            .get_mut("")
            .unwrap()
            .progress = Some(40);
        assert_eq!(word(&m), "40%");
        report(&mut m, "b", ProgramState::Idle);
        assert_eq!(word(&m), "Idle");
        let mut blocked = record(ProgramState::Blocked);
        blocked.kind = Some(BlockedKind::Permission);
        m.activity
            .insert("b".into(), BTreeMap::from([(String::new(), blocked)]));
        assert_eq!(word(&m), "Needs approval");
    }

    #[test]
    fn progress_alone_makes_a_working_row() {
        let mut m = host("dev", vec![tab("t", &["b"])]);
        m.progress
            .insert("b".into(), TerminalProgress::Set(Some(30)));
        let g = groups(&[("h", &m)], &[], ActivityGrouping::Priority, false);
        assert_eq!(g[0].title, "Working");
        assert_eq!(g[0].rows[0].rank, 3);
        assert_eq!(activity_word(&g[0].rows[0].activity).0, "30%");
    }

    #[test]
    fn grouping_by_session_names_session_and_host() {
        let mut laptop = host("dev", vec![tab("t1", &["a"])]);
        report(&mut laptop, "a", ProgramState::Working);
        let mut server = host("ops", vec![tab("t2", &["b"])]);
        report(&mut server, "b", ProgramState::Blocked);
        let g = groups(
            &[("laptop", &laptop), ("server", &server)],
            &[],
            ActivityGrouping::Session,
            false,
        );
        assert_eq!(
            g.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(),
            ["ops · server", "dev · laptop"]
        );
        assert_eq!(g[0].rows[0].detail, "t2");
    }

    #[test]
    fn subtasks_nest_under_their_pane() {
        let mut m = host("dev", vec![tab("t", &["b"])]);
        let mut sub = record(ProgramState::Working);
        sub.title = Some("Build".into());
        m.activity.insert(
            "b".into(),
            BTreeMap::from([
                (String::new(), record(ProgramState::Working)),
                ("build".into(), sub),
                ("build/test".into(), record(ProgramState::Working)),
            ]),
        );
        let g = groups(&[("h", &m)], &[], ActivityGrouping::Priority, false);
        assert_eq!(g[0].rows.len(), 1);
        let tasks: Vec<_> = g[0].rows[0]
            .tasks
            .iter()
            .map(|t| (t.id.as_str(), t.depth, t.title.as_str()))
            .collect();
        assert_eq!(tasks, [("build", 0, "Build"), ("build/test", 1, "test")]);
    }

    #[test]
    fn user_labels_keep_the_program_beside_them() {
        let mut m = host("dev", vec![tab("t", &["named", "plain"])]);
        let blocks = &mut m.views.get_mut("dev").unwrap().windows[0].layers[0].blocks;
        blocks[0].label = Some("api".into());
        let mut r = record(ProgramState::Blocked);
        r.app = Some("claude-code".into());
        r.msg = Some("Run tests?".into());
        m.activity
            .insert("named".into(), BTreeMap::from([(String::new(), r)]));
        report(&mut m, "plain", ProgramState::Working);
        m.titles
            .insert("plain".into(), ("Claude Code".into(), None));
        let g = groups(&[("h", &m)], &[], ActivityGrouping::Priority, false);
        let named = &g[0].rows[0];
        assert_eq!(named.name, "api");
        assert_eq!(named.program.as_deref(), Some("claude-code"));
        assert!(
            named.detail.starts_with("claude-code · Run tests?"),
            "{}",
            named.detail
        );
        let plain = &g[1].rows[0];
        assert_eq!(plain.name, "Claude Code");
        assert_eq!(plain.program, None);
        assert!(plain.detail.starts_with("t · dev · h"), "{}", plain.detail);
    }

    #[test]
    fn floating_panes_have_rows_and_disconnected_hosts_none() {
        let win = window(
            "t",
            vec![
                layer("tiled", vec![block("tiled")]),
                layer("floating", vec![block("float")]),
            ],
        );
        let mut m = host("dev", vec![win]);
        report(&mut m, "float", ProgramState::Error);
        report(&mut m, "detached", ProgramState::Error);
        let g = groups(&[("h", &m)], &[], ActivityGrouping::Priority, false);
        assert_eq!(names(&g).len(), 1);
        assert_eq!(g[0].rows.len(), 1);
        assert_eq!(g[0].rows[0].target.block, "float");
        m.connected = false;
        assert!(groups(&[("h", &m)], &[], ActivityGrouping::Priority, false).is_empty());
    }

    #[test]
    fn session_activity_counts_panes_needing_you() {
        let mut m = host("dev", vec![tab("t1", &["a", "b"]), tab("t2", &["c", "d"])]);
        for (b, s) in [
            ("a", ProgramState::Blocked),
            ("b", ProgramState::Blocked),
            ("c", ProgramState::Error),
            ("d", ProgramState::Working),
        ] {
            report(&mut m, b, s);
        }
        let (a, n) = session_activity(&m, "dev").unwrap();
        assert_eq!((a.state, n), (ProgramState::Blocked, 3));
        assert!(session_activity(&m, "other").is_none());
    }

    #[test]
    fn short_dirs_disambiguate_duplicates() {
        assert_eq!(
            short_dirs(&["/a/app", "/b/app", "/c/web"]),
            ["a/app", "b/app", "web"]
        );
        assert_eq!(short_dirs(&["/x"]), ["x"]);
        assert_eq!(short_dirs(&["/"]), ["/"]);
        assert_eq!(short_dirs(&["/a/app", "/a/app"]), ["app", "app"]);
    }
}
