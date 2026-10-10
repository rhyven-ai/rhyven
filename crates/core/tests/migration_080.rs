use agent_market_core::{catalog, marketplace, registry, store, Runtime};
use serde_json::json;
use std::path::Path;
#[test]
fn old_source_and_evidence_remain_but_are_not_discoverable() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path(), "agent").unwrap();
    let saved = home.path().join("pallets/saved.json");
    std::fs::create_dir_all(saved.parent().unwrap()).unwrap();
    std::fs::write(&saved, b"old source").unwrap();
    let db = store::open(&r.root).unwrap();
    db.execute_batch("CREATE TABLE pallet_tests(hash TEXT PRIMARY KEY,result TEXT); INSERT INTO pallet_tests VALUES('old','passed');").unwrap();
    let functions = marketplace::describe().to_string();
    assert!(!functions.contains("prepare_pallet"));
    assert!(!functions.contains("pallet_search"));
    assert!(r
        .call(
            "rhyven_call",
            json!({"category":"rhyven/marketplace","function":"action_pallet_list","args":{}})
        )
        .is_err());
    assert_eq!(std::fs::read(&saved).unwrap(), b"old source");
    assert_eq!(
        db.query_row(
            "SELECT result FROM pallet_tests WHERE hash='old'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "passed"
    );
    let index = home.path().join("index.json");
    std::fs::write(&index, r#"{"format":1,"publishers":{},"apps":[]}"#).unwrap();
    std::fs::write(home.path().join("pallets.json"), b"ignored retired sidecar").unwrap();
    assert!(registry::read_index(&index).is_ok());
}
#[test]
fn bundled_source_contract_remains_without_a_pallet_store() {
    let mut p =
        catalog::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/script-python"))
            .unwrap();
    p["files"]["vendor/helper/text.py"] = json!("def normalize(s): return s.strip()\n");
    p["libraries"] = json!({"helper":{"name":"example/helper","version":"0.1.0","sha256":"a".repeat(64),"files":{"vendor/helper/text.py":store::hash(&p["files"]["vendor/helper/text.py"])}}});
    catalog::validate(&p).unwrap();
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path(), "agent").unwrap();
    r.install(&p, true, false).unwrap();
    assert_eq!(
        r.call(
            "execute",
            json!({"app":p["name"],"action":"analyze","args":{"text":"two words"}})
        )
        .unwrap()["words"],
        2
    );
    p["files"]["vendor/helper/text.py"] = json!("changed");
    assert!(catalog::validate(&p).is_err());
}
