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

#[test]
fn discovery_search_index_hash_and_batch() {
    let mut p = catalog::bundled()
        .into_iter()
        .find(|p| p["name"] == "rhyven/work-management")
        .unwrap();
    let action = p["actions"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    p["actions"][&action]["keywords"] = json!(["sum", "reconcile"]);
    catalog::validate(&p).unwrap();
    let found = tools::describe(&p, &json!({"search":"add"})).unwrap();
    assert!(found["functions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == format!("action_{action}")));
    let found = tools::describe(&p, &json!({"search":"reconcile"})).unwrap();
    assert_eq!(found["functions"].as_array().unwrap().len(), 1);
    let index = tools::describe(&p, &json!({"index":true})).unwrap();
    assert!(index["functions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f.get("inputSchema").is_none()));
    let hash = index["contract_hash"].clone();
    let fresh = tools::describe(&p, &json!({"if_hash":hash})).unwrap();
    assert_eq!(fresh["unchanged"], true);
    assert!(fresh.get("functions").is_none());
    p["guide"] = json!("Changed guidance invalidates the cache.");
    assert_ne!(
        tools::describe(&p, &json!({})).unwrap()["contract_hash"],
        hash
    );
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    r.install(&p, true, false).unwrap();
    let category = p["name"].clone();
    let categories = r.call("rhyven_categories", json!({})).unwrap();
    let entry = categories["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == category)
        .unwrap();
    assert_eq!(
        entry["contract_hash"],
        tools::contract_hash(&r.describe(category.as_str().unwrap()).unwrap())
    );
    assert!(entry.get("hosting").is_none());
    let request = json!({"category":category,"search":"reconcile"});
    let batch = r
        .call(
            "rhyven_describe",
            json!({"requests":[request.clone(),{"category":"rhyven/marketplace","index":true}]}),
        )
        .unwrap();
    assert_eq!(
        batch["descriptions"][0],
        r.call("rhyven_describe", request).unwrap()
    );
    let error = r
        .call(
            "rhyven_call",
            json!({"category":category,"function":"action_reconcile","args":{}}),
        )
        .unwrap_err();
    assert!(error.to_string().contains(&format!("action_{action}")));
    for bad in [
        json!({"requests":[]}),
        json!({"requests":[{"requests":[]}]}),
        json!({"requests":[{}]}),
        json!({"requests":[{"category":category}],"full":true}),
        json!({"category":category,"index":"yes"}),
        json!({"category":category,"if_hash":"bad"}),
    ] {
        assert!(r.call("rhyven_describe", bad).is_err());
    }
    p["actions"][&action]["keywords"] = json!([17]);
    assert!(catalog::validate(&p).is_err());
}

#[test]
fn discovery_annotations_do_not_approve_execution() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(dir.path(), "test").unwrap();
    let session = tools::AgentSession::new(runtime, &[]).unwrap();
    let definitions = session.definitions();
    for name in ["rhyven_categories", "rhyven_describe"] {
        let tool = definitions.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["annotations"]["destructiveHint"], false);
    }
    let call = definitions
        .iter()
        .find(|t| t["name"] == "rhyven_call")
        .unwrap();
    assert_ne!(call["annotations"]["readOnlyHint"], true);
}
