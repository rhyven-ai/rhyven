//! Replace the running CLI installation using the same signed-release bootstrap.
use agent_market_core::{error::ensure, Error, Result};
use serde_json::{json, Value};
use std::{io::Write, path::Path, process::Command};

pub fn run(home: &Path, check: bool) -> Result<Value> {
    let output = Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-fsSL",
            "--connect-timeout",
            "10",
            "--max-time",
            "30",
            "--max-filesize",
            "1024",
            "https://rhyvenai.com/VERSION",
        ])
        .output()?;
    ensure(
        output.status.success(),
        "network",
        "Could not check the latest runtime release; check your connection and retry",
    )?;
    let latest = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let wanted =
        semver::Version::parse(&latest).map_err(|e| Error::new("version", e.to_string()))?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let executable = std::env::current_exe()?;
    if check || wanted <= current {
        return Ok(
            json!({"current":env!("CARGO_PKG_VERSION"),"latest":latest,"update_available":wanted>current,"executable":executable,"updated":false}),
        );
    }
    ensure(executable.file_name().is_some_and(|n| n == "rhyven"), "configuration", "This binary is not installed as rhyven. Run the website installer to install into ~/.local/bin, then use that executable")?;
    let directory = executable
        .parent()
        .ok_or_else(|| Error::new("configuration", "Cannot locate install directory"))?;
    let mut script = tempfile::NamedTempFile::new()?;
    script.write_all(include_bytes!("../../../scripts/install.sh"))?;
    let mut key = tempfile::NamedTempFile::new()?;
    key.write_all(include_bytes!("../../../packaging/release-key.txt"))?;
    // The embedded bootstrap verifies signatures and hashes, then atomically replaces
    // the binary. It does not need to execute a freshly downloaded installer script.
    let status = Command::new("bash")
        .arg(script.path())
        .args([
            "--download-base-url",
            "https://rhyvenai.com",
            "--version",
            &format!("v{latest}"),
            "--no-modify-path",
            "--public-key",
        ])
        .arg(key.path())
        .arg("--bin-dir")
        .arg(directory)
        .env("RHYVEN_HOME", home)
        .env("RHYVEN_SETUP_OFFLINE", "1")
        .status()?;
    ensure(
        status.success(),
        "upgrade",
        "Runtime upgrade failed; review the installer error above",
    )?;
    Ok(
        json!({"updated":true,"previous":env!("CARGO_PKG_VERSION"),"version":latest,"executable":executable,"next":"Restart long-running Rhyven servers, supervisors and agent MCP connections to use the new runtime. Installed apps and data are retained."}),
    )
}
