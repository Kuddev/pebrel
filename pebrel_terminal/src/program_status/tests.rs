use super::*;

fn record(body: &str) -> Record {
    let Report::Replace(record) = parse(body.as_bytes()).unwrap() else { panic!("not a record") };
    record
}

fn apply(records: &mut Records, body: &str) {
    records.apply(parse(body.as_bytes()).unwrap());
}

#[test]
fn pairs_are_extensible_and_last_valid_pair_wins() {
    let r = record(
        " state = idle :junk: =x:BAD=x:unknown=ok:state=blocked:kind=auth:progress=37:app=claude-code:msg=5L2g5aW9:title=VGVzdA",
    );
    assert_eq!(r.state, State::Blocked);
    assert_eq!(r.kind, Some(BlockedKind::Auth));
    assert_eq!(r.progress, Some(37));
    assert_eq!(r.message.as_deref(), Some("你好"));
    assert_eq!(r.title.as_deref(), Some("Test"));
    assert_eq!(record("state=working:progress=8:progress=101:kind=permission").progress, None);
    assert_eq!(record("state=idle:progress=8:kind=permission").kind, None);
    assert_eq!(record("state=blocked:kind=unknown:app=a/b").app, None);
    for value in ["+1", "-1", "1.0", "", "101"] {
        assert_eq!(record(&format!("state=working:progress={value}")).progress, None);
    }
    assert!(parse(b"state=working:state=future").is_none());
    assert!(parse(b"msg=VGVzdA==").is_none());
}

#[test]
fn invalid_text_and_limits_discard_the_entire_report_even_when_overwritten() {
    for body in [
        "state=working:msg=_:msg=VGVzdA==".to_owned(),
        "state=working:msg=/w==".to_owned(),
        "state=working:title=VGVzdAo=".to_owned(),
        "state=clear:msg=CQ==".to_owned(),
        "state=done:msg=woA=".to_owned(),
        format!("state=done:app={}", "a".repeat(33)),
        format!("state=done:{}=x", "a".repeat(17)),
        format!("state=done:title={}", "YQ==".repeat(65)),
        format!("state=done:msg={}", "YQ==".repeat(684)),
    ] {
        assert!(parse(body.as_bytes()).is_none(), "{body}");
    }
    let base64 = base64::engine::general_purpose::STANDARD;
    for (key, limit) in [("msg", 2048), ("title", 192)] {
        let valid = format!("state=working:{key}={}", base64.encode("a".repeat(limit)));
        assert!(parse(valid.as_bytes()).is_some());
        let invalid = format!("state=working:{key}={}", base64.encode("a".repeat(limit + 1)));
        assert!(parse(invalid.as_bytes()).is_none());
    }
    for control in ['\0', '\n', '\r', '\t', '\u{7f}', '\u{85}', '\u{9f}'] {
        let body = format!("state=done:msg={}", base64.encode(control.to_string()));
        assert!(parse(body.as_bytes()).is_none());
    }
}

#[test]
fn invalid_ids_never_address_root() {
    for id in ["", "/", "/a", "a/", "a//b", "a=b", "$", "a/b/c/d/e/f/g/h/i"] {
        assert!(parse(format!("state=clear:id={id}").as_bytes()).is_none(), "{id}");
    }
    for id in ["x".repeat(33), vec!["x".repeat(32); 4].join("/")] {
        assert!(parse(format!("state=done:id={id}").as_bytes()).is_none());
    }
    assert!(parse(b"state=working:id=a/b/c/d/e/f/g/h").is_some());
}

#[test]
fn replacement_inheritance_and_subtree_clear_are_atomic() {
    let mut records = Records::default();
    apply(&mut records, "state=working:app=cargo:msg=VGVzdA==");
    apply(&mut records, "state=working:id=build:app=worker");
    apply(&mut records, "state=blocked:id=build/test:kind=question:progress=25");
    apply(&mut records, "state=working:id=builder/test");
    let child = records.iter().find(|r| r.id == "build/test").unwrap();
    assert_eq!(records.app_for(child), Some("worker"));
    apply(&mut records, "state=idle:id=build");
    let child = records.iter().find(|r| r.id == "build/test").unwrap();
    assert_eq!(records.app_for(child), Some("cargo"));
    apply(&mut records, "state=done:id=build/test");
    let child = records.iter().find(|r| r.id == "build/test").unwrap();
    assert_eq!((child.kind, child.progress), (None, None));
    apply(&mut records, "state=clear:id=build");
    assert_eq!(records.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["", "builder/test"]);
    apply(&mut records, "state=idle");
    assert_eq!(records.iter().last().unwrap().message, None);
    apply(&mut records, "state=clear");
    assert_eq!(records.iter().count(), 0);
    assert!(records.seen());
}

#[test]
fn records_are_bounded_and_evicted_by_last_update_not_read() {
    let mut records = Records::default();
    for i in 0..256 {
        apply(&mut records, &format!("state=working:id=r{i}"));
    }
    apply(&mut records, "state=working:id=r0");
    for r in records.iter() {
        let _ = records.app_for(r);
    }
    apply(&mut records, "state=blocked:id=new");
    assert_eq!(records.iter().count(), 256);
    assert!(records.iter().any(|r| r.id == "r0"));
    assert!(!records.iter().any(|r| r.id == "r1"));
    assert!(!records.apply(parse(b"state=blocked:id=new").unwrap()));
}

#[test]
fn prompts_and_exit_retain_results_without_heartbeat_expiry() {
    let mut records = Records::default();
    for state in ["working", "blocked", "idle", "done", "error"] {
        apply(&mut records, &format!("state={state}:id={state}"));
    }
    assert_eq!(records.iter().count(), 5);
    assert!(records.command_ended());
    assert_eq!(records.iter().map(|r| r.state).collect::<Vec<_>>(), [State::Done, State::Error]);
    assert!(!records.command_ended());
    assert!(records.reset());
    assert_eq!(records.iter().count(), 0);
    assert!(!records.seen());
}

#[test]
fn acknowledging_results_keeps_live_children_and_rearms_on_a_new_report() {
    let mut records = Records::default();
    apply(&mut records, "state=done:app=codex");
    apply(&mut records, "state=error:id=failed");
    apply(&mut records, "state=blocked:id=worker");
    assert!(records.acknowledge_results());
    assert!(!records.acknowledge_results());
    let child = records.iter().find(|r| !r.result_acknowledged).unwrap();
    assert_eq!(child.id, "worker");
    assert_eq!(records.app_for(child), Some("codex"));
    assert_eq!(records.iter().count(), 3);
    apply(&mut records, "state=clear:id=worker");
    assert!(records.iter().all(|r| r.result_acknowledged));
    apply(&mut records, "state=done:app=codex");
    assert!(!records.iter().last().unwrap().result_acknowledged);
}
