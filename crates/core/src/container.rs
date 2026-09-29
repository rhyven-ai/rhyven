//! Docker execution backend. One bounded JSON request per container invocation.
use crate::{catalog, error::ensure, Error, Result};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MAX: usize = 1_048_576;
pub fn enabled(p: &Value) -> bool {
    p["execution"]["driver"] == "container"
}
pub fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn image_reference(s: &str) -> bool {
    if let Some(id) = s.strip_prefix("sha256:") {
        return digest(id);
    }
    s.split_once("@sha256:").is_some_and(|(repo, hash)| {
        !repo.is_empty()
            && repo.as_bytes()[0].is_ascii_lowercase()
            && repo
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._:/-".contains(&b))
            && digest(hash)
    })
}
pub fn validate_execution(p: &Value) -> Result<()> {
    let Some(e) = p.get("execution") else {
        return Ok(());
    };
    if e["driver"] == "declarative" {
        return catalog::keys(e, &["driver"]);
    }
    catalog::keys(
        e,
        &[
            "driver",
            "image",
            "protocol",
            "timeout_seconds",
            "memory_mb",
            "cpus",
            "secrets",
            "mode",
            "start_policy",
            "startup_timeout_seconds",
            "shutdown_timeout_seconds",
            "restart_limit",
            "calls",
        ],
    )?;
    ensure(
        e["driver"] == "container"
            && ((crate::services::enabled(p) && e["protocol"] == "rhyven.service/1")
                || (!crate::services::enabled(p) && e["protocol"] == "rhyven.container/1")),
        "package",
        "Expected container driver and rhyven.container/1 protocol",
    )?;
    crate::services::validate_execution(p)?;
    ensure(
        image_reference(e["image"].as_str().unwrap_or("")),
        "package",
        "Container image must be repository@sha256:digest, or a local sha256:image-id",
    )?;
    for (field, max) in [("timeout_seconds", 300), ("memory_mb", 32768), ("cpus", 32)] {
        if let Some(v) = e.get(field) {
            ensure(
                v.as_u64().is_some_and(|n| n > 0 && n <= max),
                "package",
                format!("{field} must be 1..{max}"),
            )?;
        }
    }
    if let Some(names) = e.get("secrets") {
        let names = names.as_array().ok_or_else(|| {
            Error::new(
                "package",
                "secrets must be an array of environment variable names",
            )
        })?;
        let mut seen = std::collections::BTreeSet::new();
        ensure(names.len() <= 32, "package", "At most 32 secrets")?;
        for name in names {
            let s = name.as_str().unwrap_or("");
            ensure(
                s.starts_with("RHYVEN_SECRET_")
                    && s.len() > 14
                    && s.len() <= 128
                    && s.bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                    && seen.insert(s),
                "package",
                "Secrets must be unique RHYVEN_SECRET_* environment names",
            )?;
        }
    }
    Ok(())
}

fn capture(mut input: impl Read, overflow: Arc<AtomicBool>) -> Vec<u8> {
    let mut result = Vec::new();
    let _ = (&mut input).take((MAX + 1) as u64).read_to_end(&mut result);
    if result.len() > MAX {
        overflow.store(true, Ordering::Relaxed);
    }
    result
}
pub(crate) fn docker(args: &[String], input: Vec<u8>, timeout: u64) -> Result<Vec<u8>> {
    let mut child = Command::new("docker")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            Error::new(
                "container_unavailable",
                format!("Docker is required for this app: {e}"),
            )
        })?;
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out_flag = overflow.clone();
    let err_flag = overflow.clone();
    let reader = std::thread::spawn(move || capture(stdout, out_flag));
    let errors = std::thread::spawn(move || capture(stderr, err_flag));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(timeout) || overflow.load(Ordering::Relaxed) {
            timed_out = started.elapsed() > Duration::from_secs(timeout);
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = writer.join();
    let out = reader.join().unwrap_or_default();
    let err = errors.join().unwrap_or_default();
    ensure(
        !timed_out,
        "container_timeout",
        "Container call timed out; side effects may have occurred. Do not blindly retry",
    )?;
    ensure(
        !overflow.load(Ordering::Relaxed),
        "container_protocol",
        "Container output exceeds 1 MiB",
    )?;
    ensure(
        status.success(),
        "container_failed",
        format!(
            "Docker command failed: {}",
            String::from_utf8_lossy(&err[..err.len().min(4096)])
        ),
    )?;
    Ok(out)
}
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}

/// Installation may pull a pinned image, but never starts app code.
fn local_endpoint() -> Result<()> {
    let host =
        if std::env::var_os("DOCKER_CONTEXT").is_none() && std::env::var("DOCKER_HOST").is_ok() {
            std::env::var("DOCKER_HOST").unwrap()
        } else {
            String::from_utf8_lossy(&docker(
                &args(&[
                    "context",
                    "inspect",
                    "--format",
                    "{{.Endpoints.docker.Host}}",
                ]),
                vec![],
                15,
            )?)
            .trim()
            .to_owned()
        };
    ensure(
        host.starts_with("unix://") || host.starts_with("npipe://"),
        "container_unavailable",
        "Container apps require a local Docker endpoint; select a local Docker context",
    )
}

fn capability_report(info: &Value) -> Value {
    let rootless = info["SecurityOptions"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == "name=rootless"));
    let checks: Vec<Value> = [
        ("cgroup_driver", info["CgroupDriver"].as_str().is_some_and(|s| matches!(s, "systemd" | "cgroupfs"))
            && (!rootless || (info["CgroupDriver"] == "systemd" && info["CgroupVersion"] == "2")),
         "Configure a supported cgroup driver; rootless resource limits require systemd and cgroup v2"),
        (
            "linux_containers",
            info["OSType"] == "linux",
            "Select an engine running Linux containers",
        ),
        (
            "cpu_limit",
            info["CpuCfsQuota"] == true && info["CpuCfsPeriod"] == true,
            "Enable CPU controller delegation for rootless Docker, or select a compatible engine",
        ),
        (
            "memory_limit",
            info["MemoryLimit"] == true,
            "Enable memory controller support on the engine",
        ),
        (
            "process_limit",
            info["PidsLimit"] == true,
            "Enable PID controller support on the engine",
        ),
    ]
    .into_iter()
    .map(|(name, available, remedy)| json!({"name":name,"available":available,"remedy":remedy}))
    .collect();
    let ready = checks.iter().all(|c| c["available"] == true);
    json!({"ready":ready,"checks":checks,"engine_version":info["ServerVersion"],
        "os":info["OSType"],"architecture":info["Architecture"],
        "rootless":info["SecurityOptions"].as_array().is_some_and(|a| a.iter().any(|v| v.as_str().is_some_and(|s| s == "name=rootless"))),
        "cgroup_driver":info["CgroupDriver"],"cgroup_version":info["CgroupVersion"]})
}

/// Read-only diagnosis; does not pull images, execute app code, or change engines.
pub fn doctor() -> Value {
    let result = (|| -> Result<Value> {
        local_endpoint()?;
        let info: Value = serde_json::from_slice(&docker(
            &args(&["info", "--format", "{{json .}}"]),
            vec![],
            15,
        )?)?;
        Ok(capability_report(&info))
    })();
    let container = match result {
        Ok(report) => report,
        Err(e) => json!({"ready":false,"checks":[],"error":e.message,
            "remedy":"Install or start a supported local Docker engine, then run rhyven doctor again"}),
    };
    json!({"rhyven_protocol":1,"declarative":{"ready":true},"container":container,
        "scope":"Engine capability report; image compatibility and data access are checked per app. Registry connectivity and effective isolation require acceptance testing."})
}

fn preflight() -> Result<Value> {
    let report = doctor();
    let c = &report["container"];
    if c["ready"] != true {
        let issues: Vec<String> = c["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["available"] != true)
            .map(|v| {
                format!(
                    "{}: {}",
                    v["name"].as_str().unwrap(),
                    v["remedy"].as_str().unwrap()
                )
            })
            .collect();
        return Err(Error::new("container_unavailable", format!(
            "Container host is incompatible: {}. Run rhyven doctor. Declarative apps remain available.",
            if issues.is_empty() { c["error"].as_str().unwrap_or("Docker unavailable").to_owned() } else { issues.join("; ") }
        )));
    }
    Ok(c.clone())
}

fn architecture(value: &str) -> &str {
    match value {
        "x86_64" | "amd64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        other => other,
    }
}
fn image_compatible(info: &Value, host: &Value) -> Result<()> {
    ensure(info[0]["Os"] == "linux" && info[0]["Architecture"].as_str().zip(host["architecture"].as_str())
        .is_some_and(|(image, host)| architecture(image) == architecture(host)),
        "container_unavailable", "Image platform does not match the engine; use a pinned multi-platform image supporting this host architecture. Emulation is not selected automatically")
}

pub fn prepare(p: &Value) -> Result<()> {
    if !enabled(p) {
        return Ok(());
    }
    let image = p["execution"]["image"].as_str().unwrap();
    let host = preflight()?;
    if docker(&args(&["image", "inspect", image]), vec![], 15).is_err() {
        ensure(
            !image.starts_with("sha256:"),
            "container_unavailable",
            "Local image ID is missing; build or load the image before installation",
        )?;
        docker(&args(&["pull", "--quiet", image]), vec![], 300).map_err(|e| {
            Error::new("container_unavailable", format!("Image download failed; check registry access, authentication, DNS/proxy settings and platform availability: {}", e.message))
        })?;
    }
    let info: Value =
        serde_json::from_slice(&docker(&args(&["image", "inspect", image]), vec![], 15)?)?;
    image_compatible(&info, &host)?;
    ensure(
        info[0]["Config"]["Volumes"].is_null()
            || info[0]["Config"]["Volumes"]
                .as_object()
                .is_some_and(|v| v.is_empty()),
        "container",
        "Image VOLUME declarations are unsupported; use RHYVEN_DATA_DIR",
    )?;
    Ok(())
}
pub(crate) use crate::execution::instance;
pub use crate::execution::lock;
pub(crate) struct Cleanup(pub(crate) String);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = docker(&args(&["rm", "--force", &self.0]), vec![], 10);
    }
}

/// Shared isolation and host checks for one-shot and persistent containers.
pub(crate) fn launch_command(
    root: &Path,
    p: &Value,
    instance_name: &str,
    persistent: bool,
) -> Result<Vec<String>> {
    // Check before recording a pending receipt: unsupported hosts never start app code.
    let host = preflight()?;
    let image_info: Value = serde_json::from_slice(
        &docker(
            &args(&[
                "image",
                "inspect",
                p["execution"]["image"].as_str().unwrap(),
            ]),
            vec![],
            15,
        )
        .map_err(|e| {
            Error::new(
                "container_unavailable",
                format!(
                    "Installed image unavailable; reinstall or load the pinned image: {}",
                    e.message
                ),
            )
        })?,
    )?;
    image_compatible(&image_info, &host)?;
    let data = instance(root, p["name"].as_str().unwrap())?.join("data");
    std::fs::create_dir_all(&data)?;
    ensure(
        !std::fs::symlink_metadata(&data)?.file_type().is_symlink(),
        "container",
        "App data directory must not be a symlink",
    )?;
    let data = std::fs::canonicalize(data)?;
    let path = data
        .to_str()
        .ok_or_else(|| Error::new("container", "Data path must be UTF-8"))?;
    ensure(
        !path.contains([',', '\n', '\r']),
        "container",
        "Data path cannot contain commas or newlines",
    )?;
    let permissions = p["permissions"].as_array().unwrap();
    let writable = permissions.contains(&json!("state.write"));
    std::fs::read_dir(&data).map_err(|e| {
        Error::new(
            "container_unavailable",
            format!("Cannot access app data directory: {e}"),
        )
    })?;
    if writable {
        // Test host access before claiming the request; this does not prove VM mount access.
        let probe = tempfile::NamedTempFile::new_in(&data).map_err(|e| {
            Error::new(
                "container_unavailable",
                format!("App data directory is not writable: {e}"),
            )
        })?;
        probe.close()?;
    }
    let e = &p["execution"];
    let mut command = args(&[
        "run",
        "--rm",
        "--pull=never",
        "-i",
        "--name",
        instance_name,
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--pids-limit=128",
        "--no-healthcheck",
        "--network",
        if permissions.contains(&json!("network.connect")) {
            "bridge"
        } else {
            "none"
        },
        "--tmpfs",
        "/tmp:rw,nosuid,nodev,noexec,size=64m",
        "--env",
        "RHYVEN_DATA_DIR=/data",
    ]);
    command.extend(args(&[
        "--memory",
        &format!("{}m", e["memory_mb"].as_u64().unwrap_or(512)),
        "--cpus",
        &e["cpus"].as_u64().unwrap_or(1).to_string(),
        "--mount",
        &format!(
            "type=bind,src={path},dst=/data{}",
            if writable { "" } else { ",readonly" }
        ),
    ]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(&data)?;
        let user = if host["rootless"] == true {
            "0:0".into()
        } else {
            format!("{}:{}", meta.uid(), meta.gid())
        };
        command.extend(args(&["--user", &user]));
    }
    for secret in e["secrets"].as_array().into_iter().flatten() {
        let secret = secret.as_str().unwrap();
        ensure(
            std::env::var_os(secret).is_some(),
            "auth_required",
            format!("Set {secret} in the runtime environment"),
        )?;
        command.extend(args(&["--env", secret]));
    }
    if persistent {
        command.retain(|v| v != "--rm");
        command.extend(args(&[
            "--restart=no",
            "--log-driver=none",
            "--label",
            &format!("rhyven.service={instance_name}"),
        ]));
    }
    command.push(e["image"].as_str().unwrap().into());
    Ok(command)
}

pub fn call(root: &Path, p: &Value, actor: &str, arguments: &Value) -> Result<Value> {
    crate::execution::call(root, p, actor, arguments)
}
