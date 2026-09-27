//! Schema-driven app execution and transactional state updates.
use crate::{catalog, collections, error::ensure, schema, store, Error, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Runtime {
    pub root: PathBuf,
    pub actor: String,
}
impl Runtime {
    pub fn collection(home: impl AsRef<Path>, name: &str, actor: &str) -> Result<Self> {
        Self::new(collections::create(home.as_ref(), name)?, actor)
    }
    pub fn new(root: impl AsRef<Path>, actor: &str) -> Result<Self> {
        ensure(
            !actor.is_empty()
                && actor.len() <= 128
                && !actor.starts_with(crate::services::PRINCIPAL),
            "validation",
            "Actor required, max 128 characters",
        )?;
        let root = std::path::absolute(root)?;
        Ok(Self {
            root,
            actor: actor.into(),
        })
    }
    pub fn init(&self) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        store::open(&self.root)?;
        Ok(
            json!({"workspace":self.root,"scope":collections::scope(&self.root)?,"format":2,"apps":self.apps()?}),
        )
    }
    pub fn apps(&self) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        let db = store::open(&self.root)?;
        let mut stmt = db.prepare("SELECT package FROM apps WHERE active=1 ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut apps = vec![];
        for row in rows {
            let p = collections::load(&self.root, &serde_json::from_str(&row?)?)?;
            apps.push(summary(&p));
        }
        Ok(json!(apps))
    }
    pub fn catalog(&self) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        let installed = self.apps()?;
        Ok(json!(catalog::list(&self.root)?
            .into_iter()
            .map(|p| {
                let mut s = summary(&p);
                s["installed"] = json!(installed
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["name"] == p["name"] && a["version"] == p["version"]));
                s
            })
            .collect::<Vec<_>>()))
    }
    pub fn describe(&self, name: &str) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        if name == crate::marketplace::APP {
            return Ok(crate::marketplace::describe());
        }
        if name == crate::services::APP {
            return Ok(crate::services::manifest());
        }
        let db = store::open(&self.root)?;
        let mut p = package(&self.root, &db, name)?;
        p["trust"] = json!("Unverified");
        p["trust_evidence"] =
            json!("Publisher name is self-declared; no signature or certification verified");
        Ok(p)
    }
    pub fn install(&self, p: &Value, accepted: bool, update: bool) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        if update {
            ensure(
                accepted,
                "permission_review_required",
                "Review permissions before updating",
            )?;
            let _services = crate::services::Suspension::new(self)?;
            return crate::updates::install(self, p, accepted);
        }
        let retained: Option<(String, bool)> = store::open(&self.root)?
            .query_row(
                "SELECT package,active FROM apps WHERE name=?1",
                [p["name"].as_str().unwrap_or("")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((raw, false)) = retained {
            if collections::load(&self.root, &serde_json::from_str(&raw)?)? != *p {
                ensure(
                    accepted,
                    "permission_review_required",
                    "Review permissions before reinstalling a different version",
                )?;
                let _services = crate::services::Suspension::new(self)?;
                return crate::updates::install(self, p, accepted);
            }
        }
        self.install_direct(p, accepted, update)
    }
    pub(crate) fn install_direct(&self, p: &Value, accepted: bool, update: bool) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        ensure(
            p["name"] != crate::marketplace::APP && p["name"] != crate::services::APP,
            "permission",
            "Reserved platform capability",
        )?;
        catalog::validate(p)?;
        ensure(accepted, "permission_review_required", format!("Review package, hosting disclosures and permissions; explicitly accept to install: {}", p["permissions"]))?;
        let _instance_lock = if crate::container::enabled(p) {
            Some(crate::container::lock(
                &self.root,
                p["name"].as_str().unwrap(),
            )?)
        } else {
            None
        };
        let mut db = store::open(&self.root)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let name = p["name"].as_str().unwrap();
        if let Some(old) = tx
            .query_row("SELECT package FROM apps WHERE name=?1", [name], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
        {
            let old = collections::load(&self.root, &serde_json::from_str(&old)?)?;
            if old == *p {
                crate::container::prepare(p)?;
                tx.execute("UPDATE apps SET active=1 WHERE name=?1", [name])?;
                tx.commit()?;
                return Ok(summary(p));
            }
            let active: bool =
                tx.query_row("SELECT active FROM apps WHERE name=?1", [name], |r| {
                    r.get(0)
                })?;
            ensure(
                update || !active,
                "already_installed",
                "Use update for version changes",
            )?;
            ensure(
                catalog::version(p["version"].as_str().unwrap())?
                    > catalog::version(old["version"].as_str().unwrap())?,
                "version",
                "Upgrade requires newer version",
            )?;
            if crate::updates::migration(&old, p)?.is_none() {
                compatible(&old, p)?;
            }
        } else {
            ensure(!update, "not_installed", "Install app first")?;
        }
        crate::container::prepare(p)?;
        let reference = collections::save(&self.root, p)?;
        tx.execute("INSERT INTO apps(name,package,digest) VALUES(?1,?2,?3) ON CONFLICT(name) DO UPDATE SET package=excluded.package,digest=excluded.digest,active=1", params![name,reference.to_string(),store::hash(p)])?;
        tx.execute("INSERT INTO events(app,event) VALUES(?1,?2)", params![name,json!({"operation":if update {"upgrade"} else {"install"},"actor":self.actor,"time":now(),"package_sha256":store::hash(p),"version":p["version"],"granted":p["permissions"]}).to_string()])?;
        tx.commit()?;
        Ok(summary(p))
    }
    pub fn uninstall(&self, name: &str) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        ensure(
            name != crate::marketplace::APP && name != crate::services::APP,
            "permission",
            "Reserved platform capability",
        )?;
        crate::services::disable(self, name)?;
        let _instance_lock = if self
            .describe(name)
            .is_ok_and(|p| crate::container::enabled(&p))
        {
            Some(crate::container::lock(&self.root, name)?)
        } else {
            None
        };
        let db = store::open(&self.root)?;
        db.execute("UPDATE apps SET active=0 WHERE name=?1", [name])?;
        Ok(json!({"uninstalled":name,"data_retained":true}))
    }
    pub fn call(&self, operation: &str, args: Value) -> Result<Value> {
        ensure(
            args.to_string().len() <= 1_048_576,
            "validation",
            "Request exceeds 1 MiB",
        )?;
        match operation {
            "rhyven_categories" => {
                return Ok(
                    json!({"rhyven_protocol":1,"collection":collections::scope(&self.root)?["collection"],"workspace":self.root,"apps":self.call("list_apps", args)?}),
                )
            }
            "rhyven_describe" => {
                catalog::keys(&args, &["category"])?;
                let mut manifest =
                    crate::tools::manifest(&self.describe(string(&args, "category")?)?);
                manifest["scope"] = collections::scope(&self.root)?;
                return Ok(manifest);
            }
            "rhyven_call" => return crate::tools::invoke(self, args),
            _ => (),
        }
        if args["app"] == crate::services::APP {
            catalog::keys(&args, &["app", "action", "args", "request_id"])?;
            return crate::services::dispatch(self, operation, args);
        }
        let _maintenance = crate::maintenance::lock(&self.root)?;
        match operation {
            "list_apps" => {
                catalog::keys(&args, &[])?;
                let mut apps = self.apps()?;
                apps.as_array_mut()
                    .unwrap()
                    .push(crate::marketplace::summary());
                apps.as_array_mut()
                    .unwrap()
                    .push(crate::services::summary());
                if collections::home_for(&self.root)?.is_some() {
                    for app in apps.as_array_mut().unwrap() {
                        app["scope"] = collections::scope(&self.root)?;
                    }
                }
                return Ok(apps);
            }
            "describe_app" => {
                catalog::keys(&args, &["app"])?;
                return self.describe(string(&args, "app")?);
            }
            "query" => catalog::keys(
                &args,
                &[
                    "app",
                    "object",
                    "filters",
                    "where",
                    "order_by",
                    "limit",
                    "offset",
                    "search",
                    "current_only",
                    "metadata",
                ],
            )?,
            "get" => catalog::keys(&args, &["app", "object", "id"])?,
            "create" => catalog::keys(&args, &["app", "object", "data", "request_id"])?,
            "update" => catalog::keys(
                &args,
                &[
                    "app",
                    "object",
                    "id",
                    "patch",
                    "expected_revision",
                    "request_id",
                ],
            )?,
            "execute" => catalog::keys(&args, &["app", "action", "args", "request_id"])?,
            _ => return Err(Error::new("unknown_operation", operation)),
        }
        if args["app"] == crate::marketplace::APP {
            ensure(
                ["where", "order_by", "search", "current_only", "metadata"]
                    .iter()
                    .all(|k| args.get(k).is_none()),
                "validation",
                "Marketplace queries use listing filters",
            )?;
            return crate::marketplace::call(self, operation, args);
        }
        let mut db = store::open(&self.root)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let p = package(&self.root, &tx, string(&args, "app")?)?;
        ensure(
            p["hosting"]["mode"] == "local"
                || ["where", "order_by", "search", "current_only", "metadata"]
                    .iter()
                    .all(|k| args.get(k).is_none()),
            "validation",
            "Rich queries currently require local hosting",
        )?;
        if operation == "execute" && crate::container::enabled(&p) {
            drop(tx);
            if crate::services::enabled(&p) {
                drop(_maintenance);
                return crate::services::call(self, &p, &args);
            }
            return crate::container::call(&self.root, &p, &self.actor, &args);
        }
        let mut actual_op = operation.to_owned();
        let mut actual = args.clone();
        let mut action = None;
        if operation == "execute" {
            let name = string(&args, "action")?;
            let a = p["actions"]
                .get(name)
                .ok_or_else(|| Error::new("not_found", "Unknown action"))?;
            let input =
                schema::validate(args.get("args").cloned().unwrap_or(json!({})), &a["input"])?;
            actual_op = a["operation"].as_str().unwrap().into();
            actual = json!({"app":p["name"],"object":a["object"]});
            if let Some(id) = input.get(a["id_arg"].as_str().unwrap_or("id")) {
                actual["id"] = id.clone();
            }
            if let Some(rev) = input.get("expected_revision") {
                actual["expected_revision"] = rev.clone();
            }
            let set = substitute(a.get("set").unwrap_or(&json!({})), &input)?;
            actual[match actual_op.as_str() {
                "create" => "data",
                "query" => "filters",
                _ => "patch",
            }] = set;
            action = Some((a.clone(), input));
        }
        let object_name = string(&actual, "object")?.to_owned();
        let object_name = object_name.as_str();
        let object = p["objects"]
            .get(object_name)
            .ok_or_else(|| Error::new("not_found", "Unknown object"))?;
        let writing = actual_op == "create" || actual_op == "update";
        let permission = if writing { "state.write" } else { "state.read" };
        ensure(
            p["permissions"]
                .as_array()
                .unwrap()
                .contains(&json!(permission)),
            "permission",
            format!("App lacks {permission}"),
        )?;
        let request = args
            .get("request_id")
            .map(|v| {
                v.as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128)
                    .ok_or_else(|| Error::new("validation", "Invalid request_id"))
            })
            .transpose()?;
        let fingerprint = store::hash(&json!([operation, args]));
        if let Some(request) = request {
            if let Some((previous, result)) = tx
                .query_row(
                    "SELECT fingerprint,result FROM receipts WHERE actor=?1 AND request=?2",
                    params![self.actor, request],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()?
            {
                ensure(
                    previous == fingerprint,
                    "idempotency_conflict",
                    "request_id already used for different arguments",
                )?;
                return Ok(serde_json::from_str(&result)?);
            }
        }
        // Validate at the client boundary before either local execution or remote forwarding.
        if actual_op == "create"
            && action
                .as_ref()
                .is_none_or(|(a, _)| a.get("expressions").is_none())
        {
            actual["data"] = schema::validate(actual["data"].clone(), &object["schema"])?;
        }
        if actual_op == "update" {
            validate_patch(&actual["patch"], &object["schema"])?;
            ensure(
                actual["expected_revision"].as_u64().is_some_and(|n| n > 0),
                "revision_required",
                "Positive expected_revision required",
            )?;
        }
        if actual_op == "query" {
            validate_filters(&actual, object)?;
        }
        if p["hosting"]["mode"] != "local" {
            // Release SQLite lock during network calls. Remote atomicity/idempotency is provider-owned.
            drop(tx);
            return remote(&p, operation, &args, &self.actor);
        }
        let result = match actual_op.as_str() {
            "get" => get(
                &tx,
                string(&actual, "app")?,
                object_name,
                string(&actual, "id")?,
            )?,
            "query" => query(&tx, &actual, object)?,
            "create" | "update" => {
                let name = p["name"].as_str().unwrap();
                let old = if actual_op == "update" {
                    Some(get(&tx, name, object_name, string(&actual, "id")?)?)
                } else {
                    None
                };
                if let Some(ref old) = old {
                    ensure(
                        object["immutable"] != true,
                        "immutable",
                        "Object is append-only; create a correction",
                    )?;
                    ensure(
                        actual["expected_revision"] == old["revision"],
                        "revision_conflict",
                        "Read current record then retry with its revision",
                    )?;
                    if let Some((ref a, ref input)) = action {
                        let guard = substitute(a.get("guard").unwrap_or(&json!({})), input)?;
                        for (key, value) in guard.as_object().unwrap() {
                            ensure(
                                old["data"][key] == *value,
                                "guard_failed",
                                format!("Action guard failed: {key}"),
                            )?;
                        }
                    }
                }
                if let Some((ref a, ref input)) = action {
                    let current = old.as_ref().map(|r| r["data"].clone()).unwrap_or(json!({}));
                    let timestamp = now();
                    if let Some(condition) = a.get("condition") {
                        ensure(
                            crate::expressions::eval(
                                condition,
                                input,
                                &current,
                                &self.actor,
                                timestamp,
                                0,
                            )? == true,
                            "guard_failed",
                            "Action condition failed",
                        )?;
                    }
                    let target = if actual_op == "create" {
                        "data"
                    } else {
                        "patch"
                    };
                    for (field, expr) in a["expressions"].as_object().into_iter().flatten() {
                        actual[target][field] = crate::expressions::eval(
                            expr,
                            input,
                            &current,
                            &self.actor,
                            timestamp,
                            0,
                        )?;
                    }
                }
                let mut data = old
                    .as_ref()
                    .map(|v| v["data"].clone())
                    .unwrap_or_else(|| actual["data"].clone());
                if old.is_some() {
                    for (key, value) in actual["patch"].as_object().unwrap() {
                        data[key] = value.clone();
                    }
                }
                data = schema::validate(data, &object["schema"])?;
                if action.is_none() {
                    for field in object["protected_fields"].as_array().into_iter().flatten() {
                        let key = field.as_str().unwrap();
                        let expected = old
                            .as_ref()
                            .map(|v| &v["data"][key])
                            .unwrap_or(&object["schema"]["properties"][key]["default"]);
                        ensure(
                            data[key] == *expected,
                            "protected_field",
                            format!("Use an action to change {key}"),
                        )?;
                    }
                }
                if let Some(ref old) = old {
                    for (field, states) in object["transitions"].as_object().into_iter().flatten() {
                        if old["data"][field] != data[field] {
                            let from = old["data"][field].as_str().unwrap_or("");
                            ensure(
                                states[from]
                                    .as_array()
                                    .is_some_and(|targets| targets.contains(&data[field])),
                                "transition",
                                format!("Forbidden transition for {field}"),
                            )?;
                        }
                    }
                }
                for (field, target) in object["relationships"].as_object().into_iter().flatten() {
                    if let Some(id) = data
                        .get(field)
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        let target_app = target["app"].as_str().unwrap_or(name);
                        let target_package = package(&self.root, &tx, target_app)?;
                        ensure(
                            target_package["hosting"]["mode"] == "local",
                            "relationship",
                            "Cross-remote relationships cannot be validated locally",
                        )?;
                        get(&tx, target_app, string(target, "object")?, id).map_err(|_| {
                            Error::new(
                                "relationship",
                                format!("Missing relationship target: {field}"),
                            )
                        })?;
                    }
                }
                let id = old
                    .as_ref()
                    .map(|v| v["id"].as_str().unwrap().to_owned())
                    .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
                let revision = old
                    .as_ref()
                    .map(|v| v["revision"].as_u64().unwrap() + 1)
                    .unwrap_or(1);
                let record = json!({"id":id,"app":name,"object":object_name,"revision":revision,"data":data,"created_at":old.as_ref().map(|v|v["created_at"].clone()).unwrap_or(json!(now())),"updated_at":now(),"updated_by":self.actor});
                tx.execute("INSERT INTO records(app,object,id,record) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET record=excluded.record",params![name,object_name,id,record.to_string()])?;
                tx.execute("INSERT INTO events(app,event) VALUES(?1,?2)",params![name,json!({"operation":operation,"action":args.get("action"),"actor":self.actor,"record":record,"package_sha256":store::hash(&p),"request_id":request}).to_string()])?;
                record
            }
            _ => unreachable!(),
        };
        if let Some(request) = request {
            tx.execute(
                "INSERT INTO receipts(actor,request,fingerprint,result) VALUES(?1,?2,?3,?4)",
                params![self.actor, request, fingerprint, result.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(result)
    }
    pub fn snapshot(&self) -> Result<Value> {
        let _maintenance = crate::maintenance::lock(&self.root)?;
        let mut db = store::open(&self.root)?;
        let tx = db.transaction()?;
        let mut result = json!({"format":2,"apps":[],"records":[],"events":[]});
        for (key, sql) in [
            ("apps", "SELECT package FROM apps ORDER BY name"),
            ("records", "SELECT record FROM records ORDER BY id"),
            ("events", "SELECT event FROM events ORDER BY seq"),
        ] {
            let mut stmt = tx.prepare(sql)?;
            let values = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for v in values {
                let value: Value = serde_json::from_str(&v?)?;
                result[key].as_array_mut().unwrap().push(if key == "apps" {
                    collections::load(&self.root, &value)?
                } else {
                    value
                });
            }
        }
        tx.commit()?;
        Ok(result)
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| Error::new("validation", format!("String {key} required")))
}
fn summary(p: &Value) -> Value {
    json!({"name":p["name"],"display_name":catalog::display_name(p),"version":p["version"],"description":p["description"],"hosting":p["hosting"],"execution":p.get("execution").cloned().unwrap_or(json!({"driver":"declarative"})),"permissions":p["permissions"],"trust":"Unverified","publisher":p["publisher"],"publisher_label":catalog::publisher_label(p),"sha256":store::hash(p)})
}
fn package(root: &Path, db: &Connection, name: &str) -> Result<Value> {
    let (raw, digest): (String, String) = db
        .query_row(
            "SELECT package,digest FROM apps WHERE name=?1 AND active=1",
            [name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::new("not_installed", name))?;
    let p = collections::load(root, &serde_json::from_str(&raw)?)?;
    ensure(
        store::hash(&p) == digest,
        "integrity",
        "Installed package checksum mismatch",
    )?;
    catalog::validate(&p)?;
    Ok(p)
}
fn get(db: &Connection, app: &str, object: &str, id: &str) -> Result<Value> {
    let raw: String = db
        .query_row(
            "SELECT record FROM records WHERE app=?1 AND object=?2 AND id=?3",
            params![app, object, id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::new("not_found", "Record not found in requested app/object"))?;
    Ok(serde_json::from_str(&raw)?)
}
fn validate_patch(patch: &Value, s: &Value) -> Result<()> {
    let mut partial = s.clone();
    partial["required"] = json!([]);
    for field in partial["properties"].as_object_mut().unwrap().values_mut() {
        field.as_object_mut().unwrap().remove("default");
    }
    schema::validate(patch.clone(), &partial)?;
    Ok(())
}
fn validate_filters(args: &Value, object: &Value) -> Result<()> {
    crate::query::validate(args, object)?;
    validate_patch(args.get("filters").unwrap_or(&json!({})), &object["schema"])?;
    for (field, max) in [("limit", 1000), ("offset", 1_000_000)] {
        if let Some(v) = args.get(field) {
            ensure(
                v.as_u64().is_some_and(|n| n <= max),
                "validation",
                format!("Invalid {field}"),
            )?;
        }
    }
    Ok(())
}
fn query(db: &Connection, args: &Value, object: &Value) -> Result<Value> {
    validate_filters(args, object)?;
    let mut stmt =
        db.prepare("SELECT record FROM records WHERE app=?1 AND object=?2 ORDER BY id")?;
    let rows = stmt.query_map(
        params![string(args, "app")?, string(args, "object")?],
        |r| r.get::<_, String>(0),
    )?;
    let mut matches = vec![];
    let mut superseded = std::collections::BTreeSet::new();
    for row in rows {
        let v: Value = serde_json::from_str(&row?)?;
        if args["current_only"] == true {
            if let Some(id) = v["data"][object["supersession_field"].as_str().unwrap()].as_str() {
                superseded.insert(id.to_owned());
            }
        }
        if args["filters"]
            .as_object()
            .into_iter()
            .flatten()
            .all(|(key, value)| v["data"][key] == *value)
            && crate::query::matches(&v["data"], &args["where"])?
            && crate::query::matches(&v, &args["metadata"])?
            && crate::query::search_matches(&v["data"], &args["search"], object)
        {
            matches.push(v);
        }
    }
    matches.retain(|r| !superseded.contains(r["id"].as_str().unwrap()));
    if args.get("order_by").is_some() {
        crate::query::sort(&mut matches, &args["order_by"])?;
    }
    let total = matches.len();
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(100) as usize;
    Ok(
        json!({"items":matches.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":total,"offset":offset,"limit":limit}),
    )
}
fn substitute(template: &Value, input: &Value) -> Result<Value> {
    if let Some(key) = template.get("$arg").and_then(Value::as_str) {
        return input
            .get(key)
            .cloned()
            .ok_or_else(|| Error::new("validation", format!("Missing action argument {key}")));
    }
    match template {
        Value::Object(map) => Ok(Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), substitute(v, input)?)))
                .collect::<Result<_>>()?,
        )),
        Value::Array(values) => Ok(Value::Array(
            values
                .iter()
                .map(|v| substitute(v, input))
                .collect::<Result<_>>()?,
        )),
        _ => Ok(template.clone()),
    }
}
fn compatible(old: &Value, new: &Value) -> Result<()> {
    ensure(
        crate::container::enabled(old) == crate::container::enabled(new),
        "migration_required",
        "Changing execution driver requires a separate installation and explicit state migration",
    )?;
    ensure(
        old["hosting"] == new["hosting"],
        "migration_required",
        "Hosting changes require a separate app installation and explicit data migration",
    )?;
    for (name, object) in old["objects"].as_object().unwrap() {
        let next = &new["objects"][name];
        ensure(
            next.is_object(),
            "migration_required",
            "Cannot remove objects",
        )?;
        for rule in [
            "immutable",
            "relationships",
            "protected_fields",
            "transitions",
        ] {
            ensure(
                object.get(rule) == next.get(rule),
                "migration_required",
                format!("Rule changes require migration: {rule}"),
            )?;
        }
        let mut old_schema = object["schema"].clone();
        let mut new_schema = next["schema"].clone();
        let old_props = old_schema
            .as_object_mut()
            .unwrap()
            .remove("properties")
            .unwrap();
        let new_props = new_schema
            .as_object_mut()
            .unwrap()
            .remove("properties")
            .unwrap();
        ensure(
            old_schema == new_schema,
            "migration_required",
            "Only optional property additions supported",
        )?;
        for (field, schema) in old_props.as_object().unwrap() {
            ensure(
                new_props.get(field) == Some(schema),
                "migration_required",
                "Existing fields cannot change",
            )?;
        }
        for (field, schema) in new_props.as_object().unwrap() {
            if old_props.get(field).is_none() {
                ensure(
                    schema.get("default").is_none(),
                    "migration_required",
                    "New defaults require a data migration",
                )?;
            }
        }
    }
    Ok(())
}
fn remote(p: &Value, operation: &str, args: &Value, actor: &str) -> Result<Value> {
    use std::io::Read;
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .no_proxy()
        .build()
        .map_err(|e| Error::new("remote", e.to_string()))?;
    let mut request = client.post(p["hosting"]["endpoint"].as_str().unwrap()).json(&json!({"protocol":"rhyven/1","operation":operation,"arguments":args,"actor":actor,"package_sha256":store::hash(p)}));
    if let Some(env) = p["hosting"]["auth_env"].as_str() {
        let token = std::env::var(env).map_err(|_| {
            Error::new("auth_required", format!("Set {env} in runtime environment"))
        })?;
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|_| {
        Error::new(
            "remote",
            "Remote request failed; inspect endpoint availability",
        )
    })?;
    ensure(
        response.status().is_success(),
        "remote",
        format!("Endpoint returned HTTP {}", response.status().as_u16()),
    )?;
    let mut bytes = Vec::new();
    response.take(1_048_577).read_to_end(&mut bytes)?;
    ensure(bytes.len() <= 1_048_576, "remote", "Response exceeds 1 MiB")?;
    let value: Value = serde_json::from_slice(&bytes)?;
    catalog::keys(&value, &["result", "error"])?;
    ensure(
        value.get("result").is_some() != value.get("error").is_some(),
        "remote",
        "Expected exactly result or error envelope",
    )?;
    if let Some(error) = value.get("error") {
        return Err(Error::new("remote_app", error.to_string()));
    }
    Ok(value["result"].clone())
}
