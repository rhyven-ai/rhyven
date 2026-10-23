use agent_market_core::{catalog, recovery, wobbegong, Runtime};
use serde_json::{json, Value};

fn call(r: &Runtime, action: &str, args: Value) -> Value {
    r.call(
        "rhyven_call",
        json!({"category":wobbegong::APP,"function":format!("action_{action}"),"args":args}),
    )
    .unwrap()
}
fn files(r: &Runtime, text: &str) -> Value {
    call(
        r,
        "ingest_files",
        json!({"source":"manual","documents":[{"id":"guide.md","title":"Guide","text":text,"reference":"docs/guide.md"}]}),
    )
}

#[test]
fn files_are_queryable_after_commit_and_replacement_removes_old_terms() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::collection(d.path(), "project", "agent").unwrap();
    assert_eq!(call(&r, "status", json!({}))["indexed"], false);
    assert!(!wobbegong::path(&r).unwrap().exists());
    assert_eq!(
        files(
            &r,
            "# Runbook\nWobbegong preserves source references.\nAlphaobsolete."
        )["committed"],
        true
    );
    let result = call(&r, "query", json!({"search":"wobbegong references"}));
    assert_eq!(result["items"][0]["reference"], "docs/guide.md");
    assert_eq!(result["items"][0]["source"], "files:manual");
    assert_eq!(result["items"][0]["start_line"], 1);
    assert_eq!(result["items"][0]["end_line"], 3);
    assert!(result["items"][0]["sha256"].as_str().unwrap().len() == 64);
    assert_eq!(result["items"][0]["stale"], Value::Null);
    let mut invalid = json!({"source":"manual","documents":[{"id":"duplicate","text":"x"},{"id":"duplicate","text":"y"}]});
    assert!(r
        .call(
            "rhyven_call",
            json!({"category":wobbegong::APP,"function":"action_ingest_files","args":invalid})
        )
        .is_err());
    assert_eq!(
        call(&r, "query", json!({"search":"alphaobsolete"}))["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    invalid["documents"] = json!([{"id":"too-large","text":"x".repeat(524289)}]);
    assert!(r
        .call(
            "rhyven_call",
            json!({"category":wobbegong::APP,"function":"action_ingest_files","args":invalid})
        )
        .is_err());
    files(&r, "Replacement: betanew.");
    assert!(
        call(&r, "query", json!({"search":"alphaobsolete"}))["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        call(&r, "query", json!({"search":"betanew"}))["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let other = Runtime::collection(d.path(), "other", "agent").unwrap();
    assert!(call(&other, "query", json!({"search":"betanew"}))["items"]
        .as_array()
        .unwrap()
        .is_empty());
    call(&r, "remove_source", json!({"source":"files:manual"}));
    assert!(call(&r, "query", json!({"search":"betanew"}))["items"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn record_adapter_tracks_revisions_and_survives_backup_and_app_update() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::collection(d.path(), "project", "agent").unwrap();
    let mut package = catalog::bundled()
        .into_iter()
        .find(|p| p["name"] == "rhyven/work-management")
        .unwrap();
    r.install(&package, true, false).unwrap();
    let record=r.call("create",json!({"app":package["name"],"object":"task","data":{"title":"Wobbegong retrieval acceptance"}})).unwrap();
    let ingest = json!({"category":package["name"],"object":"task","fields":["title"]});
    assert_eq!(call(&r, "ingest_records", ingest.clone())["documents"], 1);
    let result = call(&r, "query", json!({"search":"retrieval acceptance"}));
    assert_eq!(result["items"][0]["metadata"]["id"], record["id"]);
    assert_eq!(result["items"][0]["stale"], false);
    r.call("update",json!({"app":package["name"],"object":"task","id":record["id"],"expected_revision":1,"patch":{"title":"Freshlychanged title"}})).unwrap();
    assert_eq!(
        call(&r, "query", json!({"search":"retrieval acceptance"}))["items"][0]["stale"],
        true
    );
    call(&r, "ingest_records", ingest);
    assert!(
        call(&r, "query", json!({"search":"retrieval acceptance"}))["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    files(&r, "Durablefile backup coverage.");
    let backup = d.path().join("backup.rhyven");
    recovery::backup(&r, &backup).unwrap();
    recovery::restore(d.path(), "restored", &backup, false).unwrap();
    let restored = Runtime::collection(d.path(), "restored", "agent").unwrap();
    assert_eq!(
        call(&restored, "query", json!({"search":"durablefile"}))["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(&restored, "query", json!({"search":"freshlychanged"}))["items"][0]["stale"],
        false
    );
    package["version"] = json!("99.0.0");
    r.install(&package, true, true).unwrap();
    assert_eq!(
        call(&r, "query", json!({"search":"durablefile"}))["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(r.uninstall(wobbegong::APP).is_err());
    assert!(r.install(&wobbegong::manifest(), true, false).is_err());
}

#[test]
fn unicode_chunks_are_bounded_and_index_paths_cannot_be_redirected() {
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::collection(d.path(), "project", "agent").unwrap();
    let text = format!("{}\nneedleword", "海".repeat(3000));
    files(&r, &text);
    let result = call(&r, "query", json!({"search":"needleword","limit":1}));
    assert_eq!(result["items"][0]["end_line"], 2);
    assert!(result["items"][0]["text"].as_str().unwrap().len() <= 4000);
    assert!(r
        .call(
            "rhyven_call",
            json!({"category":wobbegong::APP,"function":"action_query","args":{"search":"!!!"}})
        )
        .is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        assert_eq!(
            std::fs::metadata(wobbegong::path(&r).unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        let other = Runtime::collection(d.path(), "redirected", "agent").unwrap();
        symlink(r.root.join(".wobbegong"), other.root.join(".wobbegong")).unwrap();
        assert!(other
            .call(
                "rhyven_call",
                json!({"category":wobbegong::APP,"function":"action_status","args":{}})
            )
            .is_err());
    }
}
