use super::*;
use crate::app::{panel::SettingsSection, tailscale::status_words};
use rex_linux::connection::tailscale::{Call, Status};
use serde_json::{Value, json};

fn status(value: Value) -> Status {
    serde_json::from_value(value).unwrap()
}

fn state(state: &str) -> Status {
    status(json!({"state": state, "allow_tailnet_members": false}))
}

/// Settings → Hosts open on `ws`, as a headless runtime.
fn hosts_settings(mut ws: Workspace) -> Runtime<Workspace> {
    ws.perform(Intent::Settings, &mut rune::Tx::default());
    ws.settings_section = SettingsSection::Hosts;
    runtime(ws)
}

fn commands(rx: &flume::Receiver<HostCommand>) -> Vec<HostCommand> {
    rx.try_iter().collect()
}

#[test]
fn status_words_cover_each_state() {
    let words = |value: Value| status_words(&status(value));
    assert_eq!(
        words(json!({"state":"disabled","error":"port busy","allow_tailnet_members":false})),
        ("Off · port busy".to_owned(), true)
    );
    assert_eq!(
        words(json!({"state":"disabled","allow_tailnet_members":false})),
        ("Off".to_owned(), false)
    );
    assert_eq!(
        words(
            json!({"state":"needs_login","auth_url":"https://login","allow_tailnet_members":false})
        ),
        ("Needs sign-in".to_owned(), false)
    );
    assert_eq!(
        words(json!({"state":"needs_machine_approval","allow_tailnet_members":false})),
        (
            "Waiting for a tailnet admin to approve this device".to_owned(),
            true
        )
    );
    assert_eq!(
        words(
            json!({"state":"running","dns_name":"rex-a.t.ts.net","allow_tailnet_members":false,
            "https":{"state":"available","url":"https://rex-a.t.ts.net"}})
        ),
        ("On as rex-a.t.ts.net".to_owned(), false)
    );
    assert_eq!(
        words(json!({"state":"running","allow_tailnet_members":false})),
        ("On".to_owned(), false)
    );
    assert_eq!(
        words(
            json!({"state":"running","dns_name":"rex-a.t.ts.net","allow_tailnet_members":false,
            "https":{"state":"pending"}})
        ),
        ("On as rex-a.t.ts.net · HTTPS starting…".to_owned(), false)
    );
    assert_eq!(
        words(json!({"state":"running","allow_tailnet_members":false,"https":{"state":"warming"}})),
        ("On · HTTPS starting…".to_owned(), false),
        "an unknown HTTPS state is pending"
    );
    assert_eq!(
        words(json!({"state":"starting","allow_tailnet_members":false})),
        ("Starting…".to_owned(), false)
    );
    assert_eq!(
        words(json!({"state":"hibernating","allow_tailnet_members":false})),
        ("Starting…".to_owned(), false)
    );
}

#[test]
fn only_local_hosts_that_can_run_tailscale_get_the_controls() {
    let mut ws = workspace_hosts(2);
    ws.hosts[0].model.tailscale = Some(state("disabled"));
    let mut rt = hosts_settings(ws);
    let keys = |i: usize| {
        [
            format!("tailscale:{i}"),
            format!("tailscale-name:{i}"),
            format!("tailscale-keep:{i}"),
        ]
    };
    for key in keys(0) {
        assert!(has(&rt, &key), "{key}");
    }
    for key in keys(1) {
        assert!(
            !has(&rt, &key),
            "no status means no built-in Tailscale: {key}"
        );
    }
    rt.state.settings.hosts[1].endpoint = "ssh://me@box".into();
    rt.state.hosts[1].model.tailscale = Some(state("disabled"));
    rt.invalidate();
    rt.frame(1. / 60.);
    for key in keys(1) {
        assert!(!has(&rt, &key), "not for ssh:// hosts: {key}");
    }
    std::fs::remove_file(&rt.state.settings_path).ok();
}

#[test]
fn the_switch_enables_under_the_saved_name_and_off_also_stops_keeping() {
    let mut ws = workspace();
    let rx = capture_host(&mut ws, 0);
    ws.hosts[0].model.connected = true;
    ws.hosts[0].model.tailscale = Some(state("disabled"));
    ws.settings.hosts[0].tailscale_name = Some("rex-a".into());
    let mut rt = hosts_settings(ws);
    click(&mut rt, "tailscale:0");
    let sent = commands(&rx);
    assert_eq!(sent.len(), 1);
    assert!(
        matches!(&sent[0], HostCommand::Tailscale(Call::Enable { hostname }) if hostname == "rex-a")
    );

    rt.state.hosts[0].model.tailscale = Some(state("running"));
    rt.state.settings.hosts[0].keep_tailscale = true;
    rt.invalidate();
    rt.frame(1. / 60.);
    click(&mut rt, "tailscale:0");
    let sent = commands(&rx);
    assert_eq!(sent.len(), 2);
    assert!(matches!(&sent[0], HostCommand::KeepTailscale(None)));
    assert!(matches!(&sent[1], HostCommand::Tailscale(Call::Disable)));
    assert!(!Settings::load(&rt.state.settings_path).unwrap().hosts[0].keep_tailscale);
    std::fs::remove_file(&rt.state.settings_path).ok();
}

#[test]
fn keep_and_rename_tell_the_worker() {
    let mut ws = workspace();
    let rx = capture_host(&mut ws, 0);
    ws.hosts[0].model.connected = true;
    ws.hosts[0].model.tailscale = Some(state("disabled"));
    let mut rt = hosts_settings(ws);
    let wanted = rt.state.settings.hosts[0].tailscale_name().unwrap();
    click(&mut rt, "tailscale-keep:0");
    let sent = commands(&rx);
    assert_eq!(sent.len(), 1);
    assert!(matches!(&sent[0], HostCommand::KeepTailscale(Some(name)) if *name == wanted));
    assert!(Settings::load(&rt.state.settings_path).unwrap().hosts[0].keep_tailscale);

    rt.state.set_tailscale_name(0, "rex-b".into());
    let sent = commands(&rx);
    assert_eq!(sent.len(), 1, "no rename while the worker keeps the name");
    assert!(matches!(&sent[0], HostCommand::KeepTailscale(Some(name)) if name == "rex-b"));

    rt.state.toggle_keep_tailscale(0);
    assert!(matches!(
        commands(&rx).as_slice(),
        [HostCommand::KeepTailscale(None)]
    ));
    rt.state.hosts[0].model.tailscale = Some(state("running"));
    rt.state.set_tailscale_name(0, "rex-c".into());
    let sent = commands(&rx);
    assert_eq!(sent.len(), 1);
    assert!(
        matches!(&sent[0], HostCommand::Tailscale(Call::Rename { hostname }) if hostname == "rex-c")
    );
    assert_eq!(
        Settings::load(&rt.state.settings_path).unwrap().hosts[0]
            .tailscale_name
            .as_deref(),
        Some("rex-c")
    );

    rt.invalidate();
    rt.frame(1. / 60.);
    click(&mut rt, "tailscale-name:0");
    let panel = rt.state.panel.as_mut().unwrap();
    assert!(matches!(
        panel.kind,
        PanelKind::Form(Form::TailscaleName(0))
    ));
    assert_eq!(
        panel.inputs[0].text, "rex-c",
        "prefilled with the saved name"
    );
    panel.inputs[0] = rune::TextEdit::new("bad name");
    rt.state.submit(&mut rune::Tx::default());
    let panel = rt.state.panel.as_ref().expect("the form stays open");
    assert!(matches!(
        panel.kind,
        PanelKind::Form(Form::TailscaleName(0))
    ));
    assert_eq!(
        panel.error.as_deref(),
        Some("Use 1–63 letters, digits and hyphens, not starting or ending with a hyphen.")
    );
    assert_eq!(
        rt.state.settings.hosts[0].tailscale_name.as_deref(),
        Some("rex-c")
    );
    assert!(commands(&rx).is_empty());
    rt.state.panel.as_mut().unwrap().inputs[0] = rune::TextEdit::new("rex-d");
    rt.state.submit(&mut rune::Tx::default());
    assert_eq!(
        rt.state.settings.hosts[0].tailscale_name.as_deref(),
        Some("rex-d")
    );
    assert!(matches!(
        commands(&rx).as_slice(),
        [HostCommand::Tailscale(Call::Rename { hostname })] if hostname == "rex-d"
    ));
    std::fs::remove_file(&rt.state.settings_path).ok();
}

#[test]
fn an_invalid_saved_name_is_shown_and_blocks_calls_until_fixed() {
    let mut ws = workspace();
    let rx = capture_host(&mut ws, 0);
    ws.hosts[0].model.connected = true;
    ws.hosts[0].model.tailscale = Some(state("disabled"));
    ws.settings.hosts[0].keep_tailscale = true;
    ws.settings.hosts[0].tailscale_name = Some("bad name".into());
    let mut rt = hosts_settings(ws);
    assert!(has(&rt, "tailscale-name-problem:0"));
    click(&mut rt, "tailscale:0");
    assert!(commands(&rx).is_empty(), "no enable under an invalid name");
    click(&mut rt, "tailscale-keep:0");
    click(&mut rt, "tailscale-keep:0");
    let sent = commands(&rx);
    assert_eq!(sent.len(), 2);
    assert!(
        sent.iter()
            .all(|c| matches!(c, HostCommand::KeepTailscale(None)))
    );
    assert!(rt.state.settings.hosts[0].keep_tailscale);
    click(&mut rt, "tailscale-name:0");
    assert_eq!(
        rt.state.panel.as_ref().unwrap().inputs[0].text,
        "bad name",
        "prefilled with the saved name, even an invalid one"
    );
    rt.state.cancel_panel(&mut rune::Tx::default());
    rt.state.set_tailscale_name(0, "rex-ok".into());
    assert!(matches!(
        commands(&rx).as_slice(),
        [HostCommand::KeepTailscale(Some(name))] if name == "rex-ok"
    ));
    rt.invalidate();
    // Let the return from the form to Settings finish.
    for _ in 0..30 {
        rt.frame(1. / 60.);
    }
    assert!(!has(&rt, "tailscale-name-problem:0"));
    std::fs::remove_file(&rt.state.settings_path).ok();
}
