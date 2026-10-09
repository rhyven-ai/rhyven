//! Reserved platform provider. Marketplace packages cannot register privileged handlers.
use crate::{catalog, error::ensure, registry, schema, store, Error, Result, Runtime};
use fs2::FileExt;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::File,
    time::{SystemTime, UNIX_EPOCH},
};

pub const APP: &str = "rhyven/marketplace";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .ok_or_else(|| Error::new("validation", format!("Missing string {k}")))
}
fn input(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn describe() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let mut actions = serde_json::Map::new();
    for (name, description) in [
        ("prepare_install", "Prepare an install; no package download"),
        ("prepare_update", "Prepare an update; no package download"),
        ("prepare_remove", "Prepare removal; records are retained"),
    ] {
        actions.insert(name.into(), json!({"description":description,"input":input(json!({"app":string,"version":string}), &["app"])}));
    }
    actions.insert("apply".into(), json!({"description":"Apply a prepared request. User approval is required through MCP elicitation or the local approval command. Never self-approve.","input":input(json!({"request_id":string}), &["request_id"])}));
    actions.insert("refresh".into(), json!({"description":"Refresh configured registry manifests and stars (public registry by default); installs no apps and pulls no images","input":input(json!({}), &[])}));
    actions.insert("refresh_status".into(), json!({"description":"Read last catalog sync time and error", "input":input(json!({}), &[])}));
    actions.insert("requirements".into(), json!({"description":"Check host prerequisites before installation; no app code, dependency installation or image downloads", "input":input(json!({"app":{"type":"string"}}), &["app"])}));
    actions.insert("doctor".into(), json!({"description":"Read-only execution capability diagnosis; no downloads or app execution", "input":input(json!({}), &[])}));
    actions.insert("prepare_pallet".into(),json!({"description":"Prepare a user-approved source download into workspace or global library. Does not execute source.","input":input(json!({"selector":string,"scope":{"type":"string","enum":["workspace","global"]}}), &["selector","scope"])}));
    actions.insert("pallet_search".into(),json!({"description":"Browse marketplace source libraries, separate from complete apps. Download requires user approval; no code executes during download.","input":input(json!({"query":string}), &[])}));
    actions.insert("pallet_list".into(),json!({"description":"List saved portable source libraries. Pallets are not installed apps; this does not execute source.","input":input(json!({}), &[])}));
    actions.insert("pallet_describe".into(),json!({"description":"Read a saved library index or one typed export without loading source. Reuse contract_hash with if_hash.","input":input(json!({"selector":string,"export":string,"if_hash":string}), &["selector"])}));
    actions.insert("match_plan".into(),json!({"description":"Match a structured plan against bounded local capability metadata. No installation or execution; one explicit retry maximum.", "input":crate::discovery::input_schema()}));
    actions.insert("inspect_candidate".into(),json!({"description":"Read one shortlisted contract within the eight-inspection budget","input":input(json!({"session":string,"candidate":string}), &["session","candidate"])}));
    actions.insert("workflow_report".into(),json!({"description":"Read this actor's workflow run report","input":input(json!({"request_id":string}), &["request_id"])}));
    actions.insert("stack_report".into(),json!({"description":"Deprecated alias for workflow_report","input":input(json!({"request_id":string}), &["request_id"])}));
    let long_text = json!({"type":"string"});
    let hosting = input(
        json!({"mode":string,"endpoint":long_text,"auth_env":string,"auth":long_text,"privacy":long_text,"account":long_text,"billing":long_text,"domains":{"type":"array","items":string}}),
        &["mode"],
    );
    let mut request_fields = json!({"request_id":string,"operation":string,"app":string,"version":string,"publisher":string,"publisher_label":string,"display_name":string,"repository":string,"stars":{"type":"integer"},"stars_status":long_text,"stars_checked_at":{"type":"integer"},"permissions":{"type":"array","items":string},"hosting":hosting,"trust":string,"sha256":string,"target_workspace":long_text,"target_collection":string,"expires_at":{"type":"integer"},"data_retained":{"type":"boolean"},"approval_instructions":long_text,"status":string,"review_digest":string,"execution_warning":long_text});
    request_fields["requirements"] = input(
        json!({"status":string,"summary":long_text,"detail":long_text}),
        &["status", "summary", "detail"],
    );
    request_fields["execution"] = input(
        json!({"driver":string,"language":string,"entrypoint":string,"artifacts":input(json!({"linux-x86_64":input(json!({"sha256":string}), &["sha256"]),"linux-aarch64":input(json!({"sha256":string}), &["sha256"])}), &[]),"environment":string,"python_version":string,"node_version":string,"dependencies":input(json!({"pip":string,"npm":string}), &[]),"image":long_text,"protocol":string,"timeout_seconds":{"type":"integer"},"memory_mb":{"type":"integer"},"cpus":{"type":"integer"},"secrets":{"type":"array","items":string}}),
        &["driver"],
    );
    request_fields["execution"]["properties"].as_object_mut().unwrap().extend(json!({
        "mode":string,"start_policy":string,"startup_timeout_seconds":{"type":"integer"},"shutdown_timeout_seconds":{"type":"integer"},"restart_limit":{"type":"integer"},
        "calls":{"type":"array","items":input(json!({"category":string,"function":string,"version":string}), &["category","function","version"])}
    }).as_object().unwrap().clone());
    request_fields["dependencies"] = json!({"type":"array","items":input(json!({"alias":string,"app":string,"version":string,"sha256":string}), &["alias","app","version","sha256"])});
    let fields = json!({"name":string,"version":string,"description":long_text,"publisher":string,"publisher_label":string,"display_name":string,"repository":string,"installed_version":string,"update_available":{"type":"boolean"},"trust":string,"permissions":{"type":"array","items":string},"hosting":string,"stars":{"type":"integer","minimum":0},"stars_status":string,"stars_checked_at":{"type":"integer"},"metadata_checked_at":{"type":"integer"},"metadata_stale":{"type":"boolean"}});
    json!({"name":APP,"version":"0.1.0","publisher":"rhyven","platform":true,"description":"Discover and manage agent apps through the universal interface", "hosting":{"mode":"local"},"permissions":["marketplace.read","marketplace.manage_with_user_approval"],"trust":"Platform built-in",
        "objects":{"listing":{"immutable":true,"schema":input(fields,&["name","version","description","publisher","trust","permissions","hosting","stars_status","metadata_stale"])},"request":{"immutable":true,"schema":input(request_fields, &["request_id","operation","status","expires_at","target_workspace"]) }},
        "actions":actions,"guide":"Use query listing (filters: search/name/installed/update_available) to browse. get listing uses publisher/app or publisher/app@version as id. Stars mean GitHub repository stars, never certification; unavailable is not zero. list_apps includes installed apps and this platform capability. Prepare install/update/remove, show the review including workspace, permissions and stars, ask the user, then apply the request. The host must deliver consent; an agent cannot approve itself. create/update are forbidden. Refresh downloads catalog manifests without installing apps and defaults to the public registry. All writes occur on the serving runtime's workspace. Removal retains data."})
}
pub fn summary() -> Value {
    let p = describe();
    json!({"name":APP,"version":p["version"],"description":p["description"],"hosting":p["hosting"],"platform":true,"trust":"Platform built-in"})
}

fn sources(r: &Runtime) -> Result<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    for p in catalog::list(&r.root)? {
        let key = format!("{}@{}", text(&p, "name")?, text(&p, "version")?);
        values.insert(
            key,
            json!({"kind":"local","package":p,"sha256":store::hash(&p)}),
        );
    }
    if let Some(meta) = registry::metadata(&r.root)? {
        let index: registry::Index = serde_json::from_value(meta["index"].clone())?;
        for e in index.apps {
            values.insert(format!("{}@{}",e.name,e.version), json!({"kind":"github","entry":e,"anonymous":meta["anonymous"].as_bool().unwrap_or(false)}));
        }
    }
    Ok(values)
}
fn package_info(source: &Value) -> &Value {
    if source["kind"] == "local" {
        &source["package"]
    } else {
        &source["entry"]
    }
}
fn source(r: &Runtime, selector: &str) -> Result<Value> {
    let sources = sources(r)?;
    if selector.contains('@') {
        return sources
            .get(selector)
            .cloned()
            .ok_or_else(|| Error::new("not_found", "Listing not found"));
    }
    sources
        .values()
        .filter(|v| package_info(v)["name"] == selector)
        .max_by_key(|v| catalog::version(package_info(v)["version"].as_str().unwrap()).unwrap())
        .cloned()
        .ok_or_else(|| Error::new("not_found", "Listing not found"))
}
fn listing(_r: &Runtime, source: &Value, meta: &Option<Value>, installed: &Value) -> Value {
    let p = package_info(source);
    let repo = p["repository"].as_str();
    let stats = repo.and_then(|repo| meta.as_ref().and_then(|m| m["stars"].get(repo)));
    let count = stats.and_then(|s| s["count"].as_u64());
    let mut d = json!({"name":p["name"],"version":p["version"],"description":p["description"],"publisher":p["publisher"],"publisher_label":catalog::publisher_label(p),"display_name":catalog::display_name(p),"permissions":p["permissions"],"hosting":if source["kind"]=="local" {p["hosting"]["mode"].clone()} else {p["hosting"].clone()},"trust":"Unverified","stars_status":if count.is_some() {"GitHub repository stars (cached)"} else {"unavailable"},"metadata_stale":meta.as_ref().and_then(|m|m["checked_at"].as_u64()).is_none_or(|t|now().saturating_sub(t)>3600)});
    if let Some(repo) = repo {
        d["repository"] = json!(repo);
    }
    if let Some(count) = count {
        d["stars"] = json!(count);
    }
    if let Some(t) = stats.and_then(|s| s["checked_at"].as_u64()) {
        d["stars_checked_at"] = json!(t);
    }
    if let Some(t) = meta.as_ref().and_then(|s| s["checked_at"].as_u64()) {
        d["metadata_checked_at"] = json!(t);
    }
    if let Some(old) = installed
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == p["name"])
    {
        d["installed_version"] = old["version"].clone();
        d["update_available"] = json!(
            catalog::version(p["version"].as_str().unwrap()).unwrap()
                > catalog::version(old["version"].as_str().unwrap()).unwrap()
        );
    } else {
        d["update_available"] = json!(false);
    }
    json!({"id":format!("{}@{}",p["name"].as_str().unwrap(),p["version"].as_str().unwrap()),"app":APP,"object":"listing","revision":1,"data":d})
}
fn database(r: &Runtime) -> Result<rusqlite::Connection> {
    let db = store::open(&r.root)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS market_requests (id TEXT PRIMARY KEY, body TEXT NOT NULL, status TEXT NOT NULL, result TEXT);")?;
    Ok(db)
}
fn lock(r: &Runtime) -> Result<File> {
    std::fs::create_dir_all(crate::collections::state_dir(&r.root)?)?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(crate::collections::state_dir(&r.root)?.join("market.lock"))?;
    f.lock_exclusive()?;
    Ok(f)
}
fn stored(r: &Runtime, id: &str) -> Result<(Value, String, Option<String>)> {
    database(r)?
        .query_row(
            "SELECT body,status,result FROM market_requests WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .map(|(b, s, o)| Ok((serde_json::from_str(&b)?, s, o)))
        .unwrap_or_else(|| Err(Error::new("not_found", "Approval request not found")))
}
pub fn review(r: &Runtime, id: &str) -> Result<Value> {
    let _maintenance = crate::maintenance::lock(&r.root)?;
    let (b, status, _) = stored(r, id)?;
    let mut view = b["review"].clone();
    view["status"] = json!(
        if b["expires_at"].as_u64().unwrap() < now() && status != "done" {
            "expired"
        } else {
            &status
        }
    );
    view["review_digest"] = json!(store::hash(&b));
    Ok(view)
}
/// Called only by the trusted host consent channel; never an app action.
pub fn decide(r: &Runtime, id: &str, digest: &str, accept: bool) -> Result<Value> {
    let _maintenance = crate::maintenance::lock(&r.root)?;
    let _lock = lock(r)?;
    let (b, status, _) = stored(r, id)?;
    ensure(
        status == "pending",
        "approval",
        "Request is no longer pending",
    )?;
    ensure(
        store::hash(&b) == digest && b["expires_at"].as_u64().unwrap() >= now(),
        "approval",
        "Approval is stale or expired",
    )?;
    database(r)?.execute(
        "UPDATE market_requests SET status=?2 WHERE id=?1",
        params![id, if accept { "approved" } else { "denied" }],
    )?;
    review(r, id)
}
fn installed_digest(r: &Runtime, name: &str) -> Result<Value> {
    if r.apps()?
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == name)
    {
        Ok(json!(store::hash(&r.describe(name)?)))
    } else {
        Ok(Value::Null)
    }
}
fn prepare(r: &Runtime, operation: &str, args: &Value) -> Result<Value> {
    let _lock = lock(r)?;
    let name = text(args, "app")?;
    ensure(
        name != APP,
        "permission",
        "Platform marketplace cannot be replaced or removed",
    )?;
    let before = installed_digest(r, name)?;
    ensure(
        (operation == "install") == before.is_null(),
        "validation",
        if operation == "install" {
            "Already installed; prepare an update"
        } else {
            "App is not installed"
        },
    )?;
    let target = if operation == "remove" {
        json!({"kind":"local","package":r.describe(name)?})
    } else {
        source(
            r,
            &args["version"]
                .as_str()
                .map(|v| format!("{name}@{v}"))
                .unwrap_or_else(|| name.into()),
        )?
    };
    let p = package_info(&target);
    let hosting = if target["kind"] == "local" {
        p["hosting"].clone()
    } else if p["hosting"] == "local" {
        json!({"mode":"local"})
    } else {
        ensure(
            p["hosting_details"].is_object(),
            "disclosures_required",
            "Remote hosting details must be added to registry metadata before download approval",
        )?;
        p["hosting_details"].clone()
    };
    let meta = registry::metadata(&r.root)?;
    let listing = listing(r, &target, &meta, &r.apps()?);
    let id = uuid::Uuid::new_v4().to_string();
    let expiry = now() + 900;
    let mut view = json!({"request_id":id,"operation":operation,"app":name,"version":p["version"],"publisher":p["publisher"],"publisher_label":catalog::publisher_label(p),"display_name":catalog::display_name(p),"repository":p["repository"],"stars":listing["data"]["stars"],"stars_status":listing["data"]["stars_status"],"stars_checked_at":listing["data"]["stars_checked_at"],"permissions":p["permissions"],"hosting":hosting,"trust":"Unverified","sha256":if target["kind"]=="local" {store::hash(p)} else {text(p,"sha256")?.into()},"target_workspace":r.root,"expires_at":expiry,"data_retained":true,"approval_instructions":"Ask the user before applying. MCP hosts with form elicitation prompt automatically. Otherwise the user runs rhyven --workspace PATH approve REQUEST_ID in their terminal."});
    let requirements = crate::requirements::check(p);
    view["requirements"] = json!({"status":requirements["status"],"summary":requirements["summary"],"detail":serde_json::to_string(&requirements)?});
    view["execution"] = execution_disclosure(p);
    if let Some(deps) = p.get("dependencies") {
        view["dependencies"] = json!(deps.as_object().into_iter().flatten().map(|(alias,pin)|json!({"alias":alias,"app":pin["app"],"version":pin["version"],"sha256":pin["sha256"]})).collect::<Vec<_>>());
    }
    if crate::connector::enabled(p) {
        view["execution_warning"] = json!("Connector only: the external service must already exist. Calls may change external state or incur charges. The package hash pins the wrapper, not upstream code. No automatic retries or local backups of upstream data.");
    }
    if p["permissions"]
        .as_array()
        .is_some_and(|v| v.contains(&json!("host.execute")))
    {
        view["execution_warning"] = json!("host.execute runs unsandboxed code as your OS user, including filesystem, network and process access. Environments isolate dependencies only.");
    }
    if let Some(name) = crate::collections::scope(&r.root)?["collection"].as_str() {
        view["target_collection"] = json!(name);
    }
    view.as_object_mut()
        .unwrap()
        .retain(|_, value| !value.is_null());
    let body = json!({"target":target,"before":before,"expires_at":expiry,"review":view});
    database(r)?.execute(
        "INSERT INTO market_requests(id,body,status) VALUES(?1,?2,'pending')",
        params![id, body.to_string()],
    )?;
    review(r, &id)
}
fn apply_with(
    r: &Runtime,
    id: &str,
    download: impl FnOnce(&registry::Entry, bool) -> Result<Value>,
) -> Result<Value> {
    apply_with_downloads(r, id, download, registry::fetch_reviewed_pallet)
}
fn apply_with_downloads(
    r: &Runtime,
    id: &str,
    download: impl FnOnce(&registry::Entry, bool) -> Result<Value>,
    download_pallet: impl FnOnce(&registry::PalletEntry, bool) -> Result<Value>,
) -> Result<Value> {
    let _lock = lock(r)?;
    let (body, status, result) = stored(r, id)?;
    if status == "done" {
        return Ok(serde_json::from_str(&result.unwrap())?);
    }
    ensure(
        body["expires_at"].as_u64().unwrap() >= now(),
        "approval_expired",
        "Prepare a new approval request",
    )?;
    ensure(
        status == "approved",
        "approval_required",
        "User approval is required; use host elicitation or the local approve command",
    )?;
    if body["review"]["operation"] == "pallet_download" {
        let e: registry::PalletEntry = serde_json::from_value(body["pallet_entry"].clone())?;
        let selector = format!("{}@{}", e.name, e.version);
        ensure(
            registry::pallet_entry(&r.root, &selector)? == e,
            "approval_stale",
            "Listing changed; prepare again",
        )?;
        let p = download_pallet(&e, body["anonymous"].as_bool().unwrap_or(true))?;
        let target = Runtime::new(text(&body, "pallet_root")?, &r.actor)?;
        let mut result = crate::pallet::save_scoped(&target, &p, "collection")?;
        result["scope"] = body["pallet_scope"].clone();
        database(r)?.execute(
            "UPDATE market_requests SET status='done',result=?2 WHERE id=?1",
            params![id, result.to_string()],
        )?;
        return Ok(result);
    }
    let name = text(&body["review"], "app")?;
    ensure(
        installed_digest(r, name)? == body["before"],
        "approval_stale",
        "Installed state changed; prepare again",
    )?;
    let operation = text(&body["review"], "operation")?;
    let result = if operation == "remove" {
        r.uninstall(name)?
    } else {
        let target = &body["target"];
        let selector = format!("{name}@{}", text(&body["review"], "version")?);
        ensure(
            source(r, &selector)? == *target,
            "approval_stale",
            "Listing changed; prepare again",
        )?;
        let p = if target["kind"] == "local" {
            target["package"].clone()
        } else {
            let entry: registry::Entry = serde_json::from_value(target["entry"].clone())?;
            download(&entry, target["anonymous"].as_bool().unwrap_or(false))?
        };
        ensure(
            p["hosting"] == body["review"]["hosting"],
            "integrity",
            "Hosting differs from approved disclosures",
        )?;
        ensure(
            execution_disclosure(&p) == body["review"]["execution"],
            "integrity",
            "Execution differs from approved disclosures",
        )?;
        r.install(&p, true, operation == "update")?
    };
    database(r)?.execute(
        "UPDATE market_requests SET status='done',result=?2 WHERE id=?1",
        params![id, result.to_string()],
    )?;
    Ok(result)
}

pub fn call(r: &Runtime, operation: &str, args: Value) -> Result<Value> {
    let _maintenance = crate::maintenance::lock(&r.root)?;
    match operation {
        "query" => {
            ensure(
                args["object"] == "listing",
                "not_found",
                "Query the listing object",
            )?;
            let filters = args.get("filters").cloned().unwrap_or(json!({}));
            catalog::keys(
                &filters,
                &["search", "name", "installed", "update_available"],
            )?;
            for key in ["search", "name"] {
                if let Some(v) = filters.get(key) {
                    ensure(
                        v.is_string(),
                        "validation",
                        "Search/name filters must be strings",
                    )?;
                }
            }
            for key in ["installed", "update_available"] {
                if let Some(v) = filters.get(key) {
                    ensure(
                        v.is_boolean(),
                        "validation",
                        "Installed/update filters must be booleans",
                    )?;
                }
            }
            let limit = args
                .get("limit")
                .map(|v| {
                    v.as_u64()
                        .filter(|n| *n <= 1000)
                        .ok_or_else(|| Error::new("validation", "Invalid limit"))
                })
                .transpose()?
                .unwrap_or(100);
            let offset = args
                .get("offset")
                .map(|v| {
                    v.as_u64()
                        .filter(|n| *n <= 1000000)
                        .ok_or_else(|| Error::new("validation", "Invalid offset"))
                })
                .transpose()?
                .unwrap_or(0);
            let meta = registry::metadata(&r.root)?;
            let installed = r.apps()?;
            let mut latest: BTreeMap<String, Value> = BTreeMap::new();
            for source in sources(r)?.into_values() {
                let name = text(package_info(&source), "name")?.to_owned();
                if latest.get(&name).is_none_or(|old| {
                    catalog::version(package_info(&source)["version"].as_str().unwrap()).unwrap()
                        > catalog::version(package_info(old)["version"].as_str().unwrap()).unwrap()
                }) {
                    latest.insert(name, source);
                }
            }
            let items: Vec<_> = latest
                .values()
                .map(|s| listing(r, s, &meta, &installed))
                .filter(|v| {
                    let d = &v["data"];
                    filters["search"].as_str().is_none_or(|q| {
                        format!("{} {}", d["name"], d["description"])
                            .to_lowercase()
                            .contains(&q.to_lowercase())
                    }) && filters.get("name").is_none_or(|n| *n == d["name"])
                        && filters["installed"]
                            .as_bool()
                            .is_none_or(|b| b == d.get("installed_version").is_some())
                        && filters
                            .get("update_available")
                            .is_none_or(|b| *b == d["update_available"])
                })
                .collect();
            Ok(
                json!({"total":items.len(),"items":items.into_iter().skip(offset as usize).take(limit as usize).collect::<Vec<_>>(),"offset":offset,"limit":limit}),
            )
        }
        "get" => {
            let id = text(&args, "id")?;
            match text(&args, "object")? {
                "listing" => Ok(listing(
                    r,
                    &source(r, id)?,
                    &registry::metadata(&r.root)?,
                    &r.apps()?,
                )),
                "request" => Ok(
                    json!({"id":id,"app":APP,"object":"request","revision":1,"data":review(r,id)?}),
                ),
                _ => Err(Error::new("not_found", "Unknown marketplace object")),
            }
        }
        "execute" => {
            let contract = describe();
            let action = text(&args, "action")?;
            let definition = contract["actions"]
                .get(action)
                .ok_or_else(|| Error::new("not_found", "Unknown marketplace action"))?;
            let input = schema::validate(
                args.get("args").cloned().unwrap_or(json!({})),
                &definition["input"],
            )?;
            match action {
                "prepare_pallet" => prepare_pallet(r, &input),
                "pallet_search" => {
                    let entries = crate::registry::pallet_listings(&r.root)?;
                    let q = input["query"].as_str().unwrap_or("").to_lowercase();
                    Ok(json!(entries
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|p| format!("{} {}", p["name"], p["description"])
                            .to_lowercase()
                            .contains(&q))
                        .collect::<Vec<_>>()))
                }
                "pallet_list" => crate::pallet::list(r),
                "pallet_describe" => crate::pallet::describe_recorded(
                    r,
                    &crate::pallet::resolve(r, text(&input, "selector")?)?,
                    input["export"].as_str(),
                    input["if_hash"].as_str(),
                ),
                "match_plan" => crate::discovery::match_plan(r, input),
                "inspect_candidate" => crate::discovery::inspect(
                    r,
                    text(&input, "session")?,
                    text(&input, "candidate")?,
                ),
                "workflow_report" | "stack_report" => {
                    crate::composition::report(r, text(&input, "request_id")?)
                }
                "doctor" => Ok(crate::container::doctor()),
                "prepare_install" => prepare(r, "install", &input),
                "prepare_update" => prepare(r, "update", &input),
                "prepare_remove" => prepare(r, "remove", &input),
                "apply" => apply_with(r, text(&input, "request_id")?, |entry, anonymous| {
                    registry::download_cached(&r.root, entry, anonymous)
                }),
                "refresh" => registry::refresh_catalog(&r.root),
                "refresh_status" => registry::refresh_status(&r.root),
                "requirements" => {
                    let target = source(r, text(&input, "app")?)?;
                    Ok(crate::requirements::check(package_info(&target)))
                }
                _ => unreachable!(),
            }
        }
        _ => Err(Error::new(
            "permission",
            "Marketplace objects are read-only; use declared actions",
        )),
    }
}

fn execution_disclosure(p: &Value) -> Value {
    let mut e = p
        .get("execution")
        .cloned()
        .unwrap_or(json!({"driver":"declarative"}));
    if let Some(artifacts) = e.get_mut("artifacts").and_then(Value::as_object_mut) {
        for a in artifacts.values_mut() {
            if let Some(fields) = a.as_object_mut() {
                fields.remove("hex");
            }
        }
    }
    e
}

fn prepare_pallet(r: &Runtime, args: &Value) -> Result<Value> {
    let _lock = lock(r)?;
    let scope = text(args, "scope")?;
    let root = crate::pallet::scope_root(r, scope)?;
    let e = registry::pallet_entry(&r.root, text(args, "selector")?)?;
    let meta = registry::metadata(&r.root)?;
    let id = uuid::Uuid::new_v4().to_string();
    let expiry = now() + 900;
    let mut view = json!({"request_id":id,"operation":"pallet_download","app":e.name,"version":e.version,"repository":e.repository,"sha256":e.sha256,"permissions":[],"hosting":{"mode":"local"},"trust":"Unverified","target_workspace":root,"expires_at":expiry,"execution_warning":format!("Save source in {scope}. No code executes; later execution requires separate review."),"approval_instructions":"Ask the user, then use action_apply through MCP host elicitation or the human terminal approve command."});
    view["stars"] = meta
        .as_ref()
        .map(|m| m["stars"][&e.repository]["count"].clone())
        .unwrap_or(Value::Null);
    view["stars_status"] = json!(if view["stars"].is_number() {
        "cached"
    } else {
        "unavailable"
    });
    view["execution_warning"] = json!(format!("Save {} source under {} in {scope}. No code executes; later execution requires separate review.", e.language,e.license));
    view.as_object_mut().unwrap().retain(|_, v| !v.is_null());
    let body = json!({"review":view,"expires_at":expiry,"pallet_entry":e,"pallet_root":root,"pallet_scope":scope,"anonymous":meta.as_ref().and_then(|m|m["anonymous"].as_bool()).unwrap_or(true)});
    database(r)?.execute(
        "INSERT INTO market_requests(id,body,status) VALUES(?1,?2,'pending')",
        params![id, body.to_string()],
    )?;
    review(r, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn fixture() -> (tempfile::TempDir, Runtime, Value, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let r = Runtime::new(dir.path(), "agent").unwrap();
        let mut p = catalog::bundled().remove(0);
        p["name"] = json!("tester/dummy");
        p["publisher"] = json!("tester");
        let bytes = serde_json::to_vec(&p).unwrap();
        let entry = json!({"name":p["name"],"display_name":p["display_name"],"version":p["version"],"description":p["description"],"publisher":"tester","repository":"tester/dummy","asset_id":1,"sha256":format!("{:x}",Sha256::digest(&bytes)),"permissions":p["permissions"],"hosting":"local","trust":"Unverified"});
        std::fs::create_dir_all(r.root.join(".rhyven")).unwrap();
        store::write(&r.root.join(".rhyven/market-metadata.json"),&json!({"repository":"tester/registry","ref":"main","anonymous":true,"index":{"format":1,"publishers":{"tester":"tester"},"apps":[entry]},"stars":{"tester/dummy":{"count":42,"checked_at":now()}},"checked_at":now()})).unwrap();
        (dir, r, p, bytes)
    }
    fn exec(r: &Runtime, action: &str, args: Value) -> Result<Value> {
        r.call("execute", json!({"app":APP,"action":action,"args":args}))
    }
    fn approve(r: &Runtime, v: &Value) {
        decide(
            r,
            v["request_id"].as_str().unwrap(),
            v["review_digest"].as_str().unwrap(),
            true,
        )
        .unwrap();
    }
    #[test]
    fn approvals_are_bound_to_collection() {
        let d = tempfile::tempdir().unwrap();
        let a = Runtime::collection(d.path(), "global", "agent").unwrap();
        let b = Runtime::collection(d.path(), "project", "agent").unwrap();
        let request = exec(
            &a,
            "prepare_install",
            json!({"app":"rhyven/work-management"}),
        )
        .unwrap();
        assert_eq!(request["target_collection"], "global");
        schema::validate(request.clone(), &describe()["objects"]["request"]["schema"]).unwrap();
        let id = request["request_id"].as_str().unwrap();
        assert!(review(&b, id).is_err());
        assert!(apply_with(&a, id, |_, _| panic!("no download before consent")).is_err());
        approve(&a, &request);
        apply_with(&a, id, |_, _| panic!("bundled app")).unwrap();
        assert_eq!(a.apps().unwrap().as_array().unwrap().len(), 1);
        assert!(b.apps().unwrap().as_array().unwrap().is_empty());
    }
    #[test]
    fn schema_discovery_approval_and_no_unapproved_downloads() {
        let (_dir, r, _p, bytes) = fixture();
        let contract = r.describe(APP).unwrap();
        for object in contract["objects"].as_object().unwrap().values() {
            schema::check(&object["schema"], 0).unwrap();
        }
        for action in contract["actions"].as_object().unwrap().values() {
            schema::check(&action["input"], 0).unwrap();
        }
        let listings = r
            .call(
                "query",
                json!({"app":APP,"object":"listing","filters":{"search":"dummy"}}),
            )
            .unwrap();
        assert_eq!(listings["total"], 1);
        assert_eq!(listings["items"][0]["data"]["stars"], 42);
        schema::validate(
            listings["items"][0]["data"].clone(),
            &contract["objects"]["listing"]["schema"],
        )
        .unwrap();
        let request = exec(&r, "prepare_install", json!({"app":"tester/dummy"})).unwrap();
        schema::validate(request.clone(), &contract["objects"]["request"]["schema"]).unwrap();
        let id = request["request_id"].as_str().unwrap();
        assert_eq!(
            apply_with(&r, id, |_, _| panic!("unapproved download"))
                .unwrap_err()
                .code,
            "approval_required"
        );
        assert!(exec(&r, "apply", json!({"request_id":id,"approved":true})).is_err());
        assert!(r
            .call(
                "create",
                json!({"app":APP,"object":"request","data":{"status":"approved"}})
            )
            .is_err());
        assert!(decide(&r, id, "forged", true).is_err());
        decide(&r, id, request["review_digest"].as_str().unwrap(), false).unwrap();
        assert!(apply_with(&r, id, |_, _| panic!("denied download")).is_err());
        let request = exec(&r, "prepare_install", json!({"app":"tester/dummy"})).unwrap();
        let id = request["request_id"].as_str().unwrap();
        approve(&r, &request);
        let result =
            apply_with(&r, id, |entry, _| registry::verify_package(entry, &bytes)).unwrap();
        assert_eq!(result["name"], "tester/dummy");
        assert_eq!(
            apply_with(&r, id, |_, _| panic!("retry download")).unwrap(),
            result
        );
        assert!(r
            .call("list_apps", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "tester/dummy"));
        let remove = exec(&r, "prepare_remove", json!({"app":"tester/dummy"})).unwrap();
        approve(&r, &remove);
        let removed = exec(&r, "apply", json!({"request_id":remove["request_id"]})).unwrap();
        assert_eq!(removed["data_retained"], true);
        assert!(r.apps().unwrap().as_array().unwrap().is_empty());
        assert!(exec(&r, "prepare_remove", json!({"app":APP})).is_err());
    }
    #[test]
    fn stale_expired_and_corrupt_assets_fail_closed() {
        let (_dir, r, _p, bytes) = fixture();
        let request = exec(&r, "prepare_install", json!({"app":"tester/dummy"})).unwrap();
        let id = request["request_id"].as_str().unwrap();
        approve(&r, &request);
        assert!(apply_with(&r, id, |entry, _| registry::verify_package(
            entry,
            b"bad bytes"
        ))
        .is_err());
        assert!(r.apps().unwrap().as_array().unwrap().is_empty());
        let (mut body, _, _) = stored(&r, id).unwrap();
        body["expires_at"] = json!(0);
        database(&r)
            .unwrap()
            .execute(
                "UPDATE market_requests SET body=?2 WHERE id=?1",
                params![id, body.to_string()],
            )
            .unwrap();
        assert_eq!(
            apply_with(&r, id, |_, _| panic!("expired download"))
                .unwrap_err()
                .code,
            "approval_expired"
        );
        let request = exec(&r, "prepare_install", json!({"app":"tester/dummy"})).unwrap();
        let id = request["request_id"].as_str().unwrap();
        approve(&r, &request);
        let mut meta = registry::metadata(&r.root).unwrap().unwrap();
        meta["index"]["apps"][0]["asset_id"] = json!(2);
        store::write(&r.root.join(".rhyven/market-metadata.json"), &meta).unwrap();
        assert_eq!(
            apply_with(&r, id, |_, _| panic!("changed download"))
                .unwrap_err()
                .code,
            "approval_stale"
        );
        meta["stars"] = json!({});
        store::write(&r.root.join(".rhyven/market-metadata.json"), &meta).unwrap();
        let listing = r
            .call(
                "get",
                json!({"app":APP,"object":"listing","id":"tester/dummy"}),
            )
            .unwrap();
        assert_eq!(listing["data"]["stars_status"], "unavailable");
        assert!(listing["data"].get("stars").is_none());
        let request = exec(&r, "prepare_install", json!({"app":"tester/dummy"})).unwrap();
        approve(&r, &request);
        apply_with(&r, request["request_id"].as_str().unwrap(), |entry, _| {
            registry::verify_package(entry, &bytes)
        })
        .unwrap();
    }
}

#[cfg(test)]
mod pallet_download_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn fixture() -> (tempfile::TempDir, Runtime, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let mut r = Runtime::collection(dir.path().join("home"), "test", "agent").unwrap();
        r.pallet_workspace = Some(project);
        let p = crate::pallet::read(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/pallet-text"),
        )
        .unwrap();
        let bytes = serde_json::to_vec(&p).unwrap();
        let entry = json!({"name":p["name"],"version":p["version"],"description":p["description"],"language":p["language"],"license":p["license"],"repository":"example/text-kit","asset_id":1,"sha256":format!("{:x}",Sha256::digest(&bytes))});
        let cache = crate::collections::registry_dir(&r.root).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        store::write(&cache.join("market-metadata.json"),&json!({"repository":"example/registry","ref":"main","anonymous":true,"index":{"format":1,"publishers":{"example":"example"},"apps":[],"pallets":[entry]},"stars":{}})).unwrap();
        (dir, r, bytes)
    }

    #[test]
    fn approved_source_download_pins_destination_and_reuses_without_redownload() {
        for scope in ["workspace", "global"] {
            let (_dir, mut r, bytes) = fixture();
            let target = crate::pallet::scope_root(&r, scope).unwrap();
            let request = prepare_pallet(
                &r,
                &json!({"selector":"example/text-kit@0.1.0","scope":scope}),
            )
            .unwrap();
            schema::validate(request.clone(), &describe()["objects"]["request"]["schema"]).unwrap();
            let id = request["request_id"].as_str().unwrap();
            assert_eq!(
                apply_with_downloads(
                    &r,
                    id,
                    |_, _| panic!("app transport"),
                    |_, _| panic!("download before approval")
                )
                .unwrap_err()
                .code,
                "approval_required"
            );
            decide(&r, id, request["review_digest"].as_str().unwrap(), true).unwrap();
            // Reconnecting without a project must not redirect a reviewed download.
            r.pallet_workspace = None;
            let receipt = apply_with_downloads(
                &r,
                id,
                |_, _| panic!("app transport"),
                |e, anonymous| {
                    assert!(anonymous);
                    registry::verify_pallet(e, &bytes)
                },
            )
            .unwrap();
            assert_eq!(receipt["scope"], scope);
            assert_eq!(
                receipt,
                apply_with_downloads(&r, id, |_, _| panic!("retry"), |_, _| panic!("retry"))
                    .unwrap()
            );
            let reader = Runtime::new(target, "second-agent").unwrap();
            let p = crate::pallet::resolve(&reader, "example/text-kit@0.1.0").unwrap();
            assert_eq!(
                crate::pallet::run(
                    &p,
                    "prepare_document",
                    json!({"title":"  Customer   Release Notes!  "}),
                    true,
                    None
                )
                .unwrap(),
                json!({"title":"Customer Release Notes!","slug":"customer-release-notes"})
            );
            assert!(r.apps().unwrap().as_array().unwrap().is_empty());
        }
    }

    #[test]
    fn tampered_download_is_not_saved_and_changed_listing_requires_new_consent() {
        let (_dir, r, _bytes) = fixture();
        let request = prepare_pallet(
            &r,
            &json!({"selector":"example/text-kit@0.1.0","scope":"global"}),
        )
        .unwrap();
        let id = request["request_id"].as_str().unwrap();
        decide(&r, id, request["review_digest"].as_str().unwrap(), true).unwrap();
        assert_eq!(
            apply_with_downloads(
                &r,
                id,
                |_, _| panic!("app transport"),
                |e, _| registry::verify_pallet(e, b"{}")
            )
            .unwrap_err()
            .code,
            "integrity"
        );
        assert!(crate::pallet::list(&r)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
        let path = crate::collections::registry_dir(&r.root)
            .unwrap()
            .join("market-metadata.json");
        let mut cache: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        cache["index"]["pallets"][0]["asset_id"] = json!(2);
        store::write(&path, &cache).unwrap();
        assert_eq!(
            apply_with_downloads(
                &r,
                id,
                |_, _| panic!("app transport"),
                |_, _| panic!("changed download")
            )
            .unwrap_err()
            .code,
            "approval_stale"
        );
    }
}
