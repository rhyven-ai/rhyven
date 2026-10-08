#![cfg(target_os = "linux")]
use agent_market_core::{authoring, catalog, native, tools, Runtime};
use serde_json::{json, Value};
use std::{path::Path, process::Command};

#[test]
fn frames_preview_apply_and_refuse_overwrites_or_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frame.json");
    let mut f = json!({"format":"rhyven.frame/1","name":"test/frame","version":"0.1.0","files":{"src/main.py":"print('hello')"},"pallets":["test/notes@0.1.0"]});
    std::fs::write(&path, f.to_string()).unwrap();
    let out = dir.path().join("project");
    assert_eq!(
        authoring::frame(&path, &out, false).unwrap()["applied"],
        false
    );
    assert!(!out.exists());
    authoring::frame(&path, &out, true).unwrap();
    assert!(out.join("frame-provenance.json").exists());
    assert!(authoring::frame(&path, &out, true).is_err());
    f["files"] = json!({"../escape":"bad"});
    std::fs::write(&path, f.to_string()).unwrap();
    assert!(authoring::frame(&path, &dir.path().join("another"), true).is_err());
    assert!(!dir.path().join("escape").exists());
}

fn native_package(binary: &Path) -> Value {
    let mut p =
        catalog::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/script-python"))
            .unwrap();
    p["name"] = json!("example/native");
    p.as_object_mut().unwrap().remove("files");
    p["execution"] = json!({"driver":"native","protocol":"rhyven.action/1","timeout_seconds":2,"artifacts":{format!("linux-{}",std::env::consts::ARCH):native::artifact(binary).unwrap()}});
    p["actions"]["analyze"]["output"] = json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"],"additionalProperties":false});
    p
}
#[test]
fn native_execution_checks_integrity_consent_protocol_and_registry() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("action.c");
    let binary = dir.path().join("action");
    std::fs::write(&source,r#"#include <stdio.h>
int main(void) { char request[4096]; if (!fgets(request,sizeof request,stdin)) return 1; puts("{\"result\":{\"answer\":42}}"); return 0; }"#).unwrap();
    assert!(Command::new("cc")
        .args(["-Os", "-s"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .status()
        .unwrap()
        .success());
    let p = native_package(&binary);
    catalog::validate(&p).unwrap();
    let r = Runtime::new(dir.path().join("home"), "test").unwrap();
    assert!(r.install(&p, false, false).is_err());
    r.install(&p, true, false).unwrap();
    assert_eq!(
        r.call(
            "execute",
            json!({"app":"example/native","action":"analyze","args":{"text":"hello"}})
        )
        .unwrap()["answer"],
        42
    );
    assert!(!tools::manifest(&p)["contract"]["execution"]["artifacts"]
        .to_string()
        .contains("hex"));
    let mut bad = p.clone();
    bad["execution"]["artifacts"][format!("linux-{}", std::env::consts::ARCH)]["sha256"] =
        json!("0".repeat(64));
    assert!(catalog::validate(&bad).is_err());
    let file = dir.path().join("app.json");
    std::fs::write(&file, p.to_string()).unwrap();
    let entry = agent_market_core::registry::entry(&file, "example/apps", 1).unwrap();
    let index = serde_json::from_value(
        json!({"format":1,"publishers":{"example":"example"},"apps":[entry]}),
    )
    .unwrap();
    agent_market_core::registry::validate(&index, None).unwrap();
}
