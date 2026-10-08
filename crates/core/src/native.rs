//! Hash-verified package-owned Linux executables; no compiler or shell at install.
use crate::{catalog, error::ensure, store, Error, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command};
pub fn enabled(p: &Value) -> bool {
    p["execution"]["driver"] == "native"
}
fn decode(s: &str) -> Result<Vec<u8>> {
    ensure(
        s.len() <= 900_000 && s.len().is_multiple_of(2) && s.is_ascii(),
        "package",
        "Native artifact exceeds 450 KB or has invalid hex",
    )?;
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| Error::new("package", "Invalid artifact hex"))
        })
        .collect()
}
fn check_artifact(target: &str, a: &Value) -> Result<Vec<u8>> {
    catalog::keys(a, &["sha256", "hex"])?;
    let bytes = decode(a["hex"].as_str().unwrap_or(""))?;
    ensure(
        bytes.len() >= 64 && &bytes[..4] == b"\x7fELF" && bytes[4] == 2 && bytes[5] == 1,
        "package",
        "Expected a 64-bit little-endian ELF executable",
    )?;
    ensure(
        matches!(u16::from_le_bytes([bytes[16], bytes[17]]), 2 | 3),
        "package",
        "ELF must be an executable or position-independent executable",
    )?;
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    ensure(
        (target == "linux-x86_64" && machine == 62)
            || (target == "linux-aarch64" && machine == 183),
        "package",
        "Artifact architecture differs from declared target",
    )?;
    ensure(
        a["sha256"] == format!("{:x}", Sha256::digest(&bytes)),
        "integrity",
        "Native artifact checksum mismatch",
    )?;
    Ok(bytes)
}
pub fn validate(p: &Value) -> Result<()> {
    let e = &p["execution"];
    catalog::keys(e, &["driver", "protocol", "timeout_seconds", "artifacts"])?;
    ensure(
        e["protocol"] == "rhyven.action/1",
        "package",
        "Native actions require rhyven.action/1",
    )?;
    ensure(
        e["timeout_seconds"]
            .as_u64()
            .is_some_and(|t| (1..=300).contains(&t)),
        "package",
        "Native timeout must be 1..300 seconds",
    )?;
    let artifacts = e["artifacts"]
        .as_object()
        .ok_or_else(|| Error::new("package", "Native artifacts required"))?;
    ensure(
        !artifacts.is_empty() && artifacts.len() <= 2,
        "package",
        "Supply one or two Linux architecture artifacts",
    )?;
    for (target, a) in artifacts {
        check_artifact(target, a)?;
    }
    ensure(
        p.to_string().len() <= 1_048_576,
        "package",
        "Native package exceeds existing 1 MiB limit; use a container for larger applications",
    )?;
    Ok(())
}
pub fn requirements(p: &Value) -> Result<Value> {
    let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let a = p["execution"]["artifacts"].get(&target).ok_or_else(|| {
        Error::new(
            "native_unavailable",
            "No artifact for this host; obtain a matching Linux binary or use a container",
        )
    })?;
    check_artifact(&target, a)?;
    Ok(
        serde_json::json!({"target":target,"abi":"ELF64; required dynamic libraries remain the operator's responsibility","unsandboxed":true}),
    )
}
pub fn prepare(_: &Path, p: &Value) -> Result<()> {
    requirements(p).map(|_| ())
}
pub(crate) fn launch(root: &Path, p: &Value) -> Result<crate::script::Invocation> {
    requirements(p)?;
    let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let bytes = check_artifact(&target, &p["execution"]["artifacts"][&target])?;
    let source = tempfile::tempdir()?;
    let binary = source.path().join("action");
    std::fs::write(&binary, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o500))?;
    }
    let data_dir = crate::execution::instance(root, p["name"].as_str().unwrap())?.join("data");
    store::private_dir(&data_dir)?;
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("HOME", source.path())
        .env("PATH", "/usr/bin:/bin")
        .env("RHYVEN_DATA_DIR", &data_dir)
        .current_dir(source.path());
    Ok(crate::script::Invocation {
        command,
        data_dir,
        _source: source,
    })
}
/// Authoring-only helper. The packaged manifest carries bytes, never an arbitrary path.
pub fn artifact(path: &Path) -> Result<Value> {
    ensure(
        std::fs::symlink_metadata(path)?.is_file(),
        "package",
        "Native artifact must be a regular file, not a symlink",
    )?;
    ensure(
        std::fs::metadata(path)?.len() <= 450_000,
        "package",
        "Native artifact exceeds 450 KB; use a container",
    )?;
    let bytes = std::fs::read(path)?;
    Ok(
        serde_json::json!({"sha256":format!("{:x}",Sha256::digest(&bytes)),"hex":bytes.iter().map(|b|format!("{b:02x}")).collect::<String>()}),
    )
}
