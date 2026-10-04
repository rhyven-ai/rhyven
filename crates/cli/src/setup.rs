use agent_market_core::{container, Error, Result, Runtime};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub fn run(home: &Path, containers: bool, yes: bool, plan: bool) -> Result<Value> {
    let before = container::doctor();
    if plan {
        return Ok(
            json!({"plan":true,"home":home,"containers_requested":containers,
            "diagnosis":before,"automatic_routes":["Ubuntu/Debian: Docker CE apt repository and system service", "macOS: vendor-verified Docker Desktop installer"],
            "approval":"System changes require confirmation or --yes; Docker Desktop terms are accepted by the user", "resume":"rhyven setup --containers"}),
        );
    }
    let skills = crate::skills::run(home, true)?;
    let runtime = Runtime::collection(home, "global", "setup")?;
    runtime.init()?;
    let marketplace = catalog_setup(
        &runtime.root,
        std::env::var_os("RHYVEN_SETUP_OFFLINE").is_some_and(|v| v == "1"),
        || agent_market_core::registry::sync(&runtime.root, "rhyven-ai/registry", "main", true),
    );
    let mut setup_exit = None;
    if containers && before["container"]["ready"] != true {
        if cfg!(windows) {
            return Err(Error::new(
                "configuration",
                "Native Windows setup is not yet supported; use the documented WSL route",
            ));
        }
        let mut script = tempfile::NamedTempFile::new()?;
        script.write_all(include_bytes!("../../../scripts/setup-containers.sh"))?;
        let status = Command::new("bash")
            .arg("-c")
            .arg("exec bash \"$1\" >&2")
            .arg("rhyven-setup")
            .arg(script.path())
            .env("RHYVEN_SETUP_YES", if yes { "1" } else { "0" })
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        setup_exit = status.code();
    }
    let diagnosis = if containers {
        container::doctor()
    } else {
        before
    };
    let ready = !containers || diagnosis["container"]["ready"] == true;
    let value = json!({"status":if ready {"ready"} else {"pending"},
        "home":home,"containers_requested":containers,"setup_exit":setup_exit,"marketplace":marketplace,"skills":skills,
        "diagnosis":diagnosis,"resume":if ready {Value::Null} else {json!("rhyven setup --containers")},
        "agent_connection":{"instructions":"rhyven --agent","setup":"rhyven connect --client CLIENT","clients":["codex","claude","cursor","vscode","cline","hermes","openclaw","generic"],"verify":"rhyven connect --check"}});
    let mut state = tempfile::NamedTempFile::new_in(home)?;
    serde_json::to_writer_pretty(&mut state, &value)?;
    state.as_file().sync_all()?;
    state
        .persist(home.join("setup-state.json"))
        .map_err(|e| Error::new("io", e.to_string()))?;
    Ok(value)
}

/// Preserve an existing registry selection and never make network availability a setup requirement.
fn catalog_setup(root: &Path, offline: bool, sync: impl FnOnce() -> Result<Value>) -> Value {
    let retry = "rhyven registry-sync rhyven-ai/registry --anonymous";
    match agent_market_core::collections::registry_dir(root) {
        Ok(dir) if dir.join("github-registry.json").exists() => {
            return json!({"status":"cached","message":"Existing catalog retained; no registry selection changed"})
        }
        Err(error) => return json!({"status":"unavailable","error":error,"retry":retry}),
        _ => (),
    }
    if offline {
        return json!({"status":"offline","message":"Using bundled apps; catalog download skipped","retry":retry});
    }
    match sync() {
        Ok(result) => json!({"status":"synced","result":result}),
        Err(error) => {
            json!({"status":"offline","message":"Catalog unavailable; bundled apps remain usable","error":error,"retry":retry})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_catalog_success_offline_failure_and_existing_registry() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        assert_eq!(
            catalog_setup(root, false, || Ok(json!({"packages":9})))["status"],
            "synced"
        );
        assert_eq!(
            catalog_setup(root, true, || panic!("offline setup contacted network"))["status"],
            "offline"
        );
        let failure = catalog_setup(root, false, || Err(Error::new("network", "unreachable")));
        assert_eq!(failure["status"], "offline");
        assert!(failure["retry"].as_str().unwrap().contains("registry-sync"));
        let dir = agent_market_core::collections::registry_dir(root).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("github-registry.json"),
            b"existing private registry",
        )
        .unwrap();
        assert_eq!(
            catalog_setup(root, false, || panic!("existing registry must be retained"))["status"],
            "cached"
        );
    }
}
