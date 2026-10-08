use agent_market_core::{authoring, catalog, conformance, Runtime};
use serde_json::{json, Value};
use std::path::Path;
#[test]
fn saved_quality_stack_uses_three_apps_and_another_agent_reads_evidence() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/quality-stack");
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path(), "author").unwrap();
    let mut deps = Vec::new();
    for name in [
        "preflight-checker",
        "failure-to-regression",
        "workflow-evaluator",
    ] {
        let p = catalog::read(&base.join("dependencies").join(name)).unwrap();
        r.install(&p, true, false).unwrap();
        deps.push(p);
    }
    let out = home.path().join("stack.json");
    authoring::compose(&r, &base.join("draft.json"), &out).unwrap();
    let p = catalog::read(&out).unwrap();
    assert!(p["permissions"]
        .as_array()
        .unwrap()
        .contains(&json!("host.execute")));
    assert!(conformance::run_with_dependencies(&p, &deps, false, false).is_err());
    assert_eq!(
        conformance::run_with_dependencies(&p, &deps, false, true).unwrap()["passed"],
        true
    );
    r.install(&p, true, false).unwrap();
    let args = json!({"category":"example/quality-stack","function":"action_verify","args":{"request_id":"quality-once"}});
    let result = r.call("rhyven_call", args.clone()).unwrap();
    assert_eq!(result["improved_without_regressions"], true);
    assert_eq!(r.call("rhyven_call", args).unwrap(), result);
    let other = Runtime::new(home.path(), "reader").unwrap();
    let evaluations=other.call("rhyven_call",json!({"category":"rhyven/workflow-evaluator","function":"action_list_evaluations","args":{}})).unwrap();
    assert_eq!(evaluations["items"].as_array().unwrap().len(), 2);
    let stored: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert!(stored["dependencies"]["preflight"]["sha256"].is_string());
}
