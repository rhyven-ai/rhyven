//! Bounded plan matching over local metadata. Never install or execute candidates.
use crate::{error::ensure, schema, store, Error, Result, Runtime};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Instant};

pub fn input_schema() -> Value {
    let text = json!({"type":"string","minLength":1,"maxLength":1000});
    let shape = json!({"type":"array","maxItems":32,"items":{"type":"object","properties":{"name":text,"type":{"type":"string","enum":["object","array","string","number","integer","boolean"]}},"required":["name","type"],"additionalProperties":false}});
    json!({"type":"object","properties":{
        "task":text,"revision":{"type":"integer","minimum":1},"retry":{"type":"boolean","default":false},
        "steps":{"type":"array","maxItems":12,"items":{"type":"object","properties":{"id":text,"need":text,"input_fields":shape,"output_fields":shape,"output_type":{"type":"string","enum":["object","array","string","number","integer","boolean"]}},"required":["id","need"],"additionalProperties":false}},
        "permissions":{"type":"array","maxItems":16,"items":{"type":"string"}},
        "backends":{"type":"array","maxItems":5,"items":{"type":"string","enum":["declarative","script","container","native"]}}
    },"required":["task","revision","steps","permissions","backends"],"additionalProperties":false})
}
fn db(r: &Runtime) -> Result<rusqlite::Connection> {
    let db = store::open(&r.root)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS discovery_sessions(key TEXT PRIMARY KEY, actor TEXT NOT NULL, fingerprint TEXT NOT NULL, result TEXT NOT NULL, rounds INTEGER NOT NULL, inspections INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS discovery_contracts(session TEXT, candidate TEXT, result TEXT NOT NULL, PRIMARY KEY(session,candidate));
        CREATE VIRTUAL TABLE IF NOT EXISTS capability_index USING fts5(id UNINDEXED, document, tokenize='unicode61');")?;
    Ok(db)
}
fn candidates(r: &Runtime) -> Result<Vec<Value>> {
    let installed = r.apps()?;
    let installed_versions: std::collections::BTreeMap<String, String> = installed
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["name"].as_str().unwrap().to_owned(),
                p["version"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let mut packages = vec![];
    let mut seen = BTreeSet::new();
    for s in installed.as_array().unwrap() {
        packages.push((
            true,
            crate::composition::installed(r, s["name"].as_str().unwrap())?,
        ));
    }
    for p in crate::catalog::list(&r.root)?.into_iter().take(1000) {
        if installed_versions
            .get(p["name"].as_str().unwrap())
            .is_some_and(|v| v != p["version"].as_str().unwrap())
        {
            continue;
        }
        packages.push((false, p));
    }
    let mut entries = vec![];
    for (installed, p) in packages {
        let name = p["name"].as_str().unwrap();
        let version = p["version"].as_str().unwrap();
        if !seen.insert(format!("{name}@{version}")) {
            continue;
        }
        let package_hash = store::hash(&p);
        for (action, a) in p["actions"].as_object().unwrap() {
            if entries.len() >= 5000 {
                break;
            }
            let output = a
                .get("output")
                .cloned()
                .unwrap_or_else(|| json!({"type":"object"}));
            entries.push(json!({"id":format!("{name}@{version}#action_{action}"),"category":name,"function":format!("action_{action}"),"version":version,"package_hash":package_hash,"contract_hash":store::hash(a),"description":a["description"].as_str().unwrap_or("").chars().take(240).collect::<String>(),"permissions":p["permissions"],"backend":crate::execution::driver(&p),"installed":installed,"output_type":output["type"],"kind":"app_action","source_kind":"app","_input":a["input"],"_output":output,"requirements_status":"not_checked","keywords":a.get("keywords").cloned().unwrap_or(json!([]))}));
        }
    }
    Ok(entries)
}
pub fn match_plan(r: &Runtime, args: Value) -> Result<Value> {
    let _guard = crate::maintenance::lock(&r.root)?;
    let args = schema::validate(args, &input_schema())?;
    let steps = args["steps"].as_array().unwrap();
    ensure(!steps.is_empty(), "validation", "Plan needs 1..12 steps")?;
    let mut ids = BTreeSet::new();
    for step in steps {
        ensure(
            ids.insert(step["id"].as_str().unwrap()),
            "validation",
            "Duplicate plan step ID",
        )?;
    }
    let started = Instant::now();
    let entries = candidates(r)?;
    let ranker = crate::ranker::config(r).unwrap_or(json!({"configuration_error":true}));
    let revision = store::hash(&json!([
        entries,
        std::env::consts::OS,
        std::env::consts::ARCH,
        ranker
    ]));
    let key = store::hash(&json!([r.actor, args["task"], args["revision"]]));
    let fingerprint = store::hash(&json!([
        args["steps"],
        args["permissions"],
        args["backends"],
        revision
    ]));
    let db = db(r)?;
    let old = db
        .query_row(
            "SELECT fingerprint,result,rounds,inspections FROM discovery_sessions WHERE key=?1",
            [&key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            },
        )
        .optional()?;
    let mut rounds = 1;
    let mut inspections = 0;
    if let Some((old, raw, n, reads)) = old {
        if old == fingerprint {
            inspections = reads;
            let mut value: Value = serde_json::from_str(&raw)?;
            value["inspections_remaining"] = json!(8u64.saturating_sub(reads));
            if args["retry"] != true || n >= 2 {
                value["cached"] = json!(true);
                if n >= 2 {
                    value["stop_reason"] = json!("round_budget_exhausted");
                }
                return Ok(value);
            }
            rounds = n + 1;
        } else {
            return Err(Error::new(
                "validation",
                "Plan/catalog changed; increment the explicit plan revision",
            ));
        }
    }
    db.execute("DELETE FROM capability_index", [])?;
    for entry in &entries {
        db.execute(
            "INSERT INTO capability_index(id,document) VALUES(?1,?2)",
            params![
                entry["id"].as_str().unwrap(),
                format!(
                    "{} {} {} {}",
                    &entry["category"], &entry["function"], entry["description"], entry["keywords"]
                )
            ],
        )?;
    }
    let mut selected: Vec<Value> = vec![];
    let mut gaps = vec![];
    let mut known = BTreeSet::new();
    for step in steps {
        if started.elapsed().as_secs() >= 15 {
            gaps.push(json!({"step":step["id"],"next_action":"build","reason":"deadline"}));
            continue;
        }
        let terms: Vec<String> = step["need"]
            .as_str()
            .unwrap()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 1)
            .take(24)
            .map(|s| format!("\"{s}\""))
            .collect();
        let query = terms.join(" OR ");
        let mut matches = vec![];
        if !query.is_empty() {
            let mut statement=db.prepare("SELECT id,bm25(capability_index) FROM capability_index WHERE capability_index MATCH ?1 ORDER BY bm25(capability_index),id LIMIT 20")?;
            let rows = statement.query_map([query], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            for row in rows {
                let (id, score) = row?;
                let entry = entries.iter().find(|e| e["id"] == id).unwrap();
                if !entry["permissions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|p| args["permissions"].as_array().unwrap().contains(p))
                {
                    continue;
                }
                if !args["backends"]
                    .as_array()
                    .unwrap()
                    .contains(&entry["backend"])
                {
                    continue;
                }
                if step.get("output_type").is_some() && step["output_type"] != entry["output_type"]
                {
                    continue;
                }
                if let Some(fields) = step["input_fields"].as_array() {
                    let schema = &entry["_input"];
                    let required = schema["required"].as_array();
                    if required
                        .into_iter()
                        .flatten()
                        .any(|name| !fields.iter().any(|f| f["name"] == *name))
                    {
                        continue;
                    }
                    if fields.iter().any(|f| {
                        !fits_type(
                            &f["type"],
                            &schema["properties"][f["name"].as_str().unwrap()]["type"],
                        )
                    }) {
                        continue;
                    }
                }
                if let Some(fields) = step["output_fields"].as_array() {
                    let schema = &entry["_output"];
                    if fields.iter().any(|f| {
                        !fits_type(
                            &schema["properties"][f["name"].as_str().unwrap()]["type"],
                            &f["type"],
                        )
                    }) {
                        continue;
                    }
                }
                let mut candidate = entry.clone();
                candidate["_score"] = json!(score);
                matches.push(candidate);
            }
        }

        matches.sort_by(|a, b| {
            a["_score"]
                .as_f64()
                .unwrap()
                .total_cmp(&b["_score"].as_f64().unwrap())
        });
        for mut entry in matches.into_iter().take(3) {
            entry.as_object_mut().unwrap().remove("_score");
            entry.as_object_mut().unwrap().remove("_input");
            entry.as_object_mut().unwrap().remove("_output");
            let id = entry["id"].as_str().unwrap().to_owned();
            if known.contains(&id) {
                if let Some(e) = selected.iter_mut().find(|e| e["id"] == id) {
                    e["covers"].as_array_mut().unwrap().push(step["id"].clone());
                }
                continue;
            }
            if selected.len() >= 12 {
                break;
            }
            known.insert(id);
            entry["covers"] = json!([step["id"]]);
            entry["compatibility"] = json!("requires_contract_validation");
            selected.push(entry);
        }
        if !selected
            .iter()
            .any(|e| e["covers"].as_array().unwrap().contains(&step["id"]))
        {
            gaps.push(json!({"step":step["id"],"next_action":"build","reason":"No compatible candidate within search budget"}));
        }
    }
    let remaining = std::time::Duration::from_secs(15).saturating_sub(started.elapsed());
    let ranking = if ranker["configuration_error"] == true {
        json!({"status":"fallback","reason":"invalid classifier configuration"})
    } else if rounds == 1 && !remaining.is_zero() {
        crate::ranker::rerank(&ranker, &args, &mut selected, remaining).unwrap_or(
            json!({"status":"fallback","reason":"classifier unavailable, invalid, or denied"}),
        )
    } else {
        json!({"status":"skipped","reason":"no additional classifier call on retry"})
    };
    let mut response = json!({"session":key,"catalog_revision":revision,"candidates":selected,"gaps":gaps,"rounds":rounds,"cached":false,"stop_reason":if rounds==2{"round_budget_exhausted"}else{"shortlist_ready"},"next_action":if selected.is_empty(){"build"}else{"inspect"},"inspections_remaining":8u64.saturating_sub(inspections),"ranker":"lexical","notice":"Suggestions require contract validation and installation consent."});
    response["ranker"] = ranking;
    ensure(
        response.to_string().len() <= 16384,
        "validation",
        "Compact discovery response exceeds 16 KiB; narrow the plan",
    )?;
    response["elapsed_ms"] = json!(started.elapsed().as_millis());
    db.execute("INSERT INTO discovery_sessions(key,actor,fingerprint,result,rounds) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(key) DO UPDATE SET result=excluded.result,rounds=excluded.rounds",params![key,r.actor,fingerprint,response.to_string(),rounds])?;
    Ok(response)
}
pub fn inspect(r: &Runtime, session: &str, id: &str) -> Result<Value> {
    let _guard = crate::maintenance::lock(&r.root)?;
    let db = db(r)?;
    let raw: String = db
        .query_row(
            "SELECT result FROM discovery_sessions WHERE key=?1 AND actor=?2",
            params![session, r.actor],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::new("not_found", "Unknown discovery session"))?;
    let result: Value = serde_json::from_str(&raw)?;
    let entry = result["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id)
        .ok_or_else(|| Error::new("validation", "Candidate is not in this shortlist"))?;

    ensure(
        entry["source_kind"] != "pallet",
        "unsupported_operation",
        "Pallet discovery was retired; start a new app search",
    )?;
    let p = if entry["installed"] == true {
        crate::composition::installed(r, entry["category"].as_str().unwrap())?
    } else {
        crate::catalog::resolve(
            &r.root,
            &format!(
                "{}@{}",
                entry["category"].as_str().unwrap(),
                entry["version"].as_str().unwrap()
            ),
        )?
    };
    ensure(
        store::hash(&p) == entry["package_hash"],
        "version_conflict",
        "Candidate changed; revise the plan",
    )?;
    if let Some(raw) = db
        .query_row(
            "SELECT result FROM discovery_contracts WHERE session=?1 AND candidate=?2",
            params![session, id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        let description: Value = serde_json::from_str(&raw)?;
        return Ok(description);
    }
    ensure(db.execute("UPDATE discovery_sessions SET inspections=inspections+1 WHERE key=?1 AND inspections<8",[session])?==1,"validation","Contract inspection budget exhausted; reuse saved descriptions or build the missing capability")?;
    let description = crate::tools::describe(&p, &json!({"function":entry["function"]}))?;
    db.execute(
        "INSERT INTO discovery_contracts VALUES(?1,?2,?3)",
        params![session, id, description.to_string()],
    )?;
    Ok(description)
}

fn fits_type(actual: &Value, expected: &Value) -> bool {
    actual.is_string() && (actual == expected || (actual == "integer" && expected == "number"))
}
