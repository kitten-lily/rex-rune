use super::*;

fn queue(model: &mut HostModel) {
    model.apply_block_event(
        SESSION,
        "owned-block".into(),
        BlockEvent::Notification {
            title: "Old connection".into(),
            body: "Must not be replayed".into(),
        },
    );
    model.apply_block_event(SESSION, "owned-block".into(), BlockEvent::Bell);
    assert_eq!(model.notices.len(), 1);
    assert_eq!(model.bells.len(), 1);
}

#[tokio::test]
async fn notification_queues_end_with_the_control_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (close, closing) = tokio::sync::oneshot::channel();
    let (arrived, arrival) = tokio::sync::oneshot::channel();
    let (resume, resumed) = tokio::sync::oneshot::channel();
    let (stop, stopping) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let mut ws = accept(&listener, "rex.control.v1").await;
        welcome(&mut ws, 13).await;
        let request = recv(&mut ws).await;
        send(
            &mut ws,
            json!({"type":"response","id":request["id"],"seq":10,"payload":boot()}),
        )
        .await;
        closing.await.unwrap();
        ws.close(None).await.unwrap();
        drop(ws);
        let mut ws = accept(&listener, "rex.control.v1").await;
        let hello = recv(&mut ws).await;
        assert_eq!(hello["api_revision"], 13);
        arrived.send(()).unwrap();
        resumed.await.unwrap();
        send(&mut ws, json!({"type":"welcome","protocol":"rex.control.v1","api_revision":13,
            "limits":{"max_message_bytes":65536},"client":{"id":"client:next","reconnect_token":"next-token"}})).await;
        let request = recv(&mut ws).await;
        send(
            &mut ws,
            json!({"type":"response","id":request["id"],"seq":10,"payload":boot()}),
        )
        .await;
        stopping.await.unwrap();
    });
    let host = HostConnection::spawn(endpoint, None, None, None, false, None);
    connected(&host).await;
    let old_epoch = {
        let mut model = host.model.lock().unwrap();
        queue(&mut model);
        model.connection_epoch
    };
    close.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(8), arrival)
        .await
        .unwrap()
        .unwrap();
    {
        let mut model = host.model.lock().unwrap();
        assert!(!model.connected);
        assert!(
            model.notices.is_empty(),
            "disconnected notices can be stamped with the next epoch"
        );
        assert!(
            model.bells.is_empty(),
            "disconnected bells must not be replayed"
        );
        // Exercise the welcomed boundary too, independently of disconnect cleanup.
        queue(&mut model);
    }
    resume.send(()).unwrap();
    connected(&host).await;
    {
        let model = host.model.lock().unwrap();
        assert!(model.connection_epoch > old_epoch);
        assert!(model.notices.is_empty());
        assert!(model.bells.is_empty());
    }
    stop.send(()).unwrap();
    server.await.unwrap();
    drop(host);
}
