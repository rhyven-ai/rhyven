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
    Runtime::collection(home, "global", "setup")?.init()?;
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
        "home":home,"containers_requested":containers,"setup_exit":setup_exit,
        "diagnosis":diagnosis,"resume":if ready {Value::Null} else {json!("rhyven setup --containers")},
        "agent_connection":{"instructions":"rhyven --agent","setup":"rhyven connect --client CLIENT","clients":["codex","claude","cursor","vscode","cline","generic"],"verify":"rhyven connect --check"}});
    let mut state = tempfile::NamedTempFile::new_in(home)?;
    serde_json::to_writer_pretty(&mut state, &value)?;
    state.as_file().sync_all()?;
    state
        .persist(home.join("setup-state.json"))
        .map_err(|e| Error::new("io", e.to_string()))?;
    Ok(value)
}
