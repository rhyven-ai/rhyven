//! Pinned, bounded cross-app actions. Every child uses the ordinary runtime path.
use crate::{catalog, error::ensure, schema, store, Error, Result, Runtime};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn enabled(p: &Value) -> bool {
    p.get("dependencies").is_some()
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    crate::runtime::string(v, k)
}
fn hash(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
}
pub fn validate(p: &Value) -> Result<()> {
    if !enabled(p) {
        return Ok(());
    }
    ensure(
        p["hosting"]["mode"] == "local" && !crate::execution::enabled(p),
        "package",
        "Stacks require local declarative execution",
    )?;
    let deps = p["dependencies"]
        .as_object()
        .ok_or_else(|| Error::new("package", "dependencies must be an object"))?;
    ensure(
        deps.len() <= 16,
        "package",
        "At most 16 pinned dependencies",
    )?;
    for (alias, d) in deps {
        catalog::keys(d, &["app", "version", "sha256"])?;
        ensure(
            schema::name(alias) && catalog::app_name(text(d, "app")?) && d["app"] != p["name"],
            "package",
            "Invalid or self dependency",
        )?;
        catalog::version(text(d, "version")?)?;
        ensure(
            hash(&d["sha256"]),
            "package",
            "Dependency sha256 is required",
        )?;
    }
    Ok(())
}
pub fn validate_action(p: &Value, a: &Value) -> Result<()> {
    catalog::keys(
        a,
        &[
            "description",
            "keywords",
            "input",
            "output",
            "operation",
            "steps",
            "result",
        ],
    )?;
    ensure(enabled(p), "package", "Stack requires dependencies")?;
    ensure(
        a["description"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "package",
        "Stack description required",
    )?;
    schema::check(&a["input"], 0)?;
    schema::check(&a["output"], 0)?;
    ensure(
        a["input"]["type"] == "object",
        "package",
        "Stack input must be object",
    )?;
    let steps = a["steps"]
        .as_array()
        .ok_or_else(|| Error::new("package", "Stack steps required"))?;
    ensure(
        !steps.is_empty() && steps.len() <= 32,
        "package",
        "Stack requires 1..32 steps",
    )?;
    let mut seen = BTreeSet::new();
    for s in steps {
        catalog::keys(
            s,
            &["id", "dependency", "action", "args", "when", "foreach"],
        )?;
        let id = text(s, "id")?;
        ensure(
            schema::name(id) && !seen.contains(id),
            "package",
            "Invalid or duplicate step ID",
        )?;
        ensure(
            p["dependencies"].get(text(s, "dependency")?).is_some()
                && schema::name(text(s, "action")?),
            "package",
            "Unknown dependency or action",
        )?;
        for key in ["args", "when", "foreach"] {
            if let Some(v) = s.get(key) {
                bindings(v, &seen, &a["input"], 0)?;
            }
        }
        ensure(s.get("args").is_some(), "package", "Step args required")?;
        seen.insert(id.to_owned());
    }
    ensure(
        a.get("result").is_some(),
        "package",
        "Stack result required",
    )?;
    bindings(&a["result"], &seen, &a["input"], 0)
}
fn bindings(v: &Value, seen: &BTreeSet<String>, input: &Value, depth: usize) -> Result<()> {
    ensure(depth <= 16, "package", "Binding nesting exceeds 16")?;
    if let Some(m) = v.as_object() {
        if m.keys().any(|k| k.starts_with('$')) {
            if let Some(expr) = v.get("$expr") {
                catalog::keys(v, &["$expr"])?;
                crate::expressions::check(expr, input, &json!({}), false, 0)?;
                return Ok(());
            } else if let Some(step) = v.get("$step") {
                catalog::keys(v, &["$step", "path"])?;
                ensure(
                    step.as_str().is_some_and(|s| seen.contains(s)),
                    "package",
                    "Forward or unknown step reference",
                )?;
            } else {
                catalog::keys(v, &["$input", "$item"])?;
                ensure(
                    m.len() == 1,
                    "package",
                    "Binding requires exactly one reference",
                )?;
            }
            let pointer = v
                .get("path")
                .or_else(|| v.get("$input"))
                .or_else(|| v.get("$item"));
            if let Some(ptr) = pointer {
                ensure(
                    ptr.as_str()
                        .is_some_and(|s| s.is_empty() || s.starts_with('/')),
                    "package",
                    "Bindings use JSON pointers",
                )?;
            }
            return Ok(());
        }
        for child in m.values() {
            bindings(child, seen, input, depth + 1)?;
        }
    } else if let Some(a) = v.as_array() {
        for child in a {
            bindings(child, seen, input, depth + 1)?;
        }
    }
    Ok(())
}
fn resolve(v: &Value, input: &Value, steps: &Value, item: &Value, actor: &str) -> Result<Value> {
    if let Some(m) = v.as_object() {
        if let Some(expr) = v.get("$expr") {
            return crate::expressions::eval(
                expr,
                input,
                &Value::Null,
                actor,
                crate::marketplace::now(),
                0,
            );
        }
        let target = if let Some(ptr) = v.get("$input") {
            Some((input, ptr.as_str().unwrap()))
        } else if let Some(ptr) = v.get("$item") {
            Some((item, ptr.as_str().unwrap()))
        } else if let Some(id) = v.get("$step") {
            Some((
                &steps[id.as_str().unwrap()],
                v["path"].as_str().unwrap_or(""),
            ))
        } else {
            None
        };
        if let Some((source, ptr)) = target {
            return source
                .pointer(ptr)
                .cloned()
                .ok_or_else(|| Error::new("validation", "Missing binding value"));
        }
        return m
            .iter()
            .map(|(k, v)| Ok((k.clone(), resolve(v, input, steps, item, actor)?)))
            .collect::<Result<serde_json::Map<_, _>>>()
            .map(Value::Object);
    }
    if let Some(a) = v.as_array() {
        return a
            .iter()
            .map(|v| resolve(v, input, steps, item, actor))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array);
    }
    Ok(v.clone())
}
/// Dependencies are installed separately after review. Never download or activate here.
pub fn check_dependencies(r: &Runtime, p: &Value) -> Result<()> {
    fn walk(r: &Runtime, p: &Value, path: &mut BTreeSet<String>, count: &mut usize) -> Result<()> {
        *count += 1;
        ensure(
            *count <= 32 && path.len() < 4,
            "package",
            "Stack dependency graph exceeds limits",
        )?;
        let name = text(p, "name")?.to_owned();
        ensure(
            path.insert(name.clone()),
            "package",
            "Cyclic stack dependency",
        )?;
        for (_, d) in p["dependencies"].as_object().into_iter().flatten() {
            let child = installed(r, text(d, "app")?)?;
            ensure(
                child.get("platform").is_none(),
                "permission",
                "Platform management cannot be called by stacks",
            )?;
            ensure(
                child["version"] == d["version"] && store::hash(&child) == d["sha256"],
                "version_conflict",
                "Dependency changed or is not the pinned version",
            )?;
            for permission in child["permissions"].as_array().unwrap() {
                ensure(
                    p["permissions"].as_array().unwrap().contains(permission),
                    "permission",
                    "Stack must declare all dependency permissions",
                )?;
            }
            walk(r, &child, path, count)?;
        }
        for a in p["actions"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, a)| a)
            .filter(|a| a["operation"] == "stack")
        {
            for s in a["steps"].as_array().unwrap() {
                let d = &p["dependencies"][text(s, "dependency")?];
                let child = installed(r, text(d, "app")?)?;
                ensure(
                    child["actions"].get(text(s, "action")?).is_some(),
                    "package",
                    "Stack target action does not exist",
                )?;
            }
        }
        path.remove(&name);
        Ok(())
    }
    walk(r, p, &mut BTreeSet::new(), &mut 0)
}
thread_local! {static DEPTH: std::cell::Cell<usize> = const {std::cell::Cell::new(0)}; static CALLS: std::cell::Cell<usize> = const {std::cell::Cell::new(0)}; static STARTED: std::cell::Cell<Option<std::time::Instant>> = const {std::cell::Cell::new(None)};}
struct Depth;
impl Drop for Depth {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}
pub fn call(r: &Runtime, p: &Value, args: &Value) -> Result<Value> {
    check_dependencies(r, p)?;
    let a = &p["actions"][text(args, "action")?];
    let input = schema::validate(args.get("args").cloned().unwrap_or(json!({})), &a["input"])?;
    let depth = DEPTH.with(|d| d.get());
    ensure(depth < 4, "validation", "Stack nesting exceeds 4")?;
    if depth == 0 {
        CALLS.with(|c| c.set(0));
        STARTED.with(|s| s.set(Some(std::time::Instant::now())));
    }
    DEPTH.with(|d| d.set(depth + 1));
    let _depth = Depth;
    let db = store::open(&r.root)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS stack_runs(id TEXT PRIMARY KEY, actor TEXT NOT NULL, fingerprint TEXT NOT NULL, report TEXT NOT NULL)")?;
    let id = args["request_id"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    ensure(
        !id.is_empty() && id.len() <= 128,
        "validation",
        "Invalid request_id",
    )?;
    let key = store::hash(&json!([r.actor, id]));
    let fingerprint = store::hash(&json!([p, args]));
    if let Some((old, raw)) = db
        .query_row(
            "SELECT fingerprint,report FROM stack_runs WHERE id=?1",
            [&key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        ensure(
            old == fingerprint,
            "idempotency_conflict",
            "Stack request ID reused with different arguments",
        )?;
        let report: Value = serde_json::from_str(&raw)?;
        ensure(
            report["status"] == "complete",
            "stack_incomplete",
            "Earlier run failed or may have side effects; inspect its run report before retrying",
        )?;
        return Ok(report["result"].clone());
    }
    let mut report = json!({"run_id":id,"app":p["name"],"action":args["action"],"package_hash":store::hash(p),"actor":r.actor,"status":"running","steps":[],"started_at":crate::marketplace::now()});
    db.execute(
        "INSERT INTO stack_runs VALUES(?1,?2,?3,?4)",
        params![key, r.actor, fingerprint, report.to_string()],
    )?;
    let save = |v: &Value| -> Result<()> {
        db.execute(
            "UPDATE stack_runs SET report=?2 WHERE id=?1",
            params![key, v.to_string()],
        )?;
        Ok(())
    };
    let result = (|| -> Result<Value> {
        let mut outputs = json!({});
        for step in a["steps"].as_array().unwrap() {
            let step_id = text(step, "id")?;
            if let Some(condition) = step.get("when") {
                let condition = resolve(condition, &input, &outputs, &Value::Null, &r.actor)?;
                ensure(
                    condition.is_boolean(),
                    "validation",
                    "Step condition must be boolean",
                )?;
                if condition == false {
                    outputs[step_id] = Value::Null;
                    continue;
                }
            }
            let items = if let Some(each) = step.get("foreach") {
                let items = resolve(each, &input, &outputs, &Value::Null, &r.actor)?;
                let items = items
                    .as_array()
                    .ok_or_else(|| Error::new("validation", "foreach must resolve to an array"))?;
                ensure(items.len() <= 32, "validation", "foreach exceeds 32 items")?;
                items.clone()
            } else {
                vec![Value::Null]
            };
            let mut values = vec![];
            for (index, item) in items.iter().enumerate() {
                let calls = CALLS.with(|c| {
                    c.set(c.get() + 1);
                    c.get()
                });
                ensure(
                    calls <= 64,
                    "validation",
                    "Stack exceeds 64 total child calls",
                )?;
                let bound = resolve(&step["args"], &input, &outputs, item, &r.actor)?;
                let dependency = &p["dependencies"][text(step, "dependency")?];
                let child = installed(r, text(dependency, "app")?)?;
                let remaining =
                    STARTED.with(|s| 300u64.saturating_sub(s.get().unwrap().elapsed().as_secs()));
                let maximum = child["execution"]["timeout_seconds"].as_u64().unwrap_or(
                    if crate::execution::enabled(&child)
                        || crate::connector::enabled(&child)
                        || child["hosting"]["mode"] != "local"
                    {
                        30
                    } else {
                        1
                    },
                );
                ensure(
                    remaining >= maximum && remaining > 0,
                    "timeout",
                    "Stack's five-minute dispatch budget exhausted",
                )?;
                let child_request = store::hash(&json!([key, step_id, index]));
                let record = json!({"id":step_id,"iteration":index,"dependency":dependency,"input_hash":store::hash(&bound),"request_id":child_request,"status":"running"});
                report["steps"].as_array_mut().unwrap().push(record);
                save(&report)?;
                let value=r.call("execute",json!({"app":dependency["app"],"action":step["action"],"args":bound,"request_id":child_request}))?;
                ensure(
                    value.to_string().len() <= 262144,
                    "validation",
                    "Step output exceeds 256 KiB",
                )?;
                let last = report["steps"].as_array_mut().unwrap().last_mut().unwrap();
                last["status"] = json!("complete");
                last["output_hash"] = json!(store::hash(&value));
                save(&report)?;
                values.push(value);
            }
            outputs[step_id] = if step.get("foreach").is_some() {
                json!(values)
            } else {
                values.remove(0)
            };
            ensure(
                outputs.to_string().len() <= 1048576,
                "validation",
                "Stack intermediate values exceed 1 MiB",
            )?;
        }
        schema::validate(
            resolve(&a["result"], &input, &outputs, &Value::Null, &r.actor)?,
            &a["output"],
        )
    })();
    report["finished_at"] = json!(crate::marketplace::now());
    match &result {
        Ok(value) => {
            report["status"] = json!("complete");
            report["result"] = value.clone();
        }
        Err(e) => {
            report["status"] = json!("failed_or_unknown");
            report["error"] = json!(e);
        }
    }
    save(&report)?;
    result
}
pub fn report(r: &Runtime, id: &str) -> Result<Value> {
    let db = store::open(&r.root)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS stack_runs(id TEXT PRIMARY KEY, actor TEXT NOT NULL, fingerprint TEXT NOT NULL, report TEXT NOT NULL)")?;
    let key = store::hash(&json!([r.actor, id]));
    let raw: String = db
        .query_row("SELECT report FROM stack_runs WHERE id=?1", [key], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or_else(|| Error::new("not_found", "No stack run for this actor and request ID"))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn installed(r: &Runtime, name: &str) -> Result<Value> {
    let mut p = r.describe(name)?;
    if let Some(m) = p.as_object_mut() {
        m.remove("trust");
        m.remove("trust_evidence");
    }
    Ok(p)
}
