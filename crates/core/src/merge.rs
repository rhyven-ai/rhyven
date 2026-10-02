//! Portable, append-only record transfer. Preview binds the exact destination snapshot.
use crate::{catalog, error::ensure, runtime, schema, store, tools, Error, Result, Runtime};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const MAX_RECORDS: usize = 1000;
const MAX_BYTES: usize = 524_288;

pub fn supported(p: &Value, name: &str, object: &Value) -> bool {
    p["platform"] != true
        && p["hosting"]["mode"] == "local"
        && !crate::execution::enabled(p)
        && object["immutable"] == true
        && object["protected_fields"]
            .as_array()
            .is_none_or(|v| v.is_empty())
        && object["transitions"]
            .as_object()
            .is_none_or(|v| v.is_empty())
        && object["relationships"]
            .as_object()
            .into_iter()
            .flatten()
            .all(|(_, r)| r.get("app").is_none() && r["object"] == name)
}

pub(crate) fn tools(app: &str, name: &str, object: &Value) -> Vec<tools::Tool> {
    let fixed = json!({"app":app,"object":name});
    let mut query = crate::query::properties(object);
    query.as_object_mut().unwrap().remove("select");
    let mut filters = object["schema"].clone();
    filters["required"] = json!([]);
    for field in filters["properties"].as_object_mut().unwrap().values_mut() {
        field.as_object_mut().unwrap().remove("default");
    }
    query["filters"] = filters;
    query["limit"] = json!({"type":"integer","minimum":1,"maximum":1000});
    query["offset"] = json!({"type":"integer","minimum":0,"maximum":1000000});
    let text = json!({"type":"string"});
    let timestamp = json!({"type":"integer","minimum":0});
    let identity = json!({"type":"object","properties":{"app":text,"object":text,"id":text},"required":["app","object","id"],"additionalProperties":false});
    let provenance = json!({"type":"object","properties":{"origin":identity,"source":text,"created_at":timestamp,"updated_at":timestamp,"updated_by":text,"revision":{"type":"integer","minimum":1}},"required":["origin","created_at","updated_at","updated_by","revision"],"additionalProperties":false});
    let record = json!({"type":"object","properties":{"app":text,"object":text,"id":text,"data":object["schema"],"revision":{"type":"integer","minimum":1},"created_at":timestamp,"updated_at":timestamp,"updated_by":text,"merge_provenance":provenance},"required":["app","object","id","data","revision","created_at","updated_at","updated_by","merge_provenance"],"additionalProperties":false});
    let bundle = json!({"type":"object","properties":{"format":{"type":"string","enum":["rhyven.records/1"]},"app":text,"object":text,"contract":text,"records":{"type":"array","maxItems":1000,"items":record}},"required":["format","app","object","contract","records"],"additionalProperties":false,"description":"Unmodified export bundle; source claims are not authenticated"});
    vec![
        tools::tool(&format!("object_{name}_export"), "Export a page of immutable records and their relationship ancestors. Save the bundle or pass it to another collection's merge preview.", query, &[], "export_records", fixed.clone()),
        tools::tool(&format!("object_{name}_merge_preview"), "Validate an export bundle and preview new records, existing records and conflicting source versions without writing. Review before applying.", json!({"bundle":bundle}), &["bundle"], "merge_preview", fixed.clone()),
        tools::tool(&format!("object_{name}_merge_apply"), "Atomically apply a reviewed merge. A stale preview is rejected. Conflicts require explicit acceptance and preserve both versions; existing records are never overwritten.", json!({"bundle":bundle,"preview_token":{"type":"string"},"allow_conflicts":{"type":"boolean"}}), &["bundle","preview_token"], "merge_apply", fixed),
    ]
}

fn records(db: &Connection, app: &str, object: &str) -> Result<Vec<Value>> {
    let mut stmt =
        db.prepare("SELECT record FROM records WHERE app=?1 AND object=?2 ORDER BY id")?;
    let rows = stmt.query_map(params![app, object], |r| r.get::<_, String>(0))?;
    rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
}
fn origin(record: &Value) -> Value {
    record
        .get("merge_provenance")
        .map(|p| p["origin"].clone())
        .unwrap_or_else(|| json!({"app":record["app"],"object":record["object"],"id":record["id"]}))
}
fn key(origin: &Value) -> String {
    store::hash(origin)
}
fn content(record: &Value, by_id: &BTreeMap<String, Value>, object: &Value) -> Result<String> {
    fn visit(
        record: &Value,
        rows: &BTreeMap<String, Value>,
        object: &Value,
        path: &mut BTreeSet<String>,
        cache: &mut BTreeMap<String, String>,
    ) -> Result<String> {
        let id = runtime::string(record, "id")?.to_owned();
        if let Some(hash) = cache.get(&id) {
            return Ok(hash.clone());
        }
        ensure(
            path.len() < 256 && path.insert(id.clone()),
            "relationship",
            "Merge requires acyclic relationships of depth below 256",
        )?;
        let mut data = record["data"].clone();
        for (field, _) in object["relationships"].as_object().into_iter().flatten() {
            if let Some(id) = data[field].as_str().filter(|s| !s.is_empty()) {
                let target = rows.get(id).ok_or_else(|| {
                    Error::new("relationship", "Bundle must include all referenced records")
                })?;
                data[field] = json!({"origin":origin(target),"content":visit(target, rows, object, path, cache)?});
            }
        }
        path.remove(&id);
        let hash = store::hash(&data);
        cache.insert(id, hash.clone());
        Ok(hash)
    }
    visit(
        record,
        by_id,
        object,
        &mut BTreeSet::new(),
        &mut BTreeMap::new(),
    )
}

fn map(rows: &[Value]) -> BTreeMap<String, Value> {
    rows.iter()
        .map(|r| (r["id"].as_str().unwrap().to_owned(), r.clone()))
        .collect()
}

pub(crate) fn call(runtime: &Runtime, operation: &str, args: Value) -> Result<Value> {
    let app = runtime::string(&args, "app")?;
    let name = runtime::string(&args, "object")?;
    let mut db = store::open(&runtime.root)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let p = runtime::package(&runtime.root, &tx, app)?;
    let object = &p["objects"][name];
    ensure(object.is_object() && supported(&p, name, object), "validation", "Merge requires local declarative immutable objects with only same-object relationships and no protected fields or transitions")?;
    let permission = if operation == "merge_apply" {
        "state.write"
    } else {
        "state.read"
    };
    ensure(
        p["permissions"]
            .as_array()
            .unwrap()
            .contains(&json!(permission)),
        "permission",
        format!("App lacks {permission}"),
    )?;
    let existing = records(&tx, app, name)?;
    let existing_map = map(&existing);
    if operation == "export_records" {
        catalog::keys(
            &args,
            &[
                "app",
                "object",
                "filters",
                "where",
                "any_of",
                "order_by",
                "metadata",
                "search",
                "current_only",
                "limit",
                "offset",
            ],
        )?;
        let page = runtime::query(&tx, &args, object)?;
        let mut selected: BTreeSet<String> = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_owned())
            .collect();
        let mut pending: Vec<String> = selected.iter().cloned().collect();
        while let Some(id) = pending.pop() {
            for (field, _) in object["relationships"].as_object().into_iter().flatten() {
                if let Some(target) = existing_map[&id]["data"][field]
                    .as_str()
                    .filter(|s| !s.is_empty())
                {
                    ensure(
                        existing_map.contains_key(target),
                        "relationship",
                        "Missing source relationship",
                    )?;
                    if selected.insert(target.to_owned()) {
                        pending.push(target.to_owned());
                    }
                }
            }
            ensure(
                selected.len() <= MAX_RECORDS,
                "validation",
                "Export with ancestors exceeds 1000 records; select a smaller page",
            )?;
        }
        let source = crate::collections::scope(&runtime.root)?.to_string();
        let rows: Vec<_> = selected.iter().map(|id| {
            let mut r = existing_map[id].clone();
            if r.get("merge_provenance").is_none() {
                r["merge_provenance"] = json!({"origin":origin(&r),"source":source,"created_at":r["created_at"],"updated_at":r["updated_at"],"updated_by":r["updated_by"],"revision":r["revision"]});
            }
            r
        }).collect();
        let bundle = json!({"format":"rhyven.records/1","app":app,"object":name,"contract":store::hash(object),"records":rows});
        ensure(
            bundle.to_string().len() <= MAX_BYTES,
            "validation",
            "Export exceeds 512 KiB; select a smaller page",
        )?;
        return Ok(
            json!({"bundle":bundle,"selected":page["items"].as_array().unwrap().len(),"total":page["total"],"offset":page["offset"],"limit":page["limit"],"includes_relationship_ancestors":true}),
        );
    }
    catalog::keys(
        &args,
        if operation == "merge_apply" {
            &[
                "app",
                "object",
                "bundle",
                "preview_token",
                "allow_conflicts",
            ]
        } else {
            &["app", "object", "bundle"]
        },
    )?;
    if let Some(v) = args.get("allow_conflicts") {
        ensure(
            v.is_boolean(),
            "validation",
            "allow_conflicts must be boolean",
        )?;
    }
    let definitions = tools(app, name, object);
    let definition = &definitions[if operation == "merge_apply" { 2 } else { 1 }].definition;
    let mut input = args.clone();
    input.as_object_mut().unwrap().remove("app");
    input.as_object_mut().unwrap().remove("object");
    ensure(
        schema::validate(input.clone(), &definition["inputSchema"])? == input,
        "validation",
        "Bundle must include all defaults",
    )?;
    let bundle = &args["bundle"];
    catalog::keys(bundle, &["format", "app", "object", "contract", "records"])?;
    ensure(
        bundle["format"] == "rhyven.records/1"
            && bundle["app"] == app
            && bundle["object"] == name
            && bundle["contract"] == store::hash(object),
        "validation",
        "Bundle app, object and contract must match destination",
    )?;
    ensure(
        bundle.to_string().len() <= MAX_BYTES,
        "validation",
        "Bundle exceeds 512 KiB",
    )?;
    let incoming = bundle["records"]
        .as_array()
        .ok_or_else(|| Error::new("validation", "Bundle records must be an array"))?;
    ensure(
        incoming.len() <= MAX_RECORDS,
        "validation",
        "At most 1000 records per bundle",
    )?;
    let mut ids = BTreeSet::new();
    let mut origins = BTreeSet::new();
    for record in incoming {
        let id = runtime::string(record, "id")?;
        ensure(
            !id.is_empty() && id.len() <= 128 && ids.insert(id),
            "validation",
            "Invalid or duplicate record id",
        )?;
        ensure(
            record["app"] == app && record["object"] == name,
            "validation",
            "Invalid record scope",
        )?;
        let validated = schema::validate(record["data"].clone(), &object["schema"])?;
        ensure(
            validated == record["data"],
            "validation",
            "Export data must contain schema defaults",
        )?;
        let o = origin(record);
        catalog::keys(&o, &["app", "object", "id"])?;
        ensure(
            o["app"] == app
                && o["object"] == name
                && o["id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && id.len() <= 128),
            "validation",
            "Invalid source identity",
        )?;
        // Multiple versions of an origin are allowed; conflicting copies remain separate.
        ensure(
            origins.insert((key(&o), store::hash(&record["data"]))),
            "validation",
            "Duplicate source record in bundle",
        )?;
    }
    let incoming_map = map(incoming);
    let mut known: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for row in &existing {
        known.entry(key(&origin(row))).or_default().push((
            content(row, &existing_map, object)?,
            row["id"].as_str().unwrap().to_owned(),
        ));
    }
    let mut mapping = BTreeMap::new();
    let mut additions = BTreeSet::new();
    let mut conflicts = vec![];
    for row in incoming {
        let source_id = row["id"].as_str().unwrap();
        let o = origin(row);
        let digest = content(row, &incoming_map, object)?;
        let versions = known.entry(key(&o)).or_default();
        let id = if let Some((_, id)) = versions.iter().find(|(hash, _)| *hash == digest) {
            id.clone()
        } else {
            let id = format!("merge_{}", store::hash(&json!([o, digest])));
            ensure(
                !existing_map.contains_key(&id),
                "integrity",
                "Destination ID collision",
            )?;
            if !versions.is_empty() {
                conflicts.push(json!({"source_id":source_id,"existing_ids":versions.iter().map(|(_,id)|id).collect::<Vec<_>>(),"incoming_id":id}));
            }
            versions.push((digest, id.clone()));
            additions.insert(source_id.to_owned());
            id
        };
        mapping.insert(source_id.to_owned(), id);
    }
    if let Some(field) = object["supersession_field"].as_str() {
        let mut successors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for row in &existing {
            if let Some(parent) = row["data"][field].as_str().filter(|s| !s.is_empty()) {
                successors
                    .entry(parent.to_owned())
                    .or_default()
                    .insert(row["id"].as_str().unwrap().to_owned());
            }
        }
        for row in incoming {
            let source_id = row["id"].as_str().unwrap();
            if !additions.contains(source_id) {
                continue;
            }
            if let Some(parent) = row["data"][field].as_str().filter(|s| !s.is_empty()) {
                let target = &mapping[parent];
                let children = successors.entry(target.clone()).or_default();
                if !children.is_empty() {
                    conflicts.push(json!({"kind":"correction_branch","parent_id":target,"existing_ids":children,"incoming_id":mapping[source_id]}));
                }
                children.insert(mapping[source_id].clone());
            }
        }
    }
    let mut planned = BTreeMap::new();
    for row in incoming {
        let source_id = row["id"].as_str().unwrap();
        if !additions.contains(source_id) {
            continue;
        }
        let mut data = row["data"].clone();
        for (field, _) in object["relationships"].as_object().into_iter().flatten() {
            if let Some(id) = data[field].as_str().filter(|s| !s.is_empty()) {
                data[field] = json!(mapping[id]);
            }
        }
        schema::validate(data.clone(), &object["schema"])?;
        planned.insert(source_id.to_owned(), data);
    }
    let token = store::hash(&json!([p, existing, bundle, runtime.actor, runtime.root]));
    let result = json!({"preview_token":token,"new_records":additions.len(),"existing_records":incoming.len()-additions.len(),"conflicts":conflicts,"id_mapping":mapping,"source_claims_authenticated":false});
    if operation == "merge_preview" {
        return Ok(result);
    }
    ensure(
        args["preview_token"] == token,
        "merge_stale",
        "Preview changed or belongs to another actor; preview and review again",
    )?;
    ensure(
        conflicts.is_empty() || args["allow_conflicts"] == true,
        "merge_conflict",
        "Review conflicts and explicitly allow preserving both versions",
    )?;
    let now = runtime::now();
    for row in incoming {
        let source_id = row["id"].as_str().unwrap();
        if !additions.contains(source_id) {
            continue;
        }
        let data = &planned[source_id];
        let provenance = row.get("merge_provenance").cloned().unwrap_or_else(|| json!({"origin":origin(row),"created_at":row["created_at"],"updated_at":row["updated_at"],"updated_by":row["updated_by"],"revision":row["revision"]}));
        let id = &mapping[source_id];
        let record = json!({"id":id,"app":app,"object":name,"revision":1,"data":data,"created_at":now,"updated_at":now,"updated_by":runtime.actor,"merge_provenance":provenance});
        tx.execute(
            "INSERT INTO records(app,object,id,record) VALUES(?1,?2,?3,?4)",
            params![app, name, id, record.to_string()],
        )?;
    }
    tx.execute("INSERT INTO events(app,event) VALUES(?1,?2)", params![app,json!({"operation":"merge","object":name,"actor":runtime.actor,"time":now,"bundle_sha256":store::hash(bundle),"new_records":additions.len(),"conflicts":conflicts,"preview_token":token}).to_string()])?;
    tx.commit()?;
    Ok(json!({"applied":true,"report":result}))
}
