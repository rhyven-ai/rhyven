use agent_market_core::{catalog, Runtime};
use serde_json::{json, Value};

#[test]
fn rhyven_publisher_and_display_names_reach_agent_discovery_and_reviews() {
    let home = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(home.path(), "test").unwrap();
    for package in catalog::bundled() {
        assert_eq!(package["publisher"], "rhyven");
        assert!(package["name"].as_str().unwrap().starts_with("rhyven/"));
        assert!(!catalog::display_name(&package).is_empty());
        runtime.install(&package, true, false).unwrap();
    }
    let categories = runtime.call("rhyven_categories", json!({})).unwrap();
    let work = categories["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|app| app["name"] == "rhyven/work-management")
        .unwrap();
    assert_eq!(work["display_name"], "Work Management");
    assert_eq!(work["publisher_label"], "Rhyven");
    assert_eq!(work["trust"], "Unverified");
    runtime.uninstall("rhyven/inventory").unwrap();
    let review = runtime
        .call("rhyven_call", json!({"category":"rhyven/marketplace","function":"action_prepare_install","args":{"app":"rhyven/inventory"}}))
        .unwrap();
    assert_eq!(review["publisher_label"], "Rhyven");
    assert_eq!(review["display_name"], "Inventory");
    assert_eq!(review["trust"], "Unverified");
}

#[test]
fn old_app_ids_retain_their_state_beside_new_rhyven_apps() {
    let home = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(home.path(), "test").unwrap();
    let legacy: Value =
        serde_json::from_str(include_str!("fixtures/work-management-0.3.0.json")).unwrap();
    runtime.install(&legacy, true, false).unwrap();
    let record = runtime
        .call("create", json!({"app":"official/work-management","object":"task","data":{"title":"Existing private work"}}))
        .unwrap();
    let current = catalog::bundled().remove(0);
    runtime.install(&current, true, false).unwrap();
    assert_eq!(
        runtime
            .call(
                "query",
                json!({"app":"rhyven/work-management","object":"task"})
            )
            .unwrap()["total"],
        0
    );
    assert_eq!(
        runtime
            .call(
                "get",
                json!({"app":"official/work-management","object":"task","id":record["id"]})
            )
            .unwrap(),
        record
    );
    runtime.uninstall("rhyven/work-management").unwrap();
    assert_eq!(
        runtime.describe("official/work-management").unwrap()["name"],
        "official/work-management"
    );
}

#[test]
fn repository_documentation_manifest_uses_the_new_identity() {
    let package: Value = serde_json::from_str(include_str!(
        "../../../apps/repo-documentation-tool/app.json"
    ))
    .unwrap();
    catalog::validate(&package).unwrap();
    assert_eq!(package["name"], "rhyven/repo-documentation-tool");
    assert_eq!(package["publisher"], "rhyven");
    assert_eq!(
        catalog::display_name(&package),
        "Rhyven Repo Documentation Tool"
    );
}
