use agent_market_core::{catalog, composition, discovery, store, Runtime};
use serde_json::{json, Value};
fn brick() -> Value {
    json!({"format":2,"name":"test/notes","version":"0.1.0","publisher":"test","description":"Store notes","hosting":{"mode":"local"},"permissions":["state.read","state.write"],"objects":{"note":{"schema":{"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false}}},"actions":{"save":{"description":"Save project note","input":{"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false},"object":"note","operation":"create","set":{"title":{"$arg":"title"}}}},"guide":"Save notes","tests":[]})
}
fn stack(p: &Value) -> Value {
    json!({"format":2,"name":"test/stack","version":"0.1.0","publisher":"test","description":"Store a batch of notes","hosting":{"mode":"local"},"permissions":["state.read","state.write","app.call"],"dependencies":{"notes":{"app":p["name"],"version":p["version"],"sha256":store::hash(p)}},"objects":{},"actions":{"save":{"description":"Save notes in sequence","operation":"stack","input":{"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false},"output":{"type":"string"},"steps":[{"id":"saved","dependency":"notes","action":"save","args":{"title":{"$input":"/title"}}}],"result":{"$step":"saved","path":"/data/title"}}},"guide":"Reusable notes stack","tests":[]})
}
fn runtime() -> (tempfile::TempDir, Runtime, Value) {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test-agent").unwrap();
    let p = brick();
    r.install(&p, true, false).unwrap();
    (dir, r, p)
}
#[test]
fn stack_roundtrip_receipts_and_audit() {
    let (_dir, r, p) = runtime();
    let s = stack(&p);
    catalog::validate(&s).unwrap();
    r.install(&s, true, false).unwrap();
    let args =
        json!({"app":"test/stack","action":"save","args":{"title":"hello"},"request_id":"run-one"});
    assert_eq!(r.call("execute", args.clone()).unwrap(), "hello");
    assert_eq!(r.call("execute", args).unwrap(), "hello");
    assert_eq!(
        composition::report(&r, "run-one").unwrap()["steps"][0]["status"],
        "complete"
    );
    let other = Runtime::new(&r.root, "second-agent").unwrap();
    assert!(composition::report(&other, "run-one").is_err());
    let records = r
        .call("query", json!({"app":"test/notes","object":"note"}))
        .unwrap();
    assert_eq!(records["total"], 1);
}
#[test]
fn pins_permissions_and_forward_references_fail_closed() {
    let (_dir, r, p) = runtime();
    let mut s = stack(&p);
    s["dependencies"]["notes"]["sha256"] = json!("0".repeat(64));
    assert!(r.install(&s, true, false).is_err());
    s = stack(&p);
    s["permissions"] = json!(["state.read", "app.call"]);
    assert!(r.install(&s, true, false).is_err());
    s = stack(&p);
    s["actions"]["save"]["steps"][0]["args"]["title"] = json!({"$step":"future","path":"/x"});
    assert!(catalog::validate(&s).is_err());
}
#[test]
fn bounded_iteration_and_partial_failure() {
    let (_dir, r, p) = runtime();
    let mut s = stack(&p);
    s["actions"]["save"]["input"] = json!({"type":"object","properties":{"items":{"type":"array","items":{"type":"string"}}},"required":["items"],"additionalProperties":false});
    s["actions"]["save"]["steps"][0]["foreach"] = json!({"$input":"/items"});
    s["actions"]["save"]["steps"][0]["args"] = json!({"title":{"$item":""}});
    s["actions"]["save"]["result"] = json!("done");
    r.install(&s, true, false).unwrap();
    assert_eq!(r.call("execute",json!({"app":"test/stack","action":"save","args":{"items":["a","b"]},"request_id":"batch"})).unwrap(),"done");
    assert!(r.call("execute",json!({"app":"test/stack","action":"save","args":{"items":vec!["a";33]},"request_id":"too-many"})).is_err());
    assert_eq!(
        composition::report(&r, "too-many").unwrap()["status"],
        "failed_or_unknown"
    );
}
#[test]
fn discovery_stops_caches_and_bounds_contract_reads() {
    let (_dir, r, _) = runtime();
    let plan = json!({"task":"notes","revision":1,"steps":[{"id":"save","need":"save project note"}],"permissions":["state.read","state.write"],"backends":["declarative"]});
    let a = discovery::match_plan(&r, plan.clone()).unwrap();
    assert!(!a["candidates"].as_array().unwrap().is_empty());
    assert_eq!(
        discovery::match_plan(&r, plan.clone()).unwrap()["cached"],
        true
    );
    let mut retry = plan;
    retry["retry"] = json!(true);
    assert_eq!(
        discovery::match_plan(&r, retry.clone()).unwrap()["rounds"],
        2
    );
    assert_eq!(discovery::match_plan(&r, retry).unwrap()["rounds"], 2);
    let id = a["candidates"][0]["id"].as_str().unwrap();
    let session = a["session"].as_str().unwrap();
    for _ in 0..8 {
        discovery::inspect(&r, session, id).unwrap();
    }
    assert!(discovery::inspect(&r, session, "not-in-shortlist").is_err());
    let conn = store::open(&r.root).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT inspections FROM discovery_sessions WHERE key=?1",
            [session],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
}
#[test]
fn no_match_and_denied_permissions_do_not_execute() {
    let (_dir, r, _) = runtime();
    let p = json!({"task":"denied","revision":1,"steps":[{"id":"save","need":"save project note"}],"permissions":[],"backends":["declarative"]});
    let result = discovery::match_plan(&r, p).unwrap();
    assert_eq!(result["next_action"], "build");
    assert_eq!(result["gaps"].as_array().unwrap().len(), 1);
    assert_eq!(
        r.call("query", json!({"app":"test/notes","object":"note"}))
            .unwrap()["total"],
        0
    );
}

#[test]
fn changed_dependency_stops_stack_and_partial_failure_is_not_replayed() {
    let (_dir, r, p) = runtime();
    let mut s = stack(&p);
    let first = s["actions"]["save"]["steps"][0].clone();
    let mut second = first.clone();
    second["id"] = json!("bad");
    second["args"] = json!({"title":12});
    s["actions"]["save"]["steps"] = json!([first, second]);
    r.install(&s, true, false).unwrap();
    let args =
        json!({"app":"test/stack","action":"save","args":{"title":"once"},"request_id":"partial"});
    assert!(r.call("execute", args.clone()).is_err());
    assert_eq!(
        composition::report(&r, "partial").unwrap()["steps"][0]["status"],
        "complete"
    );
    assert_eq!(
        r.call("execute", args).unwrap_err().code,
        "stack_incomplete"
    );
    assert_eq!(
        r.call("query", json!({"app":"test/notes","object":"note"}))
            .unwrap()["total"],
        1
    );
    let mut newer = p;
    newer["version"] = json!("0.2.0");
    r.install(&newer, true, true).unwrap();
    assert_eq!(r.call("execute",json!({"app":"test/stack","action":"save","args":{"title":"changed"},"request_id":"after-update"})).unwrap_err().code,"version_conflict");
}

#[test]
fn stack_registry_contracts_and_isolated_tests_use_pinned_fixtures() {
    let (dir, r, p) = runtime();
    let mut s = stack(&p);
    s["tests"] = json!([{"operation":"execute","args":{"action":"save","args":{"title":"fixture"}},"expect":{}}]);
    let file = dir.path().join("stack.json");
    std::fs::write(&file, s.to_string()).unwrap();
    let entry = agent_market_core::registry::entry(&file, "test/apps", 1).unwrap();
    let index =
        serde_json::from_value(json!({"format":1,"publishers":{"test":"test"},"apps":[entry]}))
            .unwrap();
    agent_market_core::registry::validate(&index, None).unwrap();
    let deps = agent_market_core::conformance::dependency_fixtures(&r, &s).unwrap();
    assert_eq!(
        agent_market_core::conformance::run_with_dependencies(&s, &deps, false, false).unwrap()
            ["passed"],
        true
    );
    assert_eq!(
        r.call("query", json!({"app":"test/notes","object":"note"}))
            .unwrap()["total"],
        0
    );
}

#[test]
fn mortar_expressions_reuse_the_declarative_engine() {
    let (_dir, r, p) = runtime();
    let mut s = stack(&p);
    s["actions"]["save"]["result"] = json!({"$expr":{"arg":"title"}});
    r.install(&s, true, false).unwrap();
    assert_eq!(
        r.call(
            "execute",
            json!({"app":"test/stack","action":"save","args":{"title":"expression"}})
        )
        .unwrap(),
        "expression"
    );
}

#[test]
fn plan_shapes_filter_before_ranking_and_report_remaining_budget() {
    let (_dir, r, _) = runtime();
    let mut p = json!({"task":"shapes","revision":1,"steps":[{"id":"save","need":"save project note","input_fields":[{"name":"title","type":"integer"}]}],"permissions":["state.read","state.write"],"backends":["declarative"]});
    assert!(discovery::match_plan(&r, p.clone()).unwrap()["candidates"]
        .as_array()
        .unwrap()
        .is_empty());
    p["revision"] = json!(2);
    p["steps"][0]["output_type"] = json!("object");
    p["steps"][0]["input_fields"][0]["type"] = json!("string");
    let found = discovery::match_plan(&r, p.clone()).unwrap();
    assert!(!found["candidates"].as_array().unwrap().is_empty());
    discovery::inspect(
        &r,
        found["session"].as_str().unwrap(),
        found["candidates"][0]["id"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        discovery::match_plan(&r, p).unwrap()["inspections_remaining"],
        7
    );
}
