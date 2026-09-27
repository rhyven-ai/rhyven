use agent_market_core::{catalog, conformance, tools::AgentSession, Runtime};
use serde_json::{json, Value};
fn package(name: &str) -> Value {
    catalog::bundled()
        .into_iter()
        .find(|p| p["name"] == name)
        .unwrap()
}
#[test]
fn collections_share_packages_but_isolate_state_and_versions() {
    let d = tempfile::tempdir().unwrap();
    let a = Runtime::collection(d.path(), "global", "agent").unwrap();
    let b = Runtime::collection(d.path(), "project", "agent").unwrap();
    for bad in ["../escape", "/tmp/escape", "", "has space", "UPPER"] {
        assert!(Runtime::collection(d.path(), bad, "agent").is_err());
    }
    let mut p = package("rhyven/work-management");
    a.install(&p, true, false).unwrap();
    b.install(&p, true, false).unwrap();
    assert_eq!(
        std::fs::read_dir(d.path().join("packages/sha256"))
            .unwrap()
            .count(),
        1
    );
    let db = rusqlite::Connection::open(b.root.join("state.sqlite3")).unwrap();
    let reference: String = db
        .query_row("SELECT package FROM apps", [], |r| r.get(0))
        .unwrap();
    assert!(reference.contains("package_sha256"));
    let args = json!({"app":"rhyven/work-management","object":"task","data":{"title":"Private task"},"request_id":"same"});
    a.call("create", args.clone()).unwrap();
    let query = json!({"app":"rhyven/work-management","object":"task"});
    assert_eq!(b.call("query", query.clone()).unwrap()["total"], 0);
    b.call("create", args).unwrap();
    assert_eq!(a.call("query", query.clone()).unwrap()["total"], 1);
    let original = p["version"].clone();
    p["version"] = json!("99.0.0");
    b.install(&p, true, true).unwrap();
    assert_eq!(
        a.describe("rhyven/work-management").unwrap()["version"],
        original
    );
    b.uninstall("rhyven/work-management").unwrap();
    assert!(b.apps().unwrap().as_array().unwrap().is_empty());
    assert_eq!(a.apps().unwrap().as_array().unwrap().len(), 1);
    b.install(&p, true, false).unwrap();
    assert_eq!(b.call("query", query).unwrap()["total"], 1);
    assert_eq!(b.snapshot().unwrap()["apps"][0], p);
    assert_eq!(
        b.call("rhyven_categories", json!({})).unwrap()["collection"],
        "project"
    );
    assert_eq!(
        b.call(
            "rhyven_describe",
            json!({"category":"rhyven/work-management"})
        )
        .unwrap()["scope"]["collection"],
        "project"
    );
    catalog::publish(&a.root, &p).unwrap();
    assert_eq!(
        catalog::resolve(&b.root, "rhyven/work-management").unwrap(),
        p
    );
}
#[test]
fn all_apps_conform_and_empty_runtime_has_three_stable_tools() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    r.init().unwrap();
    assert_eq!(r.apps().unwrap(), json!([]));
    let mut session = AgentSession::new(r.clone(), &[]).unwrap();
    let tools = session.definitions();
    assert_eq!(tools.len(), 3);
    for p in catalog::bundled() {
        catalog::validate(&p).unwrap();
        assert_eq!(conformance::run(&p).unwrap()["passed"], true);
        r.install(&p, true, false).unwrap();
    }
    assert_eq!(
        session.call("rhyven_categories", json!({})).unwrap()["apps"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(tools, session.definitions());
    for p in catalog::bundled() {
        r.uninstall(p["name"].as_str().unwrap()).unwrap();
    }
    assert_eq!(
        session.call("rhyven_categories", json!({})).unwrap()["apps"],
        json!([
            agent_market_core::marketplace::summary(),
            agent_market_core::services::summary()
        ])
    );
}
#[test]
fn permissions_integrity_and_fail_closed_contract() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    let mut p = package("rhyven/inventory");
    assert_eq!(
        r.install(&p, false, false).unwrap_err().code,
        "permission_review_required"
    );
    assert_eq!(r.apps().unwrap(), json!([]));
    p["permissions"] = json!(["state.read", "shell.execute"]);
    assert_eq!(catalog::validate(&p).unwrap_err().code, "permission");
    p = package("rhyven/inventory");
    p["trust"] = json!("Rhyven Certified");
    assert!(catalog::validate(&p).is_err());
    p = package("rhyven/inventory");
    p["permissions"] = json!(["state.read"]);
    r.install(&p, true, false).unwrap();
    assert_eq!(
        r.call(
            "create",
            json!({"app":"rhyven/inventory","object":"asset","data":{"label":"A","serial":"B"}})
        )
        .unwrap_err()
        .code,
        "permission"
    );
}
#[test]
fn actions_protected_fields_relationships_revisions_and_atomic_retries() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    r.install(&package("rhyven/inventory"), true, false)
        .unwrap();
    let create = json!({"app":"rhyven/inventory","object":"asset","data":{"label":"A","serial":"B"},"request_id":"create-1"});
    let a = r.call("create", create.clone()).unwrap();
    assert_eq!(r.call("create", create).unwrap(), a);
    assert_eq!(r.call("create",json!({"app":"rhyven/inventory","object":"asset","data":{"label":"C","serial":"D"},"request_id":"create-1"})).unwrap_err().code,"idempotency_conflict");
    assert_eq!(r.call("update",json!({"app":"rhyven/inventory","object":"asset","id":a["id"],"expected_revision":1,"patch":{"status":"retired"}})).unwrap_err().code,"protected_field");
    assert_eq!(r.call("update",json!({"app":"rhyven/inventory","object":"asset","id":a["id"],"expected_revision":1,"patch":{"location_id":"missing"}})).unwrap_err().code,"relationship");
    let op = json!({"app":"rhyven/inventory","action":"retire","args":{"id":a["id"],"expected_revision":1},"request_id":"retire-1"});
    let retired = r.call("execute", op.clone()).unwrap();
    assert_eq!(retired["revision"], 2);
    assert_eq!(r.call("execute", op).unwrap(), retired);
    assert_eq!(r.call("update",json!({"app":"rhyven/inventory","object":"asset","id":a["id"],"expected_revision":1,"patch":{"label":"stale"}})).unwrap_err().code,"revision_conflict");
    assert_eq!(r.call("execute",json!({"app":"rhyven/inventory","action":"retire","args":{"id":a["id"],"expected_revision":2}})).unwrap_err().code,"guard_failed");
    let snapshot = r.snapshot().unwrap();
    assert_eq!(snapshot["records"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["events"].as_array().unwrap().len(), 3);
}
#[test]
fn concurrent_writers_only_one_revision_wins() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    r.install(&package("rhyven/inventory"), true, false)
        .unwrap();
    let a = r
        .call(
            "create",
            json!({"app":"rhyven/inventory","object":"asset","data":{"label":"A","serial":"B"}}),
        )
        .unwrap();
    let threads=(0..8).map(|i|{let r=r.clone();let id=a["id"].clone();std::thread::spawn(move||r.call("update",json!({"app":"rhyven/inventory","object":"asset","id":id,"expected_revision":1,"patch":{"label":format!("worker {i}")}})).is_ok())}).collect::<Vec<_>>();
    assert_eq!(
        threads
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|v| *v)
            .count(),
        1
    );
}
#[test]
fn package_upgrades_publication_and_standalone_share_contract() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    let mut p = package("rhyven/inventory");
    r.install(&p, true, false).unwrap();
    let mut s = AgentSession::new(r.clone(), &["rhyven/inventory".into()]).unwrap();
    let record = s
        .call(
            "object_asset_create",
            json!({"data":{"label":"A","serial":"B"}}),
        )
        .unwrap();
    assert_eq!(
        s.call(
            "action_retire",
            json!({"args":{"id":record["id"],"expected_revision":1}})
        )
        .unwrap()["data"]["status"],
        "retired"
    );
    assert!(s
        .call(
            "object_asset_get",
            json!({"app":"other/app","id":record["id"]})
        )
        .is_err());
    p["version"] = json!("0.4.0");
    p["objects"]["asset"]["schema"]["properties"]["color"] = json!({"type":"string"});
    catalog::publish(&r.root, &p).unwrap();
    r.install(&p, true, true).unwrap();
    assert!(
        r.describe("rhyven/inventory").unwrap()["objects"]["asset"]["schema"]["properties"]
            .get("color")
            .is_some()
    );
    let mut bad = p.clone();
    bad["description"] = json!("Changed same version");
    assert!(catalog::publish(&r.root, &bad).is_err());
    p["version"] = json!("0.5.0");
    p["objects"]["asset"]["schema"]["properties"]["label"] = json!({"type":"integer"});
    assert_eq!(
        r.install(&p, true, true).unwrap_err().code,
        "migration_required"
    );
    assert_eq!(r.describe("rhyven/inventory").unwrap()["version"], "0.4.0");
    r.uninstall("rhyven/inventory").unwrap();
    assert_eq!(
        r.install(&p, true, false).unwrap_err().code,
        "migration_required"
    );
    assert!(r
        .call(
            "get",
            json!({"app":"rhyven/inventory","object":"asset","id":record["id"]})
        )
        .is_err());
    let old = catalog::resolve(&r.root, "rhyven/inventory@0.4.0").unwrap();
    r.install(&old, true, false).unwrap();
    assert_eq!(
        r.call(
            "get",
            json!({"app":"rhyven/inventory","object":"asset","id":record["id"]})
        )
        .unwrap()["revision"],
        2
    );
}
#[test]
fn remote_disclosures_and_real_http_contract() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut p = package("rhyven/inventory");
    p["name"] = json!("community/remote-inventory");
    p["permissions"] = json!(["state.read", "state.write", "network.connect"]);
    p["hosting"] = json!({"mode":"self-hosted","endpoint":format!("http://127.0.0.1:{port}/rhyven"),"auth":"none; loopback test","privacy":"Inputs sent to test endpoint","account":"none","billing":"none","domains":["127.0.0.1"]});
    catalog::validate(&p).unwrap();
    let mut incomplete = p.clone();
    incomplete["hosting"]
        .as_object_mut()
        .unwrap()
        .remove("billing");
    assert!(catalog::validate(&incomplete).is_err());
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let len = header
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if bytes.len() >= end + 4 + len {
                    let request: Value =
                        serde_json::from_slice(&bytes[end + 4..end + 4 + len]).unwrap();
                    assert_eq!(request["operation"], "query");
                    assert_eq!(request["protocol"], "rhyven/1");
                    break;
                }
            }
        }
        let body = r#"{"result":{"items":[],"total":0}}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    r.install(&p, true, false).unwrap();
    assert_eq!(
        r.call("query", json!({"app":p["name"],"object":"asset"}))
            .unwrap()["total"],
        0
    );
    worker.join().unwrap();
}
