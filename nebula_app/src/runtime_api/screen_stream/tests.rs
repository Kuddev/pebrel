use super::*;
use std::sync::atomic::AtomicUsize;

fn read_frame(reader: &mut BufReader<TcpStream>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn command(hub: &RuntimeHub, sink: &EventSink, method: &str, params: Value) -> Value {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    control(&mut server, ApiRequest::new("test".into(), method, params), hub).unwrap();
    let _ = sink;
    read_frame(&mut BufReader::new(client))
}

#[test]
fn mobile_latency_socket_subscription_pushes_changes_without_polling_and_unsubscribes() {
    let hub = RuntimeHub::new();
    let captures = Arc::new(AtomicUsize::new(0));
    let content = Arc::new(Mutex::new(json!({"window_id":1,"pane_id":2,"text":"",
        "screen":{"version":1,"columns":100,
            "rows":vec![vec![json!([" ",1,-257,-258,0]);100];10],
            "cursor":[0,0,1],"palette":[]}})));
    let captured = content.clone();
    let count = captures.clone();
    let sink = EventSink::Callback(Arc::new(move |event| {
        let RuntimeCallback::Control(dispatch) = event else { return };
        assert!(matches!(dispatch.command, RuntimeCommand::ReadPane { screen: true, .. }));
        count.fetch_add(1, Ordering::SeqCst);
        dispatch.respond(Ok(captured.lock().unwrap().clone()));
    }));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server, _) = listener.accept().unwrap();
    write_json_line(
        &mut client,
        &ApiRequest::new(
            "test".into(),
            "pane.screen.subscribe",
            json!({"window_id":1,"pane_id":2}),
        ),
    )
    .unwrap();
    let worker_hub = hub.clone();
    let worker_sink = sink.clone();
    let worker = std::thread::spawn(move || {
        handle_connection(server, "test", &worker_sink, &worker_hub).unwrap();
    });
    let mut reader = BufReader::new(client);
    let reply = read_frame(&mut reader);
    let id = reply["result"]["subscription_id"].as_u64().unwrap();
    let first = read_frame(&mut reader);
    assert_eq!(first["protocol"], PROTOCOL_NAME);
    assert_eq!(first["sequence"], 1);
    assert!(first["data"]["screen"].is_object());
    assert_eq!(captures.load(Ordering::SeqCst), 1);

    // An idle screen produces neither reads nor data frames on a timer.
    reader.get_mut().set_read_timeout(Some(Duration::from_millis(120))).unwrap();
    assert!(reader.read_line(&mut String::new()).is_err());
    assert_eq!(captures.load(Ordering::SeqCst), 1);
    reader.get_mut().set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    content.lock().unwrap()["screen"]["rows"][9][0][0] = json!("x");
    hub.screens.changed(1, 2);
    // A second frame can arrive before the first is ACKed (high-RTT pipeline).
    let second = read_frame(&mut reader);
    assert_eq!(second["sequence"], 2);
    assert_eq!(second["data"]["screen_delta"]["rows"].as_array().unwrap().len(), 1);
    assert_eq!(captures.load(Ordering::SeqCst), 2);
    let target = json!({"window_id":1,"pane_id":2,"subscription_id":id});
    let mut ack = target.clone();
    ack["sequence"] = json!(3);
    assert_eq!(command(&hub, &sink, "pane.screen.ack", ack.clone())["ok"], false);
    ack["sequence"] = json!(2);
    assert_eq!(command(&hub, &sink, "pane.screen.ack", ack)["ok"], true);
    assert_eq!(command(&hub, &sink, "pane.screen.unsubscribe", target.clone())["ok"], true);
    worker.join().unwrap();
    assert_eq!(command(&hub, &sink, "pane.screen.ack", target)["ok"], false);
}
