use agent_market_core::{catalog, inspection, store, Runtime};
use serde_json::{json, Value};

fn invoke(r: &Runtime, action: &str, args: Value) -> Value {
    r.call(
        "rhyven_call",
        json!({"category":"rhyven/marketplace","function":format!("action_{action}"),"args":args}),
    )
    .unwrap()
}

#[test]
fn serving_identity_and_request_evidence_are_scoped_and_do_not_guess() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::collection(dir.path(), "project", "alice").unwrap();
    let categories = r.call("rhyven_categories", json!({})).unwrap();
    let info = invoke(&r, "connection_info", json!({}));
    assert_eq!(categories["engine"], info);
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info["scope"]["collection"], "project");
    assert_eq!(info["executable"], json!(std::env::current_exe().unwrap()));
    let package = catalog::bundled()
        .into_iter()
        .find(|p| p["name"] == "rhyven/work-management")
        .unwrap();
    r.install(&package, true, false).unwrap();
    let record = r.call("create",json!({"app":package["name"],"object":"task","data":{"title":"Inspect me"},"request_id":"create"})).unwrap();
    let report = invoke(&r, "request_report", json!({"request_id":"create"}));
    assert_eq!(report["status"], "complete");
    assert_eq!(report["result"], record);
    let bob = Runtime::new(&r.root, "bob").unwrap();
    assert_eq!(
        invoke(&bob, "request_report", json!({"request_id":"create"}))["status"],
        "not_recorded"
    );
    let other = Runtime::collection(dir.path(), "other", "alice").unwrap();
    assert_eq!(
        invoke(&other, "request_report", json!({"request_id":"create"}))["status"],
        "not_recorded"
    );
    let db = store::open(&r.root).unwrap();
    for (id, encoding, value) in [
        ("pending", "envelope", json!({"status":"pending"})),
        (
            "looks-pending",
            "raw",
            json!({"status":"pending","result":"app data"}),
        ),
        (
            "failure",
            "envelope",
            json!({"status":"error","error":{"message":"declined"}}),
        ),
    ] {
        db.execute(
            "INSERT INTO receipts VALUES('alice',?1,'fingerprint',?2)",
            rusqlite::params![id, value.to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO receipt_context VALUES('alice',?1,'test/app','action',?2)",
            rusqlite::params![id, encoding],
        )
        .unwrap();
    }
    assert_eq!(
        invoke(&r, "request_report", json!({"request_id":"pending"}))["status"],
        "unknown"
    );
    assert_eq!(
        invoke(&r, "request_report", json!({"request_id":"looks-pending"}))["status"],
        "complete"
    );
    assert_eq!(
        invoke(&r, "request_report", json!({"request_id":"failure"}))["status"],
        "error"
    );
    db.execute(
        "DELETE FROM receipt_context WHERE request='looks-pending'",
        [],
    )
    .unwrap();
    assert_eq!(
        invoke(&r, "request_report", json!({"request_id":"looks-pending"}))["status"],
        "recorded_unclassified"
    );
    db.execute_batch(
        "CREATE TABLE stack_runs(id TEXT PRIMARY KEY,actor TEXT,fingerprint TEXT,report TEXT)",
    )
    .unwrap();
    let partial = json!({"status":"failed_or_unknown","steps":[{"id":"first","status":"complete"},{"id":"second","status":"running"}]});
    db.execute(
        "INSERT INTO stack_runs VALUES(?1,'alice','x',?2)",
        rusqlite::params![
            store::hash(&json!(["alice", "partial"])),
            partial.to_string()
        ],
    )
    .unwrap();
    assert_eq!(
        invoke(&r, "request_report", json!({"request_id":"partial"}))["workflow"],
        partial
    );
    assert_eq!(
        invoke(&r, "request_report", json!({"request_id":"partial"}))["status"],
        "partial_or_unknown"
    );
}

#[test]
fn audit_pages_are_ordered_filtered_and_never_skip_later_commits() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "agent").unwrap();
    let db = store::open(&r.root).unwrap();
    for (app, id) in [("a/app", "first"), ("b/app", "other"), ("a/app", "second")] {
        db.execute("INSERT INTO events(app,event) VALUES(?1,?2)",rusqlite::params![app,json!({"operation":"create","actor":"agent","record":{"id":id,"object":"note","revision":1,"data":{"secret":"not returned"}}}).to_string()]).unwrap();
    }
    let first = invoke(&r, "changes", json!({"limit":1,"app":"a/app"}));
    assert_eq!(first["items"][0]["record_id"], "first");
    assert_eq!(first["next_cursor"], 1);
    assert_eq!(first["has_more"], true);
    assert!(!first.to_string().contains("not returned"));
    let second = invoke(
        &r,
        "changes",
        json!({"after":first["next_cursor"],"limit":1,"app":"a/app"}),
    );
    assert_eq!(second["items"][0]["record_id"], "second");
    assert_eq!(second["has_more"], false);
    db.execute(
        "INSERT INTO events(app,event) VALUES('a/app',?1)",
        [json!({"operation":"update"}).to_string()],
    )
    .unwrap();
    let next = invoke(
        &r,
        "changes",
        json!({"after":second["next_cursor"],"app":"a/app"}),
    );
    assert_eq!(next["items"].as_array().unwrap().len(), 1);
    assert_eq!(next["next_cursor"], 4);
    assert!(inspection::changes(&r, json!({"after":5})).is_err());
    assert!(inspection::changes(&r, json!({"limit":0})).is_err());
    assert!(inspection::changes(&r, json!({"after":-1})).is_err());
}
