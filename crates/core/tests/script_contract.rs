#![cfg(unix)]
use agent_market_core::{catalog, conformance, recovery, registry, tools, Runtime};
use serde_json::{json, Value};
use std::path::Path;

fn example(language: &str) -> Value {
    catalog::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/script-{language}")),
    )
    .unwrap()
}

fn call(r: &Runtime, p: &Value, id: Option<&str>) -> agent_market_core::Result<Value> {
    let mut args = json!({"app":p["name"], "action":"analyze", "args":{"text":"hello world"}});
    if let Some(id) = id {
        args["request_id"] = json!(id);
    }
    r.call("execute", args)
}

#[test]
fn script_packages_are_self_contained_and_require_explicit_host_consent() {
    let p = example("python");
    assert!(p["files"]["main.py"].is_string());
    assert!(tools::manifest(&p)["contract"].get("files").is_none());
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    assert_eq!(
        r.install(&p, false, false).unwrap_err().code,
        "permission_review_required"
    );
    assert_eq!(
        conformance::run_with_execution(&p, true).unwrap_err().code,
        "permission_review_required"
    );
    assert!(!dir.path().join(".rhyven/script-runtime").exists());
    let mut invalid = p.clone();
    invalid["permissions"] = json!(["state.read", "state.write", "container.execute"]);
    assert!(catalog::validate(&invalid).is_err());
    for field in ["memory_mb", "cpus", "secrets", "mode", "command"] {
        let mut invalid = p.clone();
        invalid["execution"][field] = json!(1);
        assert!(catalog::validate(&invalid).is_err(), "{field}");
    }
    for path in [
        "../main.py",
        "/tmp/main.py",
        "a//main.py",
        "a/./b.py",
        "a\\b.py",
        "node_modules/a.js",
    ] {
        let mut invalid = p.clone();
        invalid["files"][path] = json!("pass");
        assert!(catalog::validate(&invalid).is_err(), "{path}");
    }
    let mut invalid = p.clone();
    invalid["files"]["main.py/helper.py"] = json!("pass");
    assert!(catalog::validate(&invalid).is_err());
    let mut invalid = p;
    invalid["execution"]["dependencies"] = json!({"pip":"requirements.lock"});
    invalid["files"]["requirements.lock"] = json!("requests>=2\n-e .\n");
    assert!(catalog::validate(&invalid).is_err());
}

#[test]
fn source_bundling_rejects_symlinks_and_file_packages_cannot_read_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    let p = example("python");
    let mut source = p.clone();
    source["files"] = json!(["main.py"]);
    std::fs::write(dir.path().join("app.json"), source.to_string()).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("main.py")).unwrap();
    assert!(catalog::read(dir.path()).is_err());
    std::fs::remove_file(dir.path().join("main.py")).unwrap();
    std::fs::write(
        dir.path().join("main.py"),
        p["files"]["main.py"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(catalog::read(dir.path()).unwrap(), p);
    assert!(catalog::read(&dir.path().join("app.json")).is_err());
}

#[test]
fn python_calls_validate_results_preserve_receipts_and_recover_collection_data() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "one", "test-agent").unwrap();
    let p = example("python");
    r.install(&p, true, false).unwrap();
    let args = json!({"category":p["name"],"function":"action_analyze","args":{"text":"hello world","request_id":"retry-me"}});
    let result = r.call("rhyven_call", args.clone()).unwrap();
    assert_eq!(result["words"], 2);
    assert_eq!(result["calls"], 1);
    assert_eq!(result, r.call("rhyven_call", args).unwrap());
    assert_eq!(call(&r, &p, None).unwrap()["calls"], 2);
    let other = Runtime::collection(home.path(), "two", "another-agent").unwrap();
    other.install(&p, true, false).unwrap();
    assert_eq!(call(&other, &p, None).unwrap()["calls"], 1);
    assert_eq!(
        std::fs::read_dir(home.path().join("script-runtime/environments"))
            .unwrap()
            .count(),
        1
    );
    let backup = home.path().join("saved.rhyven");
    recovery::backup(&r, &backup).unwrap();
    let restored_home = tempfile::tempdir().unwrap();
    assert_eq!(
        recovery::restore(restored_home.path(), "denied", &backup, false)
            .unwrap_err()
            .code,
        "permission_review_required"
    );
    recovery::restore(restored_home.path(), "restored", &backup, true).unwrap();
    let restored = Runtime::collection(restored_home.path(), "restored", "test").unwrap();
    assert_eq!(call(&restored, &p, None).unwrap()["calls"], 3);
    restored.uninstall(p["name"].as_str().unwrap()).unwrap();
    restored.install(&p, true, false).unwrap();
    assert_eq!(call(&restored, &p, None).unwrap()["calls"], 4);
    let mut newer = p.clone();
    newer["version"] = json!("0.2.0");
    restored.install(&newer, true, true).unwrap();
    assert_eq!(call(&restored, &newer, None).unwrap()["calls"], 5);
}

#[test]
fn shared_environments_reuse_exact_dependencies_without_sharing_state() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "shared", "test").unwrap();
    let mut a = example("python");
    a["execution"]["environment"] = json!("shared");
    let mut b = a.clone();
    b["name"] = json!("example/another-script");
    r.install(&a, true, false).unwrap();
    r.install(&b, true, false).unwrap();
    assert_eq!(
        std::fs::read_dir(home.path().join("script-runtime/environments"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(call(&r, &a, None).unwrap()["calls"], 1);
    assert_eq!(call(&r, &b, None).unwrap()["calls"], 1);
    let mut isolated = a;
    isolated["name"] = json!("example/isolated-script");
    isolated["execution"]["environment"] = json!("isolated");
    r.install(&isolated, true, false).unwrap();
    assert_eq!(
        std::fs::read_dir(home.path().join("script-runtime/environments"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn failed_or_invalid_results_cannot_be_blindly_retried() {
    for source in [
        "print('not json')",
        "print('{\"result\":{\"words\":\"wrong\"}}')",
        "print('{\"error\":{\"code\":\"FAILED\",\"message\":\"expected\"}}')",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "test").unwrap();
        let mut p = example("python");
        p["files"]["main.py"] = json!(source);
        r.install(&p, true, false).unwrap();
        assert!(call(&r, &p, Some("failed")).is_err());
        assert_eq!(
            call(&r, &p, Some("failed")).unwrap_err().code,
            "script_incomplete"
        );
        assert_eq!(
            call(&r, &p, None).unwrap_err().kind,
            if source.contains("wrong") {
                "INVALID_ARGUMENT"
            } else {
                "APP_ERROR"
            }
        );
    }
}

#[test]
fn timeout_and_output_limits_kill_process_groups_without_blocking_on_pipes() {
    let dir = tempfile::tempdir().unwrap();
    let r = Runtime::new(dir.path(), "test").unwrap();
    let mut p = example("python");
    p["execution"]["timeout_seconds"] = json!(1);
    p["files"]["main.py"] = json!("import os,subprocess,sys,time\nsubprocess.Popen([sys.executable,'-c',\"import os,time;from pathlib import Path;time.sleep(2);Path(os.environ['RHYVEN_DATA_DIR'],'escaped').write_text('bad')\"])\ntime.sleep(30)\n");
    r.install(&p, true, false).unwrap();
    let start = std::time::Instant::now();
    assert_eq!(call(&r, &p, Some("timeout")).unwrap_err().kind, "TIMEOUT");
    assert!(start.elapsed().as_secs() < 5);
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let data = dir
        .path()
        .join(".rhyven/containers")
        .join(agent_market_core::store::hash(&p["name"]))
        .join("data");
    assert!(!data.join("escaped").exists());
    assert_eq!(
        call(&r, &p, Some("timeout")).unwrap_err().code,
        "script_incomplete"
    );
    let mut noisy = p;
    noisy["name"] = json!("example/noisy");
    noisy["files"]["main.py"] = json!("import sys\nsys.stdout.write('x'*1100000)\n");
    r.install(&noisy, true, false).unwrap();
    assert_eq!(call(&r, &noisy, None).unwrap_err().code, "script_protocol");
}

#[test]
fn native_driver_changes_are_not_implicit_updates_and_registry_discloses_host_access() {
    let p = example("python");
    let mut declarative = catalog::bundled().remove(0);
    declarative["name"] = p["name"].clone();
    assert!(agent_market_core::updates::migration(&declarative, &p).is_err());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bundle.json");
    let bytes = serde_json::to_vec(&p).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let entry: registry::Entry =
        serde_json::from_value(registry::entry(&file, "example/app", 1).unwrap()).unwrap();
    assert!(entry.permissions.contains(&"host.execute".into()));
    assert_eq!(registry::verify_package(&entry, &bytes).unwrap(), p);
    let mut edited = p;
    edited["files"]["main.py"] = json!("print('changed')");
    assert!(registry::verify_package(&entry, &serde_json::to_vec(&edited).unwrap()).is_err());
}
