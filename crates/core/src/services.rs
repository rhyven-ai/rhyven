//! Persistent container contract, lifecycle state, and collection maintenance.
//! App callbacks are capability checked before entering the normal runtime.
use crate::{
    catalog, collections, container, error::ensure, schema, store, Error, Result, Runtime,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(unix)]
#[path = "service_daemon.rs"]
mod daemon;
pub const APP: &str = "rhyven/runtime";
pub const PRINCIPAL: &str = "_rhyven_service_";
pub(crate) const MAX: usize = 1_048_576;

pub fn enabled(p: &Value) -> bool {
    p["execution"]["driver"] == "container" && p["execution"]["mode"] == "service"
}
pub fn validate_execution(p: &Value) -> Result<()> {
    let e = &p["execution"];
    ensure(
        e.get("mode").is_none() || matches!(e["mode"].as_str(), Some("action" | "service")),
        "package",
        "Container mode must be action or service",
    )?;
    let fields = [
        "start_policy",
        "startup_timeout_seconds",
        "shutdown_timeout_seconds",
        "restart_limit",
        "calls",
    ];
    if !enabled(p) {
        return ensure(
            fields.iter().all(|k| e.get(k).is_none()),
            "package",
            "Service options require mode: service",
        );
    }
    ensure(
        e.get("start_policy").is_none()
            || matches!(e["start_policy"].as_str(), Some("manual" | "on-demand")),
        "package",
        "Service start_policy must be manual or on-demand",
    )?;
    for (k, min, max) in [
        ("startup_timeout_seconds", 1, 300),
        ("shutdown_timeout_seconds", 1, 30),
        ("restart_limit", 0, 10),
    ] {
        if let Some(v) = e.get(k) {
            ensure(
                v.as_u64().is_some_and(|n| n >= min && n <= max),
                "package",
                format!("{k} must be {min}..{max}"),
            )?;
        }
    }
    if let Some(calls) = e.get("calls") {
        let calls = calls
            .as_array()
            .ok_or_else(|| Error::new("package", "calls must be an array"))?;
        ensure(
            calls.len() <= 32,
            "package",
            "At most 32 peer function grants",
        )?;
        let mut seen = std::collections::BTreeSet::new();
        for call in calls {
            catalog::keys(call, &["category", "function", "version"])?;
            let app = call["category"].as_str().unwrap_or("");
            let function = call["function"].as_str().unwrap_or("");
            ensure(
                catalog::app_name(app)
                    && app != crate::marketplace::APP
                    && app != APP
                    && schema::name(function)
                    && seen.insert((app, function)),
                "package",
                "Peer grants require unique non-platform category/function pairs",
            )?;
            catalog::version(call["version"].as_str().unwrap_or(""))?;
        }
    }
    Ok(())
}
pub fn manifest() -> Value {
    let mut actions = serde_json::Map::new();
    for name in ["list", "start", "stop", "restart", "status", "logs"] {
        let props = if name == "list" {
            json!({})
        } else {
            json!({"app":{"type":"string"}})
        };
        let required = if name == "list" {
            json!([])
        } else {
            json!(["app"])
        };
        actions.insert(format!("service_{name}"), json!({"description":format!("{name} persistent services in this collection"),"input":{"type":"object","properties":props,"required":required,"additionalProperties":false},"output":{"type":"object"}}));
    }
    json!({"format":2,"name":APP,"version":"0.1.0","publisher":"rhyven","platform":true,
        "description":"Manage persistent local app services, their status and bounded logs.",
        "hosting":{"mode":"local"},"permissions":[],"objects":{},"actions":actions,
        "guide":"Service mode uses an approved installed image. Start enables background execution and restart recovery; stop disables it durably. Status reports desired and observed state. Start a local supervisor with rhyven daemon start. Installation never starts service code. Calls use the same collection; no implicit global routing. Logs may contain private app data."})
}
pub fn summary() -> Value {
    json!({"name":APP,"version":"0.1.0","description":"Persistent service lifecycle and diagnostics","platform":true,"hosting":{"mode":"local"},"permissions":[],"publisher":"rhyven"})
}
pub fn dispatch(r: &Runtime, operation: &str, args: Value) -> Result<Value> {
    ensure(
        operation == "execute",
        "permission",
        "Runtime capability exposes actions only",
    )?;
    let action = args["action"].as_str().unwrap_or("");
    let def = manifest();
    ensure(
        def["actions"].get(action).is_some(),
        "not_found",
        "Unknown runtime action",
    )?;
    let input = schema::validate(args["args"].clone(), &def["actions"][action]["input"])?;
    control(
        r,
        action.trim_start_matches("service_"),
        input["app"].as_str(),
    )
}
pub fn base(root: &Path) -> Result<PathBuf> {
    let p = collections::home_for(root)?.unwrap_or(collections::state_dir(root)?);
    std::fs::create_dir_all(&p)?;
    Ok(std::fs::canonicalize(p)?)
}
pub(crate) fn directory(root: &Path) -> Result<PathBuf> {
    let p = collections::state_dir(root)?.join("services");
    std::fs::create_dir_all(&p)?;
    ensure(
        !std::fs::symlink_metadata(&p)?.file_type().is_symlink(),
        "permission",
        "Service directory cannot be a symlink",
    )?;
    Ok(p)
}
fn path(root: &Path, app: &str) -> Result<PathBuf> {
    Ok(directory(root)?.join(format!("{}.json", store::hash(&json!(app)))))
}
pub(crate) fn state(root: &Path, app: &str) -> Result<Value> {
    let path = path(root, app)?;
    if !path.try_exists()? {
        return Ok(
            json!({"app":app,"desired":"stopped","state":"stopped","explicitly_stopped":false,"suspended":false}),
        );
    }
    ensure(
        !std::fs::symlink_metadata(&path)?.file_type().is_symlink()
            && std::fs::metadata(&path)?.len() <= MAX as u64,
        "integrity",
        "Invalid service state file",
    )?;
    let v: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure(
        v["app"] == app,
        "integrity",
        "Service state identity mismatch",
    )?;
    Ok(v)
}
pub(crate) fn save(root: &Path, app: &str, value: &Value) -> Result<()> {
    let path = path(root, app)?;
    store::write(&path, value)?;
    #[cfg(unix)]
    std::fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
pub(crate) fn states(root: &Path) -> Result<Vec<Value>> {
    let mut out = vec![];
    for entry in std::fs::read_dir(directory(root)?)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|v| v == "json") {
            ensure(
                entry.file_type()?.is_file() && entry.metadata()?.len() <= MAX as u64,
                "integrity",
                "Invalid service descriptor",
            )?;
            let v: Value = serde_json::from_slice(&std::fs::read(entry.path())?)?;
            let app = v["app"].as_str().unwrap_or("");
            ensure(
                catalog::app_name(app) && entry.path() == path(root, app)?,
                "integrity",
                "Invalid service descriptor identity",
            )?;
            out.push(v);
        }
    }
    Ok(out)
}
pub(crate) fn package(r: &Runtime, app: &str) -> Result<Value> {
    let mut p = r.describe(app)?;
    p.as_object_mut().unwrap().remove("trust");
    p.as_object_mut().unwrap().remove("trust_evidence");
    ensure(
        enabled(&p),
        "validation",
        "App is not a persistent container service",
    )?;
    Ok(p)
}
pub(crate) fn name(root: &Path, app: &str) -> Result<String> {
    Ok(format!(
        "rhyven-svc-{}",
        &store::hash(&json!([std::fs::canonicalize(root)?, app]))[..32]
    ))
}
pub(crate) fn audit(r: &Runtime, app: &str, operation: &str) -> Result<()> {
    store::open(&r.root)?.execute(
        "INSERT INTO events(app,event) VALUES(?1,?2)",
        params![
            app,
            json!({"operation":operation,"actor":r.actor,"time":crate::marketplace::now()})
                .to_string()
        ],
    )?;
    Ok(())
}
fn docker(args: &[&str], timeout: u64) -> Result<Vec<u8>> {
    container::docker(
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        vec![],
        timeout,
    )
}
/// A successful empty listing proves absence; an unreachable engine does not.
pub(crate) fn inspect(name: &str) -> Result<Option<Value>> {
    let filter = format!("name=^/{name}$");
    let out = docker(
        &[
            "container",
            "ls",
            "-a",
            "--filter",
            &filter,
            "--format",
            "{{.ID}}",
        ],
        15,
    )?;
    if out.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let v: Value = serde_json::from_slice(&docker(&["container", "inspect", name], 15)?)?;
    ensure(
        v[0]["Config"]["Labels"]["rhyven.service"] == name,
        "permission",
        "Container name belongs to a different owner; refusing to stop it",
    )?;
    Ok(Some(v[0].clone()))
}
pub(crate) fn halt(root: &Path, app: &str, timeout: u64, clean: bool) -> Result<()> {
    let name = name(root, app)?;
    let Some(v) = inspect(&name)? else {
        return Ok(());
    };
    if v["State"]["Running"] == true {
        docker(
            &["stop", "--time", &timeout.to_string(), &name],
            timeout + 15,
        )?;
    }
    let v = inspect(&name)?;
    ensure(
        v.as_ref().is_none_or(|v| v["State"]["Running"] == false),
        "service_unavailable",
        "Cannot confirm service termination",
    )?;
    if clean {
        ensure(v.as_ref().is_none_or(|v| v["State"]["ExitCode"] != 137 && v["State"]["OOMKilled"] != true), "service_unclean_stop", "Service required a forced stop; restart and stop it cleanly before a consistent backup/update")?;
    }
    if v.is_some() {
        docker(&["rm", &name], 15)?;
    }
    Ok(())
}
/// Caller holds collection maintenance lock. No service thread is joined here.
pub(crate) fn stop_locked(
    r: &Runtime,
    app: &str,
    clean: bool,
    suspension: Option<&str>,
) -> Result<Value> {
    let mut s = state(&r.root, app)?;
    let timeout = s["shutdown_timeout_seconds"].as_u64().unwrap_or(10);
    let had_process = s.get("generation").is_some();
    if had_process {
        s["generation"] = json!(uuid::Uuid::new_v4().simple().to_string());
    }
    s["suspended"] = json!(suspension.is_some());
    s["suspension_reason"] = json!(suspension);
    if suspension.is_none() {
        s["desired"] = json!("stopped");
        s["explicitly_stopped"] = json!(true);
    }
    s["state"] = json!("stopping");
    // Revoke callback admission before asking the process to stop.
    save(&r.root, app, &s)?;
    if had_process {
        if let Err(e) = halt(&r.root, app, timeout, clean) {
            s["state"] = json!("unavailable");
            s["error"] = json!(e.to_string());
            save(&r.root, app, &s)?;
            return Err(e);
        }
    }
    s["state"] = json!(if suspension.is_some() {
        "maintenance"
    } else {
        "stopped"
    });
    save(&r.root, app, &s)?;
    audit(
        r,
        app,
        if suspension.is_some() {
            "service_suspend"
        } else {
            "service_stop"
        },
    )?;
    Ok(s)
}
pub fn disable(r: &Runtime, app: &str) -> Result<()> {
    let _gate = crate::maintenance::lock(&r.root)?;
    if path(&r.root, app)?.exists() {
        stop_locked(r, app, false, None)?;
    }
    Ok(())
}
/// Suspend all continuous writers, including orphans when the supervisor is down.
pub struct Suspension {
    root: PathBuf,
    apps: Vec<String>,
}
impl Suspension {
    pub fn new(r: &Runtime) -> Result<Self> {
        let mut result = Self {
            root: r.root.clone(),
            apps: vec![],
        };
        for s in states(&r.root)? {
            if s["suspended"] == true {
                // A previous stop may have failed while Docker was unavailable.
                // A persisted fence alone does not prove the old writer exited.
                if s.get("generation").is_some() {
                    halt(
                        &r.root,
                        s["app"].as_str().unwrap(),
                        s["shutdown_timeout_seconds"].as_u64().unwrap_or(10),
                        true,
                    )?;
                }
                continue;
            }
            let app = s["app"].as_str().unwrap();
            if s.get("generation").is_some() {
                stop_locked(r, app, true, Some("maintenance"))?;
                result.apps.push(app.into());
            }
        }
        Ok(result)
    }
}
impl Drop for Suspension {
    fn drop(&mut self) {
        for app in &self.apps {
            if let Ok(mut s) = state(&self.root, app) {
                s["suspended"] = json!(false);
                s["suspension_reason"] = Value::Null;
                s["state"] = json!("stopped");
                let _ = save(&self.root, app, &s);
            }
        }
    }
}
pub fn control(r: &Runtime, operation: &str, app: Option<&str>) -> Result<Value> {
    if operation == "list" {
        let _gate = crate::maintenance::lock(&r.root)?;
        let apps = r.apps()?;
        let mut services = vec![];
        for app in apps.as_array().unwrap().iter().filter(|p| enabled(p)) {
            services.push(state(&r.root, app["name"].as_str().unwrap())?);
        }
        return Ok(json!({"scope":collections::scope(&r.root)?,"services":services}));
    }
    let app = app.ok_or_else(|| Error::new("validation", "app required"))?;
    ensure(catalog::app_name(app), "validation", "Invalid app name")?;
    if operation == "stop" || operation == "status" {
        let mut s = {
            let _gate = crate::maintenance::lock(&r.root)?;
            package(r, app)?;
            if operation == "stop" {
                return stop_locked(r, app, false, None);
            }
            state(&r.root, app)?
        };
        // Never wait for supervisor IPC while holding the collection gate: its
        // reconciliation loop needs that gate to restart a suspended service.
        s["supervisor_available"] = json!(request(r, json!({"op":"ping"})).is_ok());
        if s["supervisor_available"] == false && s["desired"] == "running" {
            s["last_observed_state"] = s["state"].clone();
            s["state"] = json!("unavailable");
        }
        s["scope"] = collections::scope(&r.root)?;
        return Ok(s);
    }
    request(r, json!({"op":operation,"app":app}))
}
pub fn call(r: &Runtime, p: &Value, args: &Value) -> Result<Value> {
    let action = args["action"].as_str().unwrap_or("");
    ensure(
        p["actions"].get(action).is_some(),
        "not_found",
        "Unknown service action",
    )?;
    schema::validate(args["args"].clone(), &p["actions"][action]["input"])?;
    request(
        r,
        json!({"op":"call","app":p["name"],"package_sha256":store::hash(p),"arguments":args}),
    )
}
pub(crate) fn callback(r: &Runtime, p: &Value, generation: &str, message: &Value) -> Result<Value> {
    catalog::keys(message, &["type", "id", "category", "function", "args"])?;
    let _gate = crate::maintenance::try_lock(&r.root, Duration::from_millis(500))?;
    let current = state(&r.root, p["name"].as_str().unwrap())?;
    ensure(
        current["generation"] == generation
            && current["desired"] == "running"
            && current["state"] == "ready"
            && current["suspended"] != true,
        "permission",
        "Service session has been fenced or suspended",
    )?;
    ensure(
        store::hash(&package(r, p["name"].as_str().unwrap())?) == store::hash(p),
        "permission",
        "Service package changed",
    )?;
    let grant = p["execution"]["calls"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|g| g["category"] == message["category"] && g["function"] == message["function"])
        .ok_or_else(|| Error::new("permission", "Undeclared peer function"))?;
    let target = r.describe(grant["category"].as_str().unwrap())?;
    ensure(
        target["platform"] != true
            && target["hosting"]["mode"] == "local"
            && !container::enabled(&target)
            && target["version"] == grant["version"],
        "permission",
        "Peer must be a local declarative app at the granted version",
    )?;
    let actor = format!(
        "{PRINCIPAL}{}",
        &store::hash(&json!([r.root, p["name"]]))[..40]
    );
    let scoped = Runtime {
        root: r.root.clone(),
        actor,
    };
    scoped.call("rhyven_call", json!({"category":message["category"],"function":message["function"],"args":message["args"]}))
}
pub(crate) fn request(r: &Runtime, mut input: Value) -> Result<Value> {
    std::fs::create_dir_all(&r.root)?;
    input["root"] = json!(std::fs::canonicalize(&r.root)?);
    input["actor"] = json!(r.actor);
    #[cfg(unix)]
    {
        daemon::request(&base(&r.root)?, input)
    }
    #[cfg(not(unix))]
    {
        Err(Error::new(
            "service_unavailable",
            "Persistent service supervision currently requires Linux or macOS",
        ))
    }
}
pub fn run(r: &Runtime, max_services: usize, memory_mb: u64, cpus: u64) -> Result<()> {
    #[cfg(unix)]
    {
        daemon::run(&base(&r.root)?, max_services, memory_mb, cpus)
    }
    #[cfg(not(unix))]
    {
        Err(Error::new(
            "service_unavailable",
            "Persistent services require Linux or macOS",
        ))
    }
}
pub fn daemon_control(r: &Runtime, operation: &str) -> Result<Value> {
    request(r, json!({"op":operation}))
}

/// Isolated supervisor for explicitly authorized conformance/migration tests.
pub(crate) struct ScopedSupervisor {
    runtime: Runtime,
    thread: Option<std::thread::JoinHandle<Result<()>>>,
}
impl ScopedSupervisor {
    pub(crate) fn start(r: &Runtime, app: &str) -> Result<Self> {
        let runtime = r.clone();
        let thread = std::thread::spawn(move || run(&runtime, 1, 32768, 32));
        let guard = Self {
            runtime: r.clone(),
            thread: Some(thread),
        };
        for _ in 0..100 {
            if daemon_control(r, "ping").is_ok() {
                control(r, "start", Some(app))?;
                return Ok(guard);
            }
            if guard.thread.as_ref().unwrap().is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(Error::new(
            "service_unavailable",
            "Isolated service supervisor could not start",
        ))
    }
}
impl Drop for ScopedSupervisor {
    fn drop(&mut self) {
        let _ = daemon_control(&self.runtime, "shutdown");
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../examples/container-service-python/app.json"
        ))
        .unwrap()
    }
    #[test]
    fn service_contract_fails_closed_and_keeps_three_tools() {
        let p = fixture();
        catalog::validate(&p).unwrap();
        let mut bad = p.clone();
        bad["execution"]["protocol"] = json!("rhyven.container/1");
        assert!(catalog::validate(&bad).is_err());
        let mut bad = p.clone();
        bad["permissions"]
            .as_array_mut()
            .unwrap()
            .retain(|s| s != "service.run");
        assert!(catalog::validate(&bad).is_err());
        let mut bad = p.clone();
        bad["execution"]["calls"][0]["category"] = json!("rhyven/marketplace");
        assert!(catalog::validate(&bad).is_err());
        bad["execution"]["calls"][0]["category"] = json!(APP);
        assert!(catalog::validate(&bad).is_err());
        let mut bad = p.clone();
        bad["execution"]["start_policy"] = json!("always");
        assert!(catalog::validate(&bad).is_err());
        let mut bad = p;
        bad["execution"]["mode"] = json!("action");
        assert!(catalog::validate(&bad).is_err());
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "test").unwrap();
        assert_eq!(
            crate::tools::AgentSession::new(r.clone(), &[])
                .unwrap()
                .definitions()
                .len(),
            3
        );
        assert!(r.call("rhyven_categories", json!({})).unwrap()["apps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == APP));
    }
    #[test]
    fn callbacks_are_scoped_fenced_and_attributed_to_unforgeable_service_actor() {
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "test").unwrap();
        let target = catalog::bundled()
            .into_iter()
            .find(|p| p["name"] == "rhyven/project-knowledge")
            .unwrap();
        r.install(&target, true, false).unwrap();
        let p = fixture();
        let app = p["name"].as_str().unwrap();
        // Install metadata only: this test exercises core authorization without Docker.
        store::open(&r.root)
            .unwrap()
            .execute(
                "INSERT INTO apps(name,package,digest) VALUES(?1,?2,?3)",
                params![app, p.to_string(), store::hash(&p)],
            )
            .unwrap();
        let mut s = json!({"app":app,"generation":"first","desired":"running","state":"ready","suspended":false});
        save(&r.root, app, &s).unwrap();
        let message = json!({"type":"callback","id":"1","category":"rhyven/project-knowledge","function":"action_remember","args":{"title":"A finding","body":"Callback authorization","topic":"test","request_id":"note-1"}});
        let note = callback(&r, &p, "first", &message).unwrap();
        assert!(note["updated_by"].as_str().unwrap().starts_with(PRINCIPAL));
        assert!(Runtime::new(&r.root, note["updated_by"].as_str().unwrap()).is_err());
        assert_eq!(callback(&r, &p, "first", &message).unwrap(), note);
        let mut denied = message.clone();
        denied["function"] = json!("object_note_create");
        assert_eq!(
            callback(&r, &p, "first", &denied).unwrap_err().code,
            "permission"
        );
        let mut denied = message.clone();
        denied["actor"] = json!("admin");
        assert!(callback(&r, &p, "first", &denied).is_err());
        s["generation"] = json!("second");
        save(&r.root, app, &s).unwrap();
        assert_eq!(
            callback(&r, &p, "first", &message).unwrap_err().code,
            "permission"
        );
        s["suspended"] = json!(true);
        save(&r.root, app, &s).unwrap();
        assert!(callback(&r, &p, "second", &message).is_err());
    }
}
