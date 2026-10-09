use agent_market_core::{catalog, conformance, discovery, pallet, Runtime};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
fn sample(language: &str) -> Value {
    pallet::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(if language == "python" {
            "../../examples/pallet-text"
        } else {
            "../../examples/pallet-text-js"
        }),
    )
    .unwrap()
}
#[test]
fn portable_libraries_run_without_rhyven_and_never_install_apps() {
    for language in ["python", "javascript"] {
        let p = sample(language);
        let home = tempfile::tempdir().unwrap();
        let r = Runtime::new(home.path(), "author").unwrap();
        let before = r.apps().unwrap();
        pallet::save(&r, &p).unwrap();
        assert_eq!(r.apps().unwrap(), before);
        assert!(catalog::validate(&p).is_err());
        assert!(pallet::test(&p, false, None).is_err());
        assert_eq!(pallet::test(&p, true, None).unwrap()["cases"], 4);
        let out = home.path().join("exported");
        assert_eq!(pallet::export(&p, &out).unwrap()["requires_rhyven"], false);
        assert!(pallet::export(&p, &out).is_err());
        assert_eq!(pallet::read(&out).unwrap(), p);
        let program = if language == "python" {
            "python3"
        } else {
            "node"
        };
        let program = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|d| d.join(program))
            .find(|p| p.is_file())
            .unwrap();
        let mut child = Command::new(program)
            .arg(out.join(if language == "python" {
                "run.py"
            } else {
                "run.mjs"
            }))
            .current_dir(&out)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                b"{\"export\":\"prepare_document\",\"args\":{\"title\":\" Release   Notes! \"}}\n",
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"],
            json!({"title":"Release Notes!","slug":"release-notes"})
        );
        let result = pallet::run(&p, "slug", json!({"text":"Same CODE!"}), true, None).unwrap();
        assert_eq!(result, json!({"slug":"same-code"}));
        assert_eq!(r.apps().unwrap(), before);
    }
}
#[test]
fn complete_app_bundles_source_and_runs_with_no_pallet_installed() {
    let author = tempfile::tempdir().unwrap();
    let r = Runtime::new(author.path(), "author").unwrap();
    pallet::save(&r, &sample("python")).unwrap();
    let app = catalog::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/document-intake"),
    )
    .unwrap();
    let out = author.path().join("app.json");
    pallet::bundle(
        &r,
        &app,
        &[("text".into(), "example/text-kit@0.1.0".into())],
        &out,
    )
    .unwrap();
    let built = catalog::read(&out).unwrap();
    assert!(built.get("dependencies").is_none()); // no dependency on an installed library app
    assert_eq!(
        conformance::run_with_permissions(&built, false, true).unwrap()["passed"],
        true
    );
    let clean = tempfile::tempdir().unwrap();
    let second = Runtime::new(clean.path(), "user").unwrap();
    assert!(pallet::packages(&second).unwrap().is_empty());
    second.install(&built, true, false).unwrap();
    assert_eq!(second.call("rhyven_call",json!({"category":built["name"],"function":"action_add_document","args":{"title":"  Future  Release  "}})).unwrap()["slug"],"future-release");
    let other = Runtime::new(clean.path(), "another-agent").unwrap();
    assert_eq!(
        other
            .call(
                "rhyven_call",
                json!({"category":built["name"],"function":"action_list_documents","args":{}})
            )
            .unwrap()["documents"][0]["title"],
        "Future Release"
    );
    let mut tampered = built;
    tampered["files"]["vendor/text/textkit.py"] = json!("print('changed')");
    assert!(catalog::validate(&tampered).is_err());
}
#[test]
fn source_discovery_is_compact_cached_and_separate_from_app_actions() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path(), "agent").unwrap();
    let p = sample("python");
    pallet::save(&r, &p).unwrap();
    let args = json!({"task":"prepare","revision":1,"steps":[{"id":"slug","need":"Convert text to lowercase URL slug"}],"permissions":[],"backends":["source"]});
    let found = discovery::match_plan(&r, args.clone()).unwrap();
    assert!(!found["candidates"].as_array().unwrap().is_empty());
    for c in found["candidates"].as_array().unwrap() {
        assert_eq!(c["source_kind"], "pallet");
        assert_eq!(c["backend"], "source");
        assert!(c.get("category").is_none());
        assert!(c["pallet"].is_string() && c["export"].is_string());
    }
    let selected = &found["candidates"][0];
    let one = discovery::inspect(
        &r,
        found["session"].as_str().unwrap(),
        selected["id"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(one["kind"], "pallet");
    assert!(one.get("files").is_none());
    assert!(one.to_string().len() < p.to_string().len());
    assert_eq!(discovery::match_plan(&r, args).unwrap()["cached"], true);
    let unchanged=r.call("rhyven_call",json!({"category":"rhyven/marketplace","function":"action_pallet_describe","args":{"selector":"example/text-kit@0.1.0","if_hash":one["contract_hash"]}})).unwrap();
    assert_eq!(unchanged["unchanged"], true);
    assert!(unchanged.get("exports").is_none());
}
#[test]
fn identity_paths_and_execution_consent_fail_closed() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path(), "agent").unwrap();
    let p = sample("python");
    pallet::save(&r, &p).unwrap();
    let mut changed = p.clone();
    changed["files"]["textkit.py"] = json!("different");
    assert!(pallet::save(&r, &changed).is_err());
    let mut invalid = p.clone();
    invalid["files"]["../escape.py"] = json!("bad");
    assert!(pallet::validate(&invalid).is_err());
    assert!(pallet::run(&p, "slug", json!({"text":"a"}), false, None).is_err());
    assert!(pallet::run(&p, "slug", json!({"text":12}), true, None).is_err());
    let mut dependency = p;
    dependency["dependencies"] = json!(["example-package==1.0"]);
    assert!(pallet::run(&dependency, "slug", json!({"text":"a"}), true, None).is_err());
}

#[test]
fn saved_library_survives_collection_backup_without_becoming_an_app() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "source", "author").unwrap();
    let p = sample("python");
    pallet::save(&r, &p).unwrap();
    let backup = home.path().join("saved.rhyven");
    agent_market_core::recovery::backup(&r, &backup).unwrap();
    let destination = tempfile::tempdir().unwrap();
    agent_market_core::recovery::restore(destination.path(), "restored", &backup, false).unwrap();
    let restored = Runtime::collection(destination.path(), "restored", "reader").unwrap();
    assert_eq!(
        pallet::resolve(&restored, "example/text-kit@0.1.0").unwrap(),
        p
    );
    assert!(restored.apps().unwrap().as_array().unwrap().is_empty());
}

#[test]
fn scaffold_export_and_frame_keep_library_and_application_references_distinct() {
    for language in ["python", "javascript"] {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("new");
        pallet::init("local/helpers", language, &out).unwrap();
        let p = pallet::read(&out).unwrap();
        assert_eq!(pallet::test(&p, true, None).unwrap()["cases"], 1);
        assert!(pallet::init("local/helpers", language, &out).is_err());
    }
    let dir = tempfile::tempdir().unwrap();
    let frame =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/document-intake/frame.json");
    let preview =
        agent_market_core::authoring::frame(&frame, &dir.path().join("project"), false).unwrap();
    assert_eq!(preview["pallets"], json!(["example/text-kit@0.1.0"]));
    assert_eq!(preview["apps"], json!([]));
    assert_eq!(preview["installed"], false);
}

#[test]
fn workspace_and_global_libraries_are_isolated_and_conflicts_are_explicit() {
    let home = tempfile::tempdir().unwrap();
    let project_a = tempfile::tempdir().unwrap();
    let project_b = tempfile::tempdir().unwrap();
    let mut a = Runtime::collection(home.path(), "one", "author").unwrap();
    a.pallet_workspace = Some(project_a.path().into());
    let mut b = Runtime::collection(home.path(), "two", "other-agent").unwrap();
    b.pallet_workspace = Some(project_b.path().into());
    let p = sample("python");
    let selector = "example/text-kit@0.1.0";
    pallet::save(&a, &p).unwrap();
    assert!(pallet::resolve(&b, selector).is_err());
    assert_eq!(pallet::list(&a).unwrap()[0]["scope"], "workspace");
    pallet::save_scoped(&a, &p, "global").unwrap();
    assert_eq!(pallet::resolve(&b, selector).unwrap(), p);
    assert_eq!(pallet::list(&b).unwrap()[0]["scope"], "global");
    let mut other = p.clone();
    other["description"] = json!("Different project implementation");
    pallet::save(&b, &other).unwrap();
    assert_eq!(
        pallet::resolve(&b, selector).unwrap_err().code,
        "pallet_conflict"
    );
    assert_eq!(
        pallet::resolve(&b, &format!("workspace::{selector}")).unwrap(),
        other
    );
    assert_eq!(
        pallet::resolve(&b, &format!("global::{selector}")).unwrap(),
        p
    );
    assert!(a.apps().unwrap().as_array().unwrap().is_empty());
    let isolated =
        Runtime::collection(tempfile::tempdir().unwrap().path(), "one", "agent").unwrap();
    assert!(pallet::resolve(&isolated, selector).is_err());
    assert!(pallet::save_scoped(&isolated, &p, "workspace").is_err());
}

#[test]
fn marketplace_pallets_are_separate_verified_and_require_approval() {
    use agent_market_core::{collections, registry, store};
    use sha2::{Digest, Sha256};
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "test", "agent").unwrap();
    let p = sample("python");
    let bytes = serde_json::to_vec(&p).unwrap();
    let entry = registry::PalletEntry {
        name: p["name"].as_str().unwrap().into(),
        version: "0.1.0".into(),
        description: p["description"].as_str().unwrap().into(),
        language: "python".into(),
        license: "Apache-2.0".into(),
        repository: "example/libraries".into(),
        asset_id: 1,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    let index = registry::Index {
        format: 1,
        publishers: std::collections::BTreeMap::from([("example".into(), "example".into())]),
        apps: vec![],
        pallets: vec![entry.clone()],
    };
    registry::validate(&index, None).unwrap();
    assert_eq!(registry::verify_pallet(&entry, &bytes).unwrap(), p);
    assert!(registry::verify_pallet(&entry, b"{}").is_err());
    let mut changed = index.clone();
    changed.pallets[0].description = "Changed".into();
    assert!(registry::validate(&changed, Some(&index)).is_err());
    assert!(registry::verify_pallet(&changed.pallets[0], &bytes).is_err());
    changed.pallets[0].repository = "someone-else/library".into();
    assert!(registry::validate(&changed, None).is_err());
    let dir = collections::registry_dir(&r.root).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    store::write(&dir.join("market-metadata.json"),&json!({"repository":"example/registry","ref":"main","anonymous":true,"index":index,"checked_at":0,"stars":{}})).unwrap();
    let found = r.call("rhyven_call",json!({"category":"rhyven/marketplace","function":"action_pallet_search","args":{"query":"text"}})).unwrap();
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert!(r.apps().unwrap().as_array().unwrap().is_empty());
    let review = r.call("rhyven_call",json!({"category":"rhyven/marketplace","function":"action_prepare_pallet","args":{"selector":"example/text-kit@0.1.0","scope":"global"}})).unwrap();
    assert_eq!(review["operation"], "pallet_download");
    let result = r.call("rhyven_call",json!({"category":"rhyven/marketplace","function":"action_apply","args":{"request_id":review["request_id"]}}));
    assert_eq!(result.unwrap_err().code, "approval_required");
    assert!(registry::fetch_pallet(&r, "example/text-kit@0.1.0", "global", false).is_err());
    assert!(pallet::list(&r).unwrap().as_array().unwrap().is_empty());
}

#[test]
fn local_test_evidence_tracks_hash_coverage_failures_and_cached_discovery() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path(), "agent").unwrap();
    let mut p = sample("python");
    // An untested export must not inherit the passing examples of other functions.
    p["exports"]["untested"] = p["exports"]["slug"].clone();
    pallet::save(&r, &p).unwrap();
    assert_eq!(
        pallet::test_evidence(&r, &p, None).unwrap()["status"],
        "not_run"
    );
    assert!(pallet::test_recorded(&r, &p, false, None).is_err());
    assert_eq!(
        pallet::test_evidence(&r, &p, None).unwrap()["status"],
        "not_run"
    );
    let args = json!({"task":"evidence","revision":1,"steps":[{"id":"slug","need":"Convert text to lowercase URL slug"}],"permissions":[],"backends":["source"]});
    let initial = discovery::match_plan(&r, args.clone()).unwrap();
    let selected = initial["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["export"] == "slug")
        .unwrap();
    let session = initial["session"].as_str().unwrap();
    let id = selected["id"].as_str().unwrap();
    assert_eq!(
        discovery::inspect(&r, session, id).unwrap()["tests"]["status"],
        "not_run"
    );
    let report = pallet::test_recorded(&r, &p, true, None).unwrap();
    assert_eq!(report["passed_cases"], 4);
    assert!(report["interpreter"]["version"]
        .as_str()
        .unwrap()
        .contains("Python"));
    assert_eq!(
        pallet::test_evidence(&r, &p, Some("untested")).unwrap()["status"],
        "not_covered"
    );
    let hash = agent_market_core::store::hash(&p);
    let described = pallet::describe_recorded(&r, &p, None, Some(&hash)).unwrap();
    assert_eq!(described["unchanged"], true);
    assert_eq!(described["tests"]["status"], "passed");
    let cached = discovery::match_plan(&r, args.clone()).unwrap();
    assert_eq!(cached["cached"], true);
    assert_eq!(cached["rounds"], initial["rounds"]);
    assert_eq!(
        cached["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["export"] == "slug")
            .unwrap()["tests"]["status"],
        "passed"
    );
    assert_eq!(
        discovery::inspect(&r, session, id).unwrap()["tests"]["status"],
        "passed"
    );
    // A later attempted run with an unavailable interpreter replaces the pass.
    assert!(
        pallet::test_recorded(&r, &p, true, Some(Path::new("/nonexistent/rhyven-python"))).is_err()
    );
    assert_eq!(
        pallet::describe_recorded(&r, &p, None, Some(&hash)).unwrap()["tests"]["status"],
        "failed"
    );
    assert_eq!(
        discovery::inspect(&r, session, id).unwrap()["tests"]["status"],
        "failed"
    );
    // Hashes include source and examples, independent of the advertised version.
    p["files"]["textkit.py"] = json!("# changed source\n");
    assert_eq!(
        pallet::test_evidence(&r, &p, None).unwrap()["status"],
        "not_run"
    );
    let isolated = tempfile::tempdir().unwrap();
    let other = Runtime::new(isolated.path(), "other").unwrap();
    assert_eq!(
        pallet::test_evidence(&other, &sample("python"), None).unwrap()["status"],
        "not_run"
    );
}

#[test]
fn passing_evidence_breaks_discovery_ties_without_crossing_user_homes() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "project", "agent").unwrap();
    let global = Runtime::collection(home.path(), "global", "other-agent").unwrap();
    let mut first = sample("python");
    first["name"] = json!("example/aaa");
    let mut tested = first.clone();
    tested["name"] = json!("example/zzz");
    pallet::save(&r, &first).unwrap();
    pallet::save(&r, &tested).unwrap();
    pallet::test_recorded(&r, &tested, true, None).unwrap();
    assert_eq!(
        pallet::test_evidence(&global, &tested, Some("slug")).unwrap()["status"],
        "passed"
    );
    let args = json!({"task":"ranking","revision":1,"steps":[{"id":"slug","need":"lowercase URL slug"}],"permissions":[],"backends":["source"]});
    let result = discovery::match_plan(&r, args).unwrap();
    let slugs: Vec<_> = result["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["export"] == "slug")
        .collect();
    assert_eq!(slugs.len(), 2);
    assert_eq!(slugs[0]["pallet"], "example/zzz");
    let isolated = tempfile::tempdir().unwrap();
    let other = Runtime::collection(isolated.path(), "global", "other").unwrap();
    assert_eq!(
        pallet::test_evidence(&other, &tested, None).unwrap()["status"],
        "not_run"
    );
}
