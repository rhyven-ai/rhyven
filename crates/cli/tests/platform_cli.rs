use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn cli(root: &std::path::Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_rhyven"))
        .arg("--workspace")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn license_is_available_without_initializing_state() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("unused");
    let terms = cli(&root, &["license", "--third-party"]);
    assert_eq!(terms["spdx"], "Apache-2.0");
    assert_eq!(terms["license"], include_str!("../../../LICENSE"));
    assert_eq!(terms["notice"], include_str!("../../../NOTICE"));
    assert!(terms["third_party_notices"]
        .as_str()
        .unwrap()
        .contains("serde-"));
    assert!(!root.exists());
}
#[test]
fn app_scaffolds_include_app_licenses_and_docker_copy_inputs() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("workspace");
    for driver in ["declarative", "container", "service"] {
        let app = d.path().join(driver);
        cli(
            &root,
            &[
                "app",
                "init",
                "acme/example",
                "--runtime",
                driver,
                "--dir",
                app.to_str().unwrap(),
            ],
        );
        assert!(std::fs::read_to_string(app.join("LICENSE"))
            .unwrap()
            .contains("Apache License"));
        assert!(std::fs::read_to_string(app.join("NOTICE"))
            .unwrap()
            .contains("Rhyven contributors"));
        assert_eq!(
            cli(&root, &["app", "validate", app.to_str().unwrap()])["valid"],
            true
        );
        if driver != "declarative" {
            let dockerfile = std::fs::read_to_string(app.join("Dockerfile")).unwrap();
            for line in dockerfile.lines().filter(|line| line.starts_with("COPY ")) {
                let paths: Vec<_> = line.split_whitespace().skip(1).collect();
                for source in &paths[..paths.len() - 1] {
                    assert!(
                        app.join(source).is_file(),
                        "Missing Docker COPY input: {source}"
                    );
                }
            }
        }
    }
}
#[test]
fn collection_defaults_selection_and_config() {
    let d = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_rhyven"))
            .env("RHYVEN_HOME", d.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    assert_eq!(run(&["init"])["scope"]["collection"], "global");
    run(&[
        "--collection",
        "project",
        "install",
        "rhyven/work-management",
        "--accept-permissions",
    ]);
    assert_eq!(run(&["list"]), json!([]));
    assert_eq!(
        run(&["--collection", "project", "list"])[0]["name"],
        "rhyven/work-management"
    );
    assert!(d.path().join("collections/project/state.sqlite3").exists());
    assert!(!d.path().join("collections/project/.rhyven").exists());
    let config = run(&["--collection", "project", "config", "claude"]);
    assert!(config.to_string().contains("--collection"));
    assert!(config.to_string().contains("project"));
    let out = Command::new(env!("CARGO_BIN_EXE_rhyven"))
        .args([
            "--workspace",
            d.path().to_str().unwrap(),
            "--collection",
            "project",
            "list",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
#[test]
fn developer_lifecycle_export_and_universal_process() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("workspace");
    let app = d.path().join("app");
    assert_eq!(cli(&root, &["init"])["apps"], json!([]));
    cli(
        &root,
        &["app", "init", "acme/assets", "--dir", app.to_str().unwrap()],
    );
    assert_eq!(
        cli(&root, &["app", "validate", app.to_str().unwrap()])["valid"],
        true
    );
    assert_eq!(
        cli(&root, &["app", "test", app.to_str().unwrap()])["passed"],
        true
    );
    let archive = d.path().join("app.rhyven.json");
    cli(
        &root,
        &[
            "app",
            "package",
            app.to_str().unwrap(),
            "--out",
            archive.to_str().unwrap(),
        ],
    );
    cli(&root, &["app", "publish", archive.to_str().unwrap()]);
    let denied = Command::new(env!("CARGO_BIN_EXE_rhyven"))
        .arg("--workspace")
        .arg(&root)
        .args(["install", "acme/assets"])
        .output()
        .unwrap();
    assert!(!denied.status.success());
    cli(&root, &["install", "acme/assets", "--accept-permissions"]);
    assert_eq!(cli(&root, &["tools"]).as_array().unwrap().len(), 3);
    let out = d.path().join("export");
    cli(
        &root,
        &[
            "app",
            "export-mcp",
            "acme/assets",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(
        std::fs::read_to_string(out.join("LICENSE")).unwrap(),
        include_str!("../../../LICENSE")
    );
    assert_eq!(
        std::fs::read_to_string(out.join("NOTICE")).unwrap(),
        include_str!("../../../NOTICE")
    );
    assert_eq!(
        std::fs::read_to_string(out.join("THIRD_PARTY_NOTICES.txt")).unwrap(),
        cli(&root, &["license", "--third-party"])["third_party_notices"]
            .as_str()
            .unwrap()
    );
    let moved = d.path().join("moved-export");
    std::fs::rename(out, &moved).unwrap();
    let mut process = Command::new(moved.join("launch.sh"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"object_asset_create","arguments":{"data":{"label":"Portable","serial":"P1"}}}}),
    ];
    {
        let mut stdin = process.stdin.take().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .collect::<Vec<_>>();
    assert!(rows[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["name"] == "action_retire"));
    assert_eq!(
        rows[2]["result"]["structuredContent"]["data"]["label"],
        "Portable"
    );
    assert_eq!(
        cli(
            &root,
            &["call", "query", r#"{"app":"acme/assets","object":"asset"}"#]
        )["total"],
        0
    );
    let snapshot = d.path().join("snapshot.json");
    cli(&root, &["snapshot", "--out", snapshot.to_str().unwrap()]);
    assert!(snapshot.exists());
    cli(&root, &["remove", "acme/assets"]);
    assert_eq!(cli(&root, &["list"]), json!([]));
    cli(&root, &["install", "acme/assets", "--accept-permissions"]);
    cli(&root, &["uninstall", "acme/assets"]);
    assert_eq!(
        cli(&root, &["validate", app.to_str().unwrap()])["valid"],
        true
    );
}

#[test]
fn collection_selection_and_restore_happen_before_auto_creation() {
    let home = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_rhyven"))
            .arg("--home")
            .arg(home.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()
    };
    run(&["collection", "use", "project"]);
    assert_eq!(run(&["collection", "current"])["collection"], "project");
    assert_eq!(run(&["init"])["scope"]["collection"], "project");
    let config = run(&["config", "claude"]);
    assert!(config.to_string().contains("project"));
    run(&["collection", "use", "other"]);
    assert_eq!(
        run(&["--collection", "project", "collection", "current"])["collection"],
        "project"
    );
    let backup = home.path().join("backup.rhyven");
    run(&["backup", "project", "--out", backup.to_str().unwrap()]);
    run(&[
        "restore",
        backup.to_str().unwrap(),
        "--collection",
        "restored",
    ]);
    assert_eq!(
        run(&[
            "--collection",
            "restored",
            "call",
            "rhyven_categories",
            "{}"
        ])["collection"],
        "restored"
    );
}
