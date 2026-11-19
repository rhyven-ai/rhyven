//! Collection-local coordination for installed Work Management tasks.
use crate::{error::ensure, runtime, schema, store, Error, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};

pub const APP: &str = "rhyven/work-management";

fn field() -> Value {
    json!({"type":"string","minLength":1,"maxLength":256})
}
fn input(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn actions() -> &'static Value {
    static ACTIONS: std::sync::LazyLock<Value> = std::sync::LazyLock::new(build_actions);
    &ACTIONS
}
fn build_actions() -> Value {
    let id = field();
    let ttl = json!({"type":"integer","minimum":1,"maximum":86400});
    let note = json!({"type":"string","maxLength":4096});
    let refs = json!({"type":"array","maxItems":32,"items":field()});
    let checkpoint = input(
        json!({"completed":note,"references":refs,"questions":refs,"uncertain_operations":refs,"next":note}),
        &["completed", "next"],
    );
    let mut actions = json!({});
    for (name, description, properties, required) in [
        (
            "ownership",
            "List task IDs, titles, status, assigned actors and current lease holders.",
            json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100}}),
            vec![],
        ),
        (
            "handoff",
            "Read the latest task checkpoint and external operation evidence.",
            json!({"id":id}),
            vec!["id"],
        ),
        (
            "claim",
            "Acquire an unclaimed or expired task lease.",
            json!({"id":id,"ttl":ttl}),
            vec!["id"],
        ),
        (
            "renew_claim",
            "Extend a current lease.",
            json!({"id":id,"token":id,"ttl":ttl}),
            vec!["id", "token"],
        ),
        (
            "release_claim",
            "Release this attempt.",
            json!({"id":id,"token":id}),
            vec!["id", "token"],
        ),
        (
            "transfer_claim",
            "Reassign a live claim to another actor.",
            json!({"id":id,"token":id,"owner":id}),
            vec!["id", "token", "owner"],
        ),
        (
            "save_checkpoint",
            "Save successor-readable progress.",
            json!({"id":id,"token":id,"checkpoint":checkpoint}),
            vec!["id", "token", "checkpoint"],
        ),
        (
            "complete_claim",
            "Complete a task with a live claim.",
            json!({"id":id,"token":id,"result":note}),
            vec!["id", "token", "result"],
        ),
        (
            "operation_intent",
            "Reserve a logical provider operation before dispatch.",
            json!({"id":id,"token":id,"key":id,"arguments":{"type":"string","maxLength":8192}}),
            vec!["id", "token", "key", "arguments"],
        ),
        (
            "operation_resolve",
            "Record verified provider evidence.",
            json!({"id":id,"token":id,"key":id,"reference":id}),
            vec!["id", "token", "key", "reference"],
        ),
    ] {
        actions[name] = json!({"description":description,"input":input(properties,&required)});
    }
    let nullable_string = json!({"anyOf":[{"type":"string"},{"type":"null"}]});
    let owner = input(
        json!({"id":id,"task":{"type":"string"},"assigned_actor":nullable_string,"current_owner":nullable_string,"status":{"type":"string"}}),
        &["id", "task", "assigned_actor", "current_owner", "status"],
    );
    let operation = input(
        json!({"key":id,"operation_id":id,"next_action":{"type":"string","enum":["reconcile","use_recorded_result"]},"state":{"type":"string","enum":["confirmed","needs_reconciliation"]},"reference":nullable_string}),
        &["key", "operation_id", "state", "reference"],
    );
    let public_claim = input(
        json!({"owner":id,"expiry":{"type":"integer"}}),
        &["owner", "expiry"],
    );
    let handoff = input(
        json!({"contract_version":{"type":"integer"},"claim":{"anyOf":[public_claim,{"type":"null"}]},"checkpoint":{"anyOf":[checkpoint,{"type":"null"}]},"operations":{"type":"array","items":operation}}),
        &["contract_version", "claim", "checkpoint", "operations"],
    );
    actions["ownership"]["output"] = input(
        json!({"contract_version":{"type":"integer"},"items":{"type":"array","items":owner}}),
        &["contract_version", "items"],
    );
    actions["handoff"]["output"] = handoff.clone();
    actions["operation_intent"]["output"] = input(
        json!({"operation_id":id,"dispatch":{"type":"boolean"},"reference":nullable_string,"needs_reconciliation":{"type":"boolean"},"next_action":{"type":"string","enum":["dispatch","reconcile","use_recorded_result"]}}),
        &[
            "operation_id",
            "dispatch",
            "reference",
            "needs_reconciliation",
            "next_action",
        ],
    );
    actions["claim"]["output"] = input(
        json!({"task":id,"token":id,"expiry":{"type":"integer"},"handoff":handoff}),
        &["task", "token", "expiry", "handoff"],
    );
    actions
}
pub fn eligible(package: &Value) -> bool {
    package["name"] == APP
        && package["hosting"]["mode"] == "local"
        && package["objects"]["task"].is_object()
}
pub fn with_actions(package: &Value) -> std::borrow::Cow<'_, Value> {
    if !eligible(package) {
        return std::borrow::Cow::Borrowed(package);
    }
    let mut expanded = package.clone();
    for action in expanded["actions"].as_object_mut().unwrap().values_mut() {
        if action["operation"] == "update" && action["object"] == "task" {
            action["input"]["properties"]["claim_token"] = field();
        }
    }
    expanded["actions"]
        .as_object_mut()
        .unwrap()
        .extend(actions().as_object().unwrap().clone());
    std::borrow::Cow::Owned(expanded)
}
pub fn invoke(
    tx: &Transaction,
    actor: &str,
    package: &Value,
    name: &str,
    args: &Value,
) -> Result<Value> {
    let Some(request) = args.get("request_id") else {
        return dispatch(tx, actor, package, name, &args["args"]);
    };
    let id = request
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or_else(|| Error::new("validation", "Invalid request_id"))?;
    let fingerprint = store::hash(&json!(["execute", args]));
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT fingerprint,result FROM receipts WHERE actor=?1 AND request=?2",
            params![actor, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((hash, raw)) = prior {
        ensure(
            hash == fingerprint,
            "idempotency_conflict",
            "Request ID already used with different arguments",
        )?;
        let mut result: Value = serde_json::from_str(&raw)?;
        if name == "operation_intent" {
            let reference: Option<String> = tx.query_row(
                "SELECT reference FROM team_operations WHERE operation_id=?1",
                [result["operation_id"].as_str()],
                |row| row.get(0),
            )?;
            result["reference"] = json!(reference);
            result["next_action"] = json!(if reference.is_some() {
                "use_recorded_result"
            } else {
                "reconcile"
            });
            result["dispatch"] = json!(false);
            result["needs_reconciliation"] = json!(result["reference"].is_null());
        }
        return Ok(result);
    }
    let result = dispatch(tx, actor, package, name, &args["args"])?;
    tx.execute(
        "INSERT INTO receipts(actor,request,fingerprint,result) VALUES(?1,?2,?3,?4)",
        params![actor, id, fingerprint, result.to_string()],
    )?;
    crate::inspection::context(tx, actor, id, APP, name, "raw")?;
    Ok(result)
}
pub fn assert_writer(tx: &Transaction, id: &str, actor: &str, token: Option<&str>) -> Result<()> {
    let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM team_claims WHERE task=?1 AND owner=?2 AND token=?3 AND token<>'' AND expiry>?4)",params![id,actor,token,runtime::now()],|r|r.get(0))?;
    ensure(
        valid,
        "claim_required",
        "Managed task writes require a current claim token for this actor",
    )
}
pub fn managed(tx: &Transaction, id: &str) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM team_claims WHERE task=?1)",
        [id],
        |r| r.get(0),
    )?)
}
fn handoff(tx: &Transaction, id: &str) -> Result<Value> {
    let state: Option<(String, String, u64)> = tx
        .query_row(
            "SELECT checkpoint,owner,expiry FROM team_claims WHERE task=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let claim = state
        .as_ref()
        .filter(|(_, _, expiry)| *expiry > runtime::now())
        .map(|(_, owner, expiry)| json!({"owner":owner,"expiry":expiry}));
    let checkpoint = state
        .as_ref()
        .map(|(value, _, _)| value.as_str())
        .unwrap_or("null");
    let mut stmt = tx.prepare("SELECT logical_key,operation_id,reference FROM team_operations WHERE task=?1 ORDER BY logical_key")?;
    let ops = stmt.query_map([id],|r|{
        let reference: Option<String> = r.get(2)?;
        Ok(json!({"key":r.get::<_,String>(0)?,"operation_id":r.get::<_,String>(1)?,"next_action":if reference.is_some() {"use_recorded_result"} else {"reconcile"},"state":if reference.is_some() {"confirmed"} else {"needs_reconciliation"},"reference":reference}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(
        json!({"contract_version":1,"claim":claim,"checkpoint":serde_json::from_str::<Value>(checkpoint)?,"operations":ops}),
    )
}
/// Local operator override. Reuse the standard reassign transition and check the
/// observed task revision in the same transaction that invalidates the lease.
pub fn operator_reassign(
    r: &crate::Runtime,
    id: &str,
    owner: &str,
    revision: u64,
) -> Result<Value> {
    ensure(
        !owner.is_empty() && owner.len() <= 128,
        "validation",
        "Invalid actor label",
    )?;
    let _guard = crate::maintenance::lock(&r.root)?;
    let mut db = store::open(&r.root)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let package = runtime::package(&r.root, &tx, APP)?;
    ensure(
        eligible(&package),
        "validation",
        "Local Work Management task required",
    )?;
    let record = runtime::get(&tx, APP, "task", id)?;
    ensure(
        record["revision"] == revision,
        "revision_conflict",
        "Task has changed",
    )?;
    ensure(
        managed(&tx, id)?
            && record["data"]["status"] != "done"
            && record["data"]["status"] != "cancelled",
        "guard_failed",
        "An open managed task is required",
    )?;
    let result = transition(
        &tx,
        &r.actor,
        &package,
        "transfer_claim",
        &json!({"id":id,"token":"operator","owner":owner}),
        true,
    )?;
    tx.commit()?;
    Ok(result)
}
pub fn dispatch(
    tx: &Transaction,
    actor: &str,
    package: &Value,
    name: &str,
    args: &Value,
) -> Result<Value> {
    transition(tx, actor, package, name, args, false)
}
fn transition(
    tx: &Transaction,
    actor: &str,
    package: &Value,
    name: &str,
    args: &Value,
    operator: bool,
) -> Result<Value> {
    let readonly = matches!(name, "ownership" | "handoff");
    let permission = if readonly {
        "state.read"
    } else {
        "state.write"
    };
    ensure(
        package["permissions"]
            .as_array()
            .is_some_and(|p| p.contains(&json!(permission))),
        "permission",
        "App lacks required state permission",
    )?;
    let a = schema::validate(args.clone(), &actions()[name]["input"])?;
    let now = runtime::now();
    if name == "ownership" {
        let mut stmt = tx.prepare("SELECT r.record, c.owner FROM records r LEFT JOIN team_claims c ON c.task=r.id AND c.expiry>?1 WHERE r.app=?2 AND r.object='task' ORDER BY r.id LIMIT ?3 OFFSET ?4")?;
        let limit = a["limit"].as_u64().unwrap_or(100);
        let offset = a["offset"].as_u64().unwrap_or(0);
        let rows = stmt.query_map(params![now, APP, limit, offset], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (raw, owner) = row?;
            let record: Value = serde_json::from_str(&raw)?;
            items.push(json!({"id":record["id"],"task":record["data"]["title"],"assigned_actor":record["data"]["owner"],"current_owner":owner,"status":record["data"]["status"]}));
        }
        return Ok(json!({"contract_version":1,"items":items}));
    }
    let id = a["id"].as_str().unwrap();
    let mut record = runtime::get(tx, APP, "task", id)?;
    if name == "handoff" {
        return handoff(tx, id);
    }
    let state: Option<(String, String, u64)> = tx
        .query_row(
            "SELECT owner,token,expiry FROM team_claims WHERE task=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let active = state.as_ref().filter(|s| s.2 > now);
    let expiry = now + a["ttl"].as_u64().unwrap_or(300);
    let mut result = json!({"task":id});
    if name == "claim" {
        ensure(
            active.is_none_or(|s| s.0 == actor && s.1.is_empty()),
            "claim_conflict",
            "Another attempt owns this task",
        )?;
        ensure(
            record["data"]["status"] != "done" && record["data"]["status"] != "cancelled",
            "guard_failed",
            "Task is closed",
        )?;
        let token = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO team_claims(task,owner,token,expiry) VALUES(?1,?2,?3,?4) ON CONFLICT(task) DO UPDATE SET owner=excluded.owner,token=excluded.token,expiry=excluded.expiry",params![id,actor,token,expiry])?;
        result = json!({"task":id,"token":token,"expiry":expiry,"handoff":handoff(tx,id)?});
    } else {
        ensure(
            operator
                || active.is_some_and(|s| s.0 == actor && !s.1.is_empty() && a["token"] == s.1),
            "claim_conflict",
            "A live claim for this actor and attempt is required",
        )?;
        match name {
            "renew_claim" => {
                tx.execute(
                    "UPDATE team_claims SET expiry=?2 WHERE task=?1",
                    params![id, expiry],
                )?;
                result["expiry"] = json!(expiry);
            }
            "release_claim" | "complete_claim" | "transfer_claim" => {
                tx.execute(
                    "UPDATE team_claims SET expiry=0,token='',owner='' WHERE task=?1",
                    [id],
                )?;
                if name == "transfer_claim" {
                    tx.execute(
                        "UPDATE team_claims SET owner=?2,expiry=?3 WHERE task=?1",
                        params![id, a["owner"].as_str(), expiry],
                    )?;
                    record["data"]["owner"] = a["owner"].clone();
                }
                if name == "complete_claim" {
                    record["data"]["status"] = json!("done");
                    record["data"]["result"] = a["result"].clone();
                    record["data"]["closed_at"] = json!(now);
                }
            }
            "save_checkpoint" => {
                tx.execute(
                    "UPDATE team_claims SET checkpoint=?2 WHERE task=?1",
                    params![id, a["checkpoint"].to_string()],
                )?;
            }
            "operation_intent" => {
                let key = a["key"].as_str().unwrap();
                let fingerprint = store::hash(&a["arguments"]);
                let fresh = uuid::Uuid::new_v4().to_string();
                let inserted=tx.execute("INSERT OR IGNORE INTO team_operations(task,logical_key,operation_id,fingerprint) VALUES(?1,?2,?3,?4)",params![id,key,fresh,fingerprint])?;
                let (op,prior,reference):(String,String,Option<String>)=tx.query_row("SELECT operation_id,fingerprint,reference FROM team_operations WHERE task=?1 AND logical_key=?2",params![id,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                ensure(
                    prior == fingerprint,
                    "idempotency_conflict",
                    "Operation key already bound to different arguments",
                )?;
                result = json!({"operation_id":op,"next_action":if inserted==1 {"dispatch"} else if reference.is_some() {"use_recorded_result"} else {"reconcile"},"dispatch":inserted==1,"reference":reference,"needs_reconciliation":inserted==0 && reference.is_none()});
            }
            "operation_resolve" => {
                ensure(tx.execute("UPDATE team_operations SET reference=?3 WHERE task=?1 AND logical_key=?2 AND (reference IS NULL OR reference=?3)",params![id,a["key"].as_str(),a["reference"].as_str()])?==1,"guard_failed","Unknown operation or conflicting provider evidence")?;
                result["next_action"] = json!("use_recorded_result");
                result["reference"] = a["reference"].clone();
            }
            _ => return Err(Error::new("validation", "Unknown task action")),
        }
    }
    let started = name == "claim" && record["data"]["status"] == "open";
    if started {
        record["data"]["status"] = json!("in_progress");
    }
    let changed = name == "claim" || name == "complete_claim" || name == "transfer_claim";
    if changed {
        record["data"] = schema::validate(
            record["data"].clone(),
            &package["objects"]["task"]["schema"],
        )?;
        record["revision"] = json!(record["revision"].as_u64().unwrap() + 1);
        record["updated_at"] = json!(now);
        record["updated_by"] = json!(actor);
        tx.execute(
            "UPDATE records SET record=?2 WHERE id=?1",
            params![id, record.to_string()],
        )?;
    }
    let attempt = if name == "claim" {
        result["token"].as_str()
    } else {
        state.as_ref().map(|s| s.1.as_str())
    }
    .filter(|s| !s.is_empty())
    .map(|s| store::hash(&json!(s)));
    let event = json!({"attempt":attempt,"operation":"task_lifecycle","action":name,"actor":actor,"record":{"id":id,"object":"task","revision":record["revision"]},"time":now});
    tx.execute(
        "INSERT INTO events(app,event) VALUES(?1,?2)",
        params![APP, event.to_string()],
    )?;
    Ok(result)
}
