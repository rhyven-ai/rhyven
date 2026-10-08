// Deterministic retrieval fixture; measures metadata bytes, not model token savings.
use agent_market_core::{catalog, discovery, Runtime};
use serde_json::{json, Value};
use std::{path::Path, time::Instant};
#[test]
fn thirty_plan_fixture_benchmark() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path(), "benchmark").unwrap();
    let base =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/quality-stack/dependencies");
    for name in [
        "preflight-checker",
        "failure-to-regression",
        "workflow-evaluator",
    ] {
        catalog::publish(&r.root, &catalog::read(&base.join(name)).unwrap()).unwrap();
    }
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/discovery-plans.json")).unwrap();
    let mut cold = vec![];
    let mut warm = vec![];
    let mut bytes = 0;
    let mut hit = 0;
    let mut expected = 0;
    let mut abstentions = 0;
    for case in fixtures.as_array().unwrap() {
        let args = json!({"task":case["id"],"revision":1,"steps":[{"id":"work","need":case["need"]}],"permissions":["state.read","state.write","host.execute"],"backends":["script","declarative"]});
        let start = Instant::now();
        let found = discovery::match_plan(&r, args.clone()).unwrap();
        cold.push(start.elapsed().as_micros());
        assert!(found["candidates"].as_array().unwrap().len() <= 3);
        bytes += found.to_string().len();
        if let Some(wanted) = case["expected"].as_str() {
            expected += 1;
            if found["candidates"].as_array().unwrap().iter().any(|c| {
                format!(
                    "{}#{}",
                    c["category"].as_str().unwrap(),
                    c["function"].as_str().unwrap()
                ) == wanted
            }) {
                hit += 1;
            }
        } else if found["candidates"].as_array().unwrap().is_empty() {
            abstentions += 1;
        }
        let start = Instant::now();
        assert_eq!(discovery::match_plan(&r, args).unwrap()["cached"], true);
        warm.push(start.elapsed().as_micros());
    }
    cold.sort();
    warm.sort();
    println!(
        "{}",
        json!({"fixtures":30,"expected_matches":expected,"top3_hits":hit,"no_match_abstentions":abstentions,"mean_response_bytes":bytes/30,"cold_p50_us":cold[15],"warm_p50_us":warm[15],"model_calls":0,"token_claim":false})
    );
    assert_eq!(abstentions, 2);
}
