use agent_market_core::{catalog, schema, Runtime};
use serde_json::{json, Value};

fn package(name: &str) -> Value {
    catalog::bundled()
        .into_iter()
        .find(|p| p["name"] == name)
        .unwrap()
}
fn create(r: &Runtime, app: &str, object: &str, data: Value) -> Value {
    r.call("create", json!({"app":app,"object":object,"data":data}))
        .unwrap()
}
fn action(r: &Runtime, app: &str, name: &str, row: &Value, extra: Value) -> Value {
    let mut args = json!({"id":row["id"],"expected_revision":row["revision"]});
    args.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    r.call(
        "rhyven_call",
        json!({"category":app,"function":format!("action_{name}"),"args":args}),
    )
    .unwrap()
}
#[test]
fn existing_work_upgrades_and_full_lifecycles_preserve_history() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::collection(d.path(), "work", "agent").unwrap();
    let app = "rhyven/work-management";
    let mut old: Value =
        serde_json::from_str(include_str!("fixtures/work-management-0.3.0.json")).unwrap();
    old["name"] = json!(app);
    old["publisher"] = json!("rhyven");
    r.install(&old, true, false).unwrap();
    let original = create(&r, app, "task", json!({"title":"Release","owner":"alice"}));
    let issue = create(
        &r,
        app,
        "issue",
        json!({"title":"Regression","task_id":original["id"]}),
    );
    let result = r.install(&package(app), true, true).unwrap();
    assert!(std::path::Path::new(result["recovery_backup"].as_str().unwrap()).exists());
    let mut row = r
        .call(
            "get",
            json!({"app":app,"object":"task","id":original["id"]}),
        )
        .unwrap();
    assert_eq!(row["created_at"], original["created_at"]);
    assert_eq!(row["data"]["labels"], json!([]));
    assert!(row["data"].get("due_date").is_none());
    row = action(&r, app, "assign", &row, json!({"owner":"bob"}));
    row = action(
        &r,
        app,
        "block",
        &row,
        json!({"reason":"Waiting for review"}),
    );
    assert_eq!(row["data"]["status"], "blocked");
    let before = r.snapshot().unwrap();
    assert!(r.call("execute", json!({"app":app,"action":"complete","args":{"id":row["id"],"expected_revision":row["revision"],"result":"Bypass"}})).is_err());
    assert_eq!(r.snapshot().unwrap(), before);
    row = action(&r, app, "resume", &row, json!({"owner":"bob"}));
    row = action(&r, app, "complete", &row, json!({"result":"Tested"}));
    assert!(row["data"]["closed_at"].as_u64().unwrap() > 0);
    row = action(
        &r,
        app,
        "reopen",
        &row,
        json!({"reason":"Follow-up regression"}),
    );
    assert_eq!(row["data"]["closed_at"], 0);
    row = action(&r, app, "cancel", &row, json!({"reason":"Duplicate"}));
    assert_eq!(row["data"]["status"], "cancelled");
    let issue = r
        .call("get", json!({"app":app,"object":"issue","id":issue["id"]}))
        .unwrap();
    assert_eq!(issue["data"]["status"], "open");
    let issue = action(&r, app, "assign_issue", &issue, json!({"owner":"bob"}));
    let issue = action(
        &r,
        app,
        "resolve_issue",
        &issue,
        json!({"result":"Fixed upstream"}),
    );
    assert_eq!(issue["data"]["status"], "resolved");
    assert_eq!(r.call("update", json!({"app":app,"object":"issue","id":issue["id"],"expected_revision":issue["revision"],"patch":{"closed_at":0}})).unwrap_err().code,"protected_field");
}
#[test]
fn knowledge_search_handles_correction_chains_before_filters_and_pagination() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::collection(d.path(), "one", "agent").unwrap();
    let other = Runtime::collection(d.path(), "two", "agent").unwrap();
    let app = "rhyven/project-knowledge";
    let mut old: Value =
        serde_json::from_str(include_str!("fixtures/project-knowledge-0.3.0.json")).unwrap();
    old["name"] = json!(app);
    old["publisher"] = json!("rhyven");
    r.install(&old, true, false).unwrap();
    let original = create(
        &r,
        app,
        "note",
        json!({"title":"Old recovery guidance","body":"Obsolete instructions"}),
    );
    r.install(&package(app), true, true).unwrap();
    other.install(&package(app), true, false).unwrap();
    let replacement = create(
        &r,
        app,
        "note",
        json!({"title":"Release recovery","body":"Restore database and files","supersedes":original["id"],"labels":["operations"],"review_date":"2026-10-01"}),
    );
    let head = create(
        &r,
        app,
        "note",
        json!({"title":"RELEASE runbook","body":"Recovery now uses verified backup","supersedes":replacement["id"],"labels":["operations"],"review_date":"2026-10-02"}),
    );
    create(
        &other,
        app,
        "note",
        json!({"title":"RELEASE runbook","body":"Recovery private"}),
    );
    let query = |extra: Value| {
        r.call(
            "rhyven_call",
            json!({"category":app,"function":"object_note_query","args":extra}),
        )
        .unwrap()
    };
    let found = query(
        json!({"search":"release RECOVERY","current_only":true,"where":{"labels":{"has":"operations"},"review_date":{"le":"2026-10-03"}},"metadata":{"created_at":{"ge":original["created_at"]}},"order_by":[{"field":"$updated_at","direction":"desc"}],"limit":1}),
    );
    assert_eq!(found["total"], 1);
    assert_eq!(found["items"][0]["id"], head["id"]);
    assert_eq!(
        query(json!({"search":"obsolete","current_only":true}))["total"],
        0
    );
    assert_eq!(query(json!({"search":"obsolete"}))["total"], 1);
    assert_eq!(
        query(json!({"current_only":true,"offset":1,"limit":1}))["total"],
        1
    );
    assert!(
        query(json!({"current_only":true,"offset":1,"limit":1}))["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(r.call("update",json!({"app":app,"object":"note","id":head["id"],"expected_revision":1,"patch":{"body":"rewrite"}})).unwrap_err().code,"immutable");
    assert!(r
        .call("query", json!({"app":app,"object":"note","search":"  "}))
        .is_err());
}
#[test]
fn dates_are_valid_calendar_dates_and_metadata_is_engine_owned() {
    let s = json!({"type":"string","format":"date"});
    schema::check(&s, 0).unwrap();
    for date in ["2024-02-29", "2000-02-29", "2026-10-01"] {
        schema::validate(json!(date), &s).unwrap();
    }
    for date in [
        "2026-02-29",
        "1900-02-29",
        "2026-04-31",
        "0000-01-01",
        "2026-1-01",
        "2026-10-01T00:00:00Z",
        "2026-13-01",
        "é026-01-01",
    ] {
        assert!(schema::validate(json!(date), &s).is_err(), "{date}");
    }
    assert!(schema::check(&json!({"type":"integer","format":"date"}), 0).is_err());
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::new(d.path(), "agent").unwrap();
    let app = "rhyven/work-management";
    r.install(&package(app), true, false).unwrap();
    let task = create(
        &r,
        app,
        "task",
        json!({"title":"Date tags","due_date":"2026-10-01","labels":["release"]}),
    );
    create(&r, app, "task", json!({"title":"Undated"}));
    let found = r.call("query",json!({"app":app,"object":"task","search":"date RELEASE","where":{"due_date":{"le":"2026-10-01"}}})).unwrap();
    assert_eq!(found["total"], 1);
    assert_eq!(found["items"][0]["id"], task["id"]);
    assert!(r
        .call(
            "create",
            json!({"app":app,"object":"task","data":{"title":"Bad date","due_date":"2026-02-30"}})
        )
        .is_err());
    assert!(r
        .call(
            "query",
            json!({"app":app,"object":"task","current_only":true})
        )
        .is_err());
}
#[test]
fn invalid_query_contracts_and_unsafe_rule_migrations_are_rejected() {
    let mut old: Value =
        serde_json::from_str(include_str!("fixtures/work-management-0.3.0.json")).unwrap();
    old["name"] = json!("rhyven/work-management");
    old["publisher"] = json!("rhyven");
    let mut p = package("rhyven/work-management");
    p["objects"]["task"]["transitions"]["status"]["open"] = json!([]);
    assert!(agent_market_core::updates::migration(&old, &p).is_err());
    p = package("rhyven/work-management");
    p["objects"]["task"]["protected_fields"] = json!([]);
    assert!(agent_market_core::updates::migration(&old, &p).is_err());
    for fields in [
        json!(["absent"]),
        json!(["title", "title"]),
        json!(["closed_at"]),
    ] {
        let mut p = package("rhyven/work-management");
        p["objects"]["task"]["search_fields"] = fields;
        assert!(catalog::validate(&p).is_err());
    }
    let mut p = package("rhyven/project-knowledge");
    p["objects"]["note"]["immutable"] = json!(false);
    assert!(catalog::validate(&p).is_err());
}
