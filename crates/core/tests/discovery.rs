use agent_market_core::{catalog, tools, Runtime};
use serde_json::json;
#[test]
fn compact_discovery_preserves_calls_and_optional_full_contract() {
    let p = catalog::bundled()
        .into_iter()
        .find(|p| p["name"] == "rhyven/work-management")
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    r.install(&p, true, false).unwrap();
    let args = json!({"category":p["name"]});
    let compact = r.call("rhyven_describe", args.clone()).unwrap();
    let full = r
        .call("rhyven_describe", json!({"category":p["name"],"full":true}))
        .unwrap();
    assert_eq!(compact["functions"], full["functions"]);
    assert_eq!(compact["guidance_markdown"], full["guidance_markdown"]);
    assert_eq!(compact["contract"]["hosting"], full["contract"]["hosting"]);
    assert_eq!(
        compact["contract"]["permissions"],
        full["contract"]["permissions"]
    );
    assert_eq!(full["contract"]["actions"], p["actions"]);
    assert!(compact["contract"].get("actions").is_none());
    assert!(compact["contract"].get("guide").is_none());
    assert!(compact.to_string().len() < full.to_string().len());
    for f in compact["functions"].as_array().unwrap() {
        let one = r
            .call(
                "rhyven_describe",
                json!({"category":p["name"],"function":f["name"]}),
            )
            .unwrap();
        assert_eq!(one["functions"], json!([f]));
        assert_eq!(one["scope"], compact["scope"]);
    }
    let search = tools::describe(&p, &json!({"search":"CREATE"})).unwrap();
    assert!(!search["functions"].as_array().unwrap().is_empty());
    for f in search["functions"].as_array().unwrap() {
        assert!(format!("{} {}", f["name"], f["description"])
            .to_lowercase()
            .contains("create"));
    }
    assert_eq!(
        tools::describe(&p, &json!({"search":"nonexistentword"})).unwrap()["functions"],
        json!([])
    );
    for bad in [
        json!({"function":"unknown"}),
        json!({"full":"yes"}),
        json!({"function":"x","search":"y"}),
        json!({"search":false}),
        json!({"search":" "}),
        json!({"other":true}),
    ] {
        assert!(tools::describe(&p, &bad).is_err());
    }
}
