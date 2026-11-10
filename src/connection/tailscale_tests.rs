//! Built-in Tailscale: status tracking, explicit calls and keeping it on for opted-in hosts.
use super::*;
use rexrs::api::server;
use tailscale::Call;

fn host(endpoint: String, keep: Option<&str>) -> HostConnection {
    HostConnection::spawn_labeled(
        endpoint,
        None,
        None,
        None,
        false,
        None,
        "tailscale-test".into(),
        keep.map(str::to_owned),
    )
}

/// A host and its fake server, connected up to the bootstrap request.
async fn start(keep: Option<&str>) -> (HostConnection, WebSocketStream<TcpStream>, Value) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let host = host(endpoint, keep);
    let mut ws = accept(&listener, "rex.control.v1").await;
    welcome(&mut ws, 12).await;
    let request = recv(&mut ws).await;
    assert_eq!(request["method"], "session.bootstrap");
    (host, ws, request)
}

/// Answers the bootstrap at seq 10 with this Tailscale status.
async fn boot_with(ws: &mut WebSocketStream<TcpStream>, request: &Value, tailscale: Value) {
    let mut boot = boot();
    boot.server_status.tailscale = Some(serde_json::from_value(tailscale).unwrap());
    reply(ws, request, 10, serde_json::to_value(boot).unwrap()).await;
}

fn event(seq: u64, status: Value) -> Value {
    json!({"type":"event","seq":seq,"payload":{"type":"tailscale_status_changed","payload":status}})
}

fn running(dns: &str) -> Value {
    json!({"state":"running","dns_name":dns,"allow_tailnet_members":false})
}

fn disabled() -> Value {
    json!({"state":"disabled","allow_tailnet_members":false})
}

/// Expects `server.status`, answers it with this Tailscale status, then expects the enable.
async fn expect_enable(ws: &mut WebSocketStream<TcpStream>, tailscale: Value) -> Value {
    let status = next(ws).await;
    assert_eq!(status["method"], "server.status");
    let response = server::StatusResponse {
        tailscale: Some(serde_json::from_value(tailscale).unwrap()),
        ..Default::default()
    };
    reply(ws, &status, 11, serde_json::to_value(response).unwrap()).await;
    let enable = next(ws).await;
    assert_eq!(enable["method"], "server.tailscale.enable");
    enable
}

/// Nothing at all reaches the server for 300 ms: these hosts have no panes or keymap, so any
/// message would be a Tailscale call.
async fn quiet(ws: &mut WebSocketStream<TcpStream>) {
    if let Ok(message) = tokio::time::timeout(Duration::from_millis(300), recv(ws)).await {
        panic!("unexpected request {message}");
    }
}

async fn until(host: &HostConnection, done: impl Fn(&HostModel) -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !done(&host.model.lock().unwrap()) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

fn state(host: &HostConnection) -> String {
    let m = host.model.lock().unwrap();
    m.tailscale
        .as_ref()
        .expect("a Tailscale status")
        .state
        .clone()
}

async fn close(host: HostConnection, mut ws: WebSocketStream<TcpStream>) {
    drop(host);
    assert!(matches!(ws.next().await, Some(Ok(Message::Close(_)))));
}

#[tokio::test]
async fn keep_on_enables_a_disabled_device_once_with_the_servers_access() {
    let (host, mut ws, request) = start(Some("rex-a")).await;
    boot_with(&mut ws, &request, disabled()).await;
    // Changed since bootstrap: the fresh value must win.
    let enable = expect_enable(
        &mut ws,
        json!({"state":"disabled","allow_tailnet_members":true}),
    )
    .await;
    assert_eq!(
        enable["payload"],
        json!({"hostname":"rex-a","allow_tailnet_members":true})
    );
    reply(
        &mut ws,
        &enable,
        12,
        json!({"tailscale":{"state":"starting","allow_tailnet_members":true}}),
    )
    .await;
    until(&host, |m| {
        m.tailscale.as_ref().is_some_and(|t| t.state == "starting")
    })
    .await;
    send(&mut ws, event(13, disabled())).await;
    quiet(&mut ws).await;
    assert_eq!(state(&host), "disabled");
    close(host, ws).await;
}

#[tokio::test]
async fn an_older_buffered_event_never_overwrites_a_newer_status() {
    let (host, mut ws, request) = start(None).await;
    send(&mut ws, event(9, running("old.t.ts.net"))).await;
    boot_with(&mut ws, &request, disabled()).await;
    connected(&host).await;
    assert_eq!(state(&host), "disabled");
    host.commands
        .send(HostCommand::Tailscale(Call::Disable))
        .unwrap();
    let disable = next(&mut ws).await;
    assert_eq!(disable["method"], "server.tailscale.disable");
    // Buffered behind the reply, which raises the floor to 12, so both events are dropped.
    send(
        &mut ws,
        event(
            11,
            json!({"state":"starting","allow_tailnet_members":false}),
        ),
    )
    .await;
    reply(&mut ws, &disable, 12, json!({"tailscale":disabled()})).await;
    send(&mut ws, event(12, running("new.t.ts.net"))).await;
    // A later event marks that the two before it were handled.
    send(&mut ws, json!({"type":"event","seq":13,"payload":{"type":"session_event","payload":{
        "session_id":SESSION,"event":{"type":"session_label_changed","payload":{"label":"marker"}}}}}))
    .await;
    until(&host, |m| m.sessions[0].label == "marker").await;
    assert_eq!(state(&host), "disabled");
    close(host, ws).await;
}

#[tokio::test]
async fn a_cli_disable_right_after_bootstrap_stops_the_startup_enable() {
    let (host, mut ws, request) = start(Some("rex-a")).await;
    send(&mut ws, event(11, disabled())).await;
    boot_with(&mut ws, &request, running("rex-a.t.ts.net")).await;
    connected(&host).await;
    quiet(&mut ws).await;
    assert_eq!(state(&host), "disabled");
    // The user's own request may enable even after a CLI disable.
    host.commands
        .send(HostCommand::KeepTailscale(Some("rex-a".into())))
        .unwrap();
    let enable = expect_enable(&mut ws, disabled()).await;
    assert_eq!(enable["payload"]["hostname"], "rex-a");
    reply(
        &mut ws,
        &enable,
        12,
        json!({"tailscale":running("rex-a.t.ts.net")}),
    )
    .await;
    until(&host, |m| {
        m.tailscale.as_ref().is_some_and(|t| t.state == "running")
    })
    .await;
    close(host, ws).await;
}

#[tokio::test]
async fn a_listener_failure_is_shown_not_retried() {
    let (host, mut ws, request) = start(Some("rex-a")).await;
    boot_with(
        &mut ws,
        &request,
        json!({"state":"disabled","error":"listen tcp :443: in use","allow_tailnet_members":false}),
    )
    .await;
    connected(&host).await;
    quiet(&mut ws).await;
    {
        let m = host.model.lock().unwrap();
        let tailscale = m.tailscale.as_ref().unwrap();
        assert_eq!(tailscale.error.as_deref(), Some("listen tcp :443: in use"));
    }
    close(host, ws).await;
}

#[tokio::test]
async fn one_rename_survives_stale_names() {
    let (host, mut ws, request) = start(Some("rex-a")).await;
    boot_with(&mut ws, &request, running("rex.t.ts.net")).await;
    let rename = next(&mut ws).await;
    assert_eq!(rename["method"], "server.tailscale.rename");
    assert_eq!(rename["payload"], json!({"hostname":"rex-a"}));
    reply(
        &mut ws,
        &rename,
        11,
        json!({"tailscale":running("rex.t.ts.net")}),
    )
    .await;
    send(&mut ws, event(12, running("rex.t.ts.net"))).await;
    send(&mut ws, event(13, running("rex-a-1.t.ts.net"))).await;
    quiet(&mut ws).await;
    let dns = host
        .model
        .lock()
        .unwrap()
        .tailscale
        .clone()
        .unwrap()
        .dns_name;
    assert_eq!(dns.as_deref(), Some("rex-a-1.t.ts.net"));
    close(host, ws).await;
}

#[tokio::test]
async fn a_rejected_name_is_a_problem_and_the_connection_stays() {
    let (host, mut ws, request) = start(Some("rex-a")).await;
    boot_with(&mut ws, &request, disabled()).await;
    let enable = expect_enable(&mut ws, disabled()).await;
    send(
        &mut ws,
        json!({"type":"response","id":enable["id"],
        "error":{"code":"bad_request","message":"invalid hostname","status":400}}),
    )
    .await;
    until(&host, |m| m.tailscale_problem.is_some()).await;
    connected(&host).await;
    {
        let m = host.model.lock().unwrap();
        assert!(m.connected);
        assert_eq!(
            m.tailscale_problem.as_deref(),
            Some("Couldn't turn on Tailscale: invalid hostname")
        );
    }
    host.commands
        .send(HostCommand::Tailscale(Call::Disable))
        .unwrap();
    let disable = next(&mut ws).await;
    assert_eq!(disable["method"], "server.tailscale.disable");
    assert_eq!(disable["payload"], json!({}));
    reply(&mut ws, &disable, 12, json!({"tailscale":disabled()})).await;
    until(&host, |m| m.tailscale_problem.is_none()).await;
    assert!(host.model.lock().unwrap().connected);
    close(host, ws).await;
}

#[tokio::test]
async fn without_keep_nothing_is_called() {
    let (host, mut ws, request) = start(None).await;
    boot_with(&mut ws, &request, disabled()).await;
    connected(&host).await;
    quiet(&mut ws).await;
    assert_eq!(state(&host), "disabled");
    close(host, ws).await;
}

#[tokio::test]
async fn a_server_without_tailscale_gets_no_calls() {
    let (host, mut ws, request) = start(Some("rex-a")).await;
    reply(&mut ws, &request, 10, serde_json::to_value(boot()).unwrap()).await;
    connected(&host).await;
    host.commands
        .send(HostCommand::Tailscale(Call::Disable))
        .unwrap();
    until(&host, |m| m.tailscale_problem.is_some()).await;
    quiet(&mut ws).await;
    {
        let m = host.model.lock().unwrap();
        assert!(m.tailscale.is_none());
        assert_eq!(
            m.tailscale_problem.as_deref(),
            Some("This server can't run built-in Tailscale.")
        );
    }
    close(host, ws).await;
}
