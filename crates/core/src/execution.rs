//! Shared lifecycle for executable actions, independent of transport and process backend.
use crate::{catalog, collections, error::ensure, schema, store, Error, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

pub fn driver(p: &Value) -> &str {
    p["execution"]["driver"].as_str().unwrap_or("declarative")
}
pub fn enabled(p: &Value) -> bool {
    matches!(driver(p), "container" | "script")
}
pub fn validate(p: &Value) -> Result<()> {
    if crate::script::enabled(p) {
        crate::script::validate(p)
    } else {
        crate::container::validate_execution(p)
    }
}
pub fn prepare(root: &Path, p: &Value) -> Result<()> {
    if crate::script::enabled(p) {
        crate::script::prepare(root, p)
    } else {
        crate::container::prepare(p)
    }
}
fn code(p: &Value, kind: &str) -> &'static str {
    match (crate::script::enabled(p), kind) {
        (true, "incomplete") => "script_incomplete",
        (true, _) => "script_protocol",
        (false, "incomplete") => "container_incomplete",
        (false, _) => "container_protocol",
    }
}

pub(crate) fn instance(root: &Path, name: &str) -> Result<PathBuf> {
    let state = collections::state_dir(root)?;
    std::fs::create_dir_all(&state)?;
    let mut path = state;
    for part in ["containers".to_owned(), store::hash(&json!(name))] {
        path.push(part);
        std::fs::create_dir_all(&path)?;
        ensure(
            !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "container",
            "App state directory must not be a symlink",
        )?;
    }
    Ok(path)
}
pub fn lock(root: &Path, name: &str) -> Result<File> {
    let path = instance(root, name)?.join("instance.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}
pub fn call(root: &Path, p: &Value, actor: &str, arguments: &Value) -> Result<Value> {
    let _maintenance = crate::maintenance::lock(root)?;
    let name = p["name"].as_str().unwrap();
    let action = arguments["action"]
        .as_str()
        .ok_or_else(|| Error::new("validation", "Action required"))?;
    let definition = p["actions"]
        .get(action)
        .ok_or_else(|| Error::new("not_found", "Unknown action"))?;
    let input = schema::validate(
        arguments.get("args").cloned().unwrap_or(json!({})),
        &definition["input"],
    )?;
    let request = arguments
        .get("request_id")
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or_else(|| Error::new("validation", "Invalid request_id"))
        })
        .transpose()?;
    let _lock = lock(root, name)?;
    let db = store::open(root)?;
    let raw: String = db
        .query_row(
            "SELECT package FROM apps WHERE name=?1 AND active=1",
            [name],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::new("not_installed", name))?;
    ensure(
        collections::load(root, &serde_json::from_str(&raw)?)? == *p,
        "version_conflict",
        "App changed before execution; discover again",
    )?;
    let fingerprint = store::hash(&json!(["execute", arguments, store::hash(p)]));
    if let Some(request) = request {
        if let Some((old, result)) = db
            .query_row(
                "SELECT fingerprint,result FROM receipts WHERE actor=?1 AND request=?2",
                params![actor, request],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure(
                old == fingerprint,
                "idempotency_conflict",
                "request_id already used with different arguments or app version",
            )?;
            let receipt: Value = serde_json::from_str(&result)?;
            ensure(receipt["status"] == "complete", code(p, "incomplete"), "Previous invocation may have caused side effects; inspect app state before starting a new request")?;
            return Ok(receipt["result"].clone());
        }
    }
    drop(db);
    let instance_name = format!("rhyven-{}", uuid::Uuid::new_v4().simple());
    let command = if crate::script::enabled(p) {
        None
    } else {
        Some(crate::container::launch_command(
            root,
            p,
            &instance_name,
            false,
        )?)
    };
    let mut script = if crate::script::enabled(p) {
        Some(crate::script::launch(root, p)?)
    } else {
        None
    };
    let protocol = if script.is_some() {
        "rhyven.action/1"
    } else {
        "rhyven.container/1"
    };
    let data_dir = script
        .as_ref()
        .map(|s| s.data_dir.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/data".into());
    let e = &p["execution"];
    let payload = json!({"protocol":protocol,"category":name,"function":format!("action_{action}"),"args":input,"context":{"actor":actor,"request_id":request,"collection":collections::scope(root)?["collection"],"data_dir":data_dir}});
    let _cleanup = command
        .as_ref()
        .map(|_| crate::container::Cleanup(instance_name));
    if let Some(request) = request {
        let changed = store::open(root)?.execute(
            "INSERT OR IGNORE INTO receipts(actor,request,fingerprint,result) VALUES(?1,?2,?3,?4)",
            params![
                actor,
                request,
                fingerprint,
                json!({"status":"pending"}).to_string()
            ],
        )?;
        ensure(
            changed == 1,
            "idempotency_conflict",
            "request_id was claimed by another invocation",
        )?;
    }
    let bytes = format!("{payload}\n").into_bytes();
    let timeout = e["timeout_seconds"].as_u64().unwrap_or(30);
    let output = if let Some(ref mut invocation) = script {
        crate::script::run(&mut invocation.command, bytes, timeout)?
    } else {
        crate::container::docker(command.as_ref().unwrap(), bytes, timeout)?
    };
    let response: Value = serde_json::from_slice(&output).map_err(|_| {
        Error::new(
            code(p, "protocol"),
            "Expected one JSON response on stdout; log to stderr",
        )
    })?;
    catalog::keys(&response, &["result", "error"])?;
    ensure(
        response.get("result").is_some() != response.get("error").is_some(),
        code(p, "protocol"),
        "Expected exactly one result or error",
    )?;
    if response.get("error").is_some() {
        catalog::keys(&response["error"], &["code", "message"])?;
        ensure(
            response["error"]["code"].is_string() && response["error"]["message"].is_string(),
            code(p, "protocol"),
            "App errors require string code and message",
        )?;
        return Err(Error::new("app_error", response["error"].to_string()));
    }
    let result = schema::validate(response["result"].clone(), &definition["output"])?;
    let mut db = store::open(root)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some(request) = request {
        tx.execute(
            "UPDATE receipts SET result=?3 WHERE actor=?1 AND request=?2",
            params![
                actor,
                request,
                json!({"status":"complete","result":result}).to_string()
            ],
        )?;
    }
    tx.execute("INSERT INTO events(app,event) VALUES(?1,?2)", params![name,json!({"operation":"execute","action":action,"actor":actor,"request_id":request,"package_sha256":store::hash(p),"time":crate::marketplace::now()}).to_string()])?;
    tx.commit()?;
    Ok(result)
}
