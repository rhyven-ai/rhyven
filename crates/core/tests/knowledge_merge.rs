use agent_market_core::{store, Runtime};
use serde_json::{json, Value};
fn setup() -> (tempfile::TempDir, Runtime, Runtime) {
    let temp = tempfile::tempdir().unwrap();
    let a = Runtime::new(temp.path().join("source"), "author").unwrap();
    let b = Runtime::new(temp.path().join("destination"), "reviewer").unwrap();
    let p: Value =
        serde_json::from_str(include_str!("../../../catalog/project-knowledge.json")).unwrap();
    a.install(&p, true, false).unwrap();
    b.install(&p, true, false).unwrap();
    (temp, a, b)
}
fn call(r: &Runtime, suffix: &str, args: Value) -> agent_market_core::Result<Value> {
    r.call("rhyven_call", json!({"category":"rhyven/project-knowledge","function":format!("object_note_{suffix}"),"args":args}))
}
fn note(r: &Runtime, title: &str, parent: Option<&str>) -> Value {
    let mut data = json!({"title":title,"body":"Shared engineering knowledge"});
    if let Some(parent) = parent {
        data["supersedes"] = json!(parent);
    }
    call(r, "create", json!({"data":data})).unwrap()
}
fn apply(r: &Runtime, bundle: &Value) -> Value {
    let preview = call(r, "merge_preview", json!({"bundle":bundle})).unwrap();
    call(
        r,
        "merge_apply",
        json!({"bundle":bundle,"preview_token":preview["preview_token"]}),
    )
    .unwrap()
}
#[test]
fn roundtrip_preserves_history_and_deduplicates_across_hosts() {
    let (_t, a, b) = setup();
    let original = note(&a, "Original", None);
    note(&a, "Correction", original["id"].as_str());
    let exported = call(&a, "export", json!({"current_only":true})).unwrap();
    assert_eq!(exported["selected"], 1);
    let bundle = &exported["bundle"];
    assert_eq!(bundle["records"].as_array().unwrap().len(), 2);
    assert_eq!(apply(&b, bundle)["report"]["new_records"], 2);
    assert_eq!(apply(&b, bundle)["report"]["new_records"], 0);
    let current = call(&b, "query", json!({"current_only":true})).unwrap();
    assert_eq!(current["total"], 1);
    assert_eq!(current["items"][0]["data"]["title"], "Correction");
    assert_eq!(
        current["items"][0]["merge_provenance"]["updated_by"],
        "author"
    );
    let back = call(&b, "export", json!({})).unwrap();
    assert_eq!(apply(&a, &back["bundle"])["report"]["new_records"], 0);
}
#[test]
fn stale_preview_conflicts_and_relationship_errors_never_overwrite() {
    let (_t, a, b) = setup();
    note(&a, "Original", None);
    let bundle = call(&a, "export", json!({})).unwrap()["bundle"].clone();
    let preview = call(&b, "merge_preview", json!({"bundle":bundle})).unwrap();
    note(&b, "Concurrent", None);
    assert_eq!(
        call(
            &b,
            "merge_apply",
            json!({"bundle":bundle,"preview_token":preview["preview_token"]})
        )
        .unwrap_err()
        .code,
        "merge_stale"
    );
    apply(&b, &bundle);
    let mut changed = bundle.clone();
    changed["records"][0]["data"]["body"] = json!("Different source version");
    let p = call(&b, "merge_preview", json!({"bundle":changed})).unwrap();
    assert_eq!(p["conflicts"].as_array().unwrap().len(), 1);
    assert_eq!(
        call(
            &b,
            "merge_apply",
            json!({"bundle":changed,"preview_token":p["preview_token"]})
        )
        .unwrap_err()
        .code,
        "merge_conflict"
    );
    call(
        &b,
        "merge_apply",
        json!({"bundle":changed,"preview_token":p["preview_token"],"allow_conflicts":true}),
    )
    .unwrap();
    assert_eq!(call(&b, "query", json!({})).unwrap()["total"], 3);
    let mut invalid = bundle.clone();
    invalid["records"][0]["data"]["supersedes"] = json!("missing");
    assert!(call(&b, "merge_preview", json!({"bundle":invalid})).is_err());
    invalid["records"][0]["data"]["supersedes"] = invalid["records"][0]["id"].clone();
    assert!(call(&b, "merge_preview", json!({"bundle":invalid})).is_err());
    assert_eq!(call(&b, "query", json!({})).unwrap()["total"], 3);
}
#[test]
fn branching_corrections_require_review_and_keep_both() {
    let (_t, a, b) = setup();
    let base = note(&a, "Base", None);
    let bundle = call(&a, "export", json!({})).unwrap()["bundle"].clone();
    let r = apply(&b, &bundle);
    let dest = r["report"]["id_mapping"][base["id"].as_str().unwrap()]
        .as_str()
        .unwrap();
    note(&a, "Source correction", base["id"].as_str());
    note(&b, "Destination correction", Some(dest));
    let bundle = call(&a, "export", json!({"current_only":true})).unwrap()["bundle"].clone();
    let p = call(&b, "merge_preview", json!({"bundle":bundle})).unwrap();
    assert_eq!(p["conflicts"][0]["kind"], "correction_branch");
    call(
        &b,
        "merge_apply",
        json!({"bundle":bundle,"preview_token":p["preview_token"],"allow_conflicts":true}),
    )
    .unwrap();
    assert_eq!(
        call(&b, "query", json!({"current_only":true})).unwrap()["total"],
        2
    );
}
#[test]
fn query_or_missing_case_insensitive_and_projection() {
    let (_t, a, _b) = setup();
    let first = note(&a, "Linux", None);
    note(&a, "Windows", first["id"].as_str());
    note(&a, "Other", None);
    let r = call(&a,"query",json!({"any_of":[{"title":{"icontains":"LIN"}},{"title":{"starts_with":"Win"}}],"select":["title"]})).unwrap();
    assert_eq!(r["total"], 2);
    assert!(r["items"][0]["data"].get("body").is_none());
    assert!(r["items"][0].get("revision").is_some());
    assert_eq!(
        call(
            &a,
            "query",
            json!({"where":{"supersedes":{"exists":false}}})
        )
        .unwrap()["total"],
        2
    );
    assert!(call(&a, "query", json!({"any_of":[]})).is_err());
    assert!(call(&a, "query", json!({"select":["unknown"]})).is_err());
    assert!(call(&a, "query", json!({"any_of":[{"unknown":{"eq":1}}]})).is_err());
    assert_eq!(
        call(&a, "query", json!({"select":[],"limit":0})).unwrap()["total"],
        3
    );
}
#[test]
fn permissions_contract_and_audit_are_enforced() {
    let (_t, a, b) = setup();
    note(&a, "Note", None);
    let mut bundle = call(&a, "export", json!({})).unwrap()["bundle"].clone();
    bundle["contract"] = json!("wrong");
    assert!(call(&b, "merge_preview", json!({"bundle":bundle})).is_err());
    let bundle = call(&a, "export", json!({})).unwrap()["bundle"].clone();
    apply(&b, &bundle);
    let db = store::open(&b.root).unwrap();
    let n: i64 = db
        .query_row(
            "SELECT count(*) FROM events WHERE json_extract(event,'$.operation')='merge'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    let mut package: Value =
        serde_json::from_str(include_str!("../../../catalog/project-knowledge.json")).unwrap();
    package["permissions"] = json!(["state.read"]);
    // Direct dispatch must enforce permissions even without a discoverable write tool.
    db.execute(
        "UPDATE apps SET package=?1,digest=?2",
        rusqlite::params![package.to_string(), store::hash(&package)],
    )
    .unwrap();
    let p = call(&b, "merge_preview", json!({"bundle":bundle})).unwrap();
    assert_eq!(
        call(
            &b,
            "merge_apply",
            json!({"bundle":bundle,"preview_token":p["preview_token"]})
        )
        .unwrap_err()
        .code,
        "permission"
    );
}

#[test]
fn preview_is_bound_to_destination_and_bad_bundles_leave_no_partial_records() {
    let (temp, a, b) = setup();
    note(&a, "Valid", None);
    let bundle = call(&a, "export", json!({})).unwrap()["bundle"].clone();
    let p = call(&b, "merge_preview", json!({"bundle":bundle})).unwrap();
    let c = Runtime::new(temp.path().join("another-destination"), "reviewer").unwrap();
    let package: Value =
        serde_json::from_str(include_str!("../../../catalog/project-knowledge.json")).unwrap();
    c.install(&package, true, false).unwrap();
    assert_eq!(
        call(
            &c,
            "merge_apply",
            json!({"bundle":bundle,"preview_token":p["preview_token"]})
        )
        .unwrap_err()
        .code,
        "merge_stale"
    );
    let mut bad = bundle.clone();
    let mut invalid = bad["records"][0].clone();
    invalid["id"] = json!("different");
    invalid["data"]["title"] = json!(123);
    bad["records"].as_array_mut().unwrap().push(invalid);
    assert!(call(&b, "merge_preview", json!({"bundle":bad})).is_err());
    assert_eq!(call(&b, "query", json!({})).unwrap()["total"], 0);
    bad = bundle.clone();
    bad["records"][0]["data"]["body"] = json!("x".repeat(524_288));
    assert!(call(&b, "merge_preview", json!({"bundle":bad})).is_err());
    let mut mutable = package.clone();
    mutable["objects"]["note"]["immutable"] = json!(false);
    assert!(!agent_market_core::merge::supported(
        &mutable,
        "note",
        &mutable["objects"]["note"]
    ));
    let mut cross = package.clone();
    cross["objects"]["note"]["relationships"]["supersedes"]["object"] = json!("other");
    cross["objects"]["other"] = cross["objects"]["note"].clone();
    assert!(!agent_market_core::merge::supported(
        &cross,
        "note",
        &cross["objects"]["note"]
    ));
}
