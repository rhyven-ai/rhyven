use agent_market_core::{catalog, conformance, container, registry, schema, tools, Runtime};
use serde_json::{json, Value};

fn package() -> Value {
    serde_json::from_str(include_str!("../../../examples/container-python/app.json")).unwrap()
}

#[test]
fn container_schema_permissions_and_function_contract() {
    let p = package();
    catalog::validate(&p).unwrap();
    let manifest = tools::manifest(&p);
    assert_eq!(manifest["functions"][0]["name"], "action_analyze");
    assert_eq!(
        manifest["functions"][0]["outputSchema"],
        p["actions"]["analyze"]["output"]
    );
    assert!(schema::validate(json!({"text":3}), &manifest["functions"][0]["inputSchema"]).is_err());
    assert_eq!(
        conformance::run(&p).unwrap_err().code,
        "permission_review_required"
    );
    let d = tempfile::tempdir().unwrap();
    let r = Runtime::new(d.path(), "test").unwrap();
    assert_eq!(
        r.install(&p, false, false).unwrap_err().code,
        "permission_review_required"
    );
    for bad in [
        "python:latest",
        "--privileged",
        "name@sha256:bad",
        "$(id)@sha256:bad",
    ] {
        let mut p = p.clone();
        p["execution"]["image"] = json!(bad);
        assert!(catalog::validate(&p).is_err());
    }
    for extra in ["privileged", "mounts", "command", "docker_socket"] {
        let mut p = p.clone();
        p["execution"][extra] = json!(true);
        assert!(catalog::validate(&p).is_err());
    }
    let mut no_permission = p.clone();
    no_permission["permissions"] = json!(["state.read"]);
    assert!(catalog::validate(&no_permission).is_err());
    let mut secrets = p.clone();
    secrets["execution"]["secrets"] = json!(["RHYVEN_SECRET_API_KEY"]);
    assert!(catalog::validate(&secrets).is_err());
    secrets["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("secrets.read"));
    catalog::validate(&secrets).unwrap();
    secrets["execution"]["secrets"] = json!(["HOME"]);
    assert!(catalog::validate(&secrets).is_err());
    let mut network = p;
    network["permissions"]
        .as_array_mut()
        .unwrap()
        .push(json!("network.connect"));
    catalog::validate(&network).unwrap();
    assert!(container::image_reference(&format!(
        "ghcr.io/acme/app@sha256:{}",
        "a".repeat(64)
    )));
}

#[test]
fn distribution_requires_exact_container_disclosures() {
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("app.json");
    let mut p = package();
    std::fs::write(&file, serde_json::to_vec(&p).unwrap()).unwrap();
    assert!(registry::entry(&file, "example/app", 1).is_err());
    p["execution"]["image"] = json!(format!("ghcr.io/example/app@sha256:{}", "a".repeat(64)));
    let bytes = serde_json::to_vec(&p).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let mut entry: registry::Entry =
        serde_json::from_value(registry::entry(&file, "example/app", 1).unwrap()).unwrap();
    assert_eq!(registry::verify_package(&entry, &bytes).unwrap(), p);
    entry.execution.as_mut().unwrap()["image"] =
        json!(format!("ghcr.io/another/app@sha256:{}", "b".repeat(64)));
    assert!(registry::verify_package(&entry, &bytes).is_err());
    entry.execution = None;
    assert!(registry::verify_package(&entry, &bytes).is_err());
}
