//! Staged updates and crash rollback under the collection maintenance gate.
use crate::{catalog, collections, error::ensure, schema, store, Error, Result, Runtime};
use serde_json::{json, Value};
use std::path::Path;
const PARTS: [&str; 2] = ["state.sqlite3", "containers"];
pub fn validate(p: &Value) -> Result<()> {
    if let Some(action) = p.get("health_action") {
        ensure(
            crate::container::enabled(p)
                && action
                    .as_str()
                    .is_some_and(|a| p["actions"].get(a).is_some()),
            "package",
            "health_action must name a container action",
        )?;
    }
    if let Some(migrations) = p.get("migrations") {
        let steps = migrations
            .as_array()
            .ok_or_else(|| Error::new("package", "migrations must be an array"))?;
        ensure(steps.len() <= 100, "package", "Too many migrations")?;
        let mut versions = std::collections::BTreeSet::new();
        for m in steps {
            catalog::keys(m, &["from", "protocol", "steps", "action"])?;
            let from = m["from"].as_str().unwrap_or("");
            catalog::version(from)?;
            ensure(
                m["protocol"] == 1 && versions.insert(from),
                "package",
                "Migration protocol must be 1 with unique source versions",
            )?;
            if crate::container::enabled(p) {
                ensure(
                    m.get("steps").is_none()
                        && m["action"]
                            .as_str()
                            .is_some_and(|a| p["actions"].get(a).is_some()),
                    "package",
                    "Container migration requires a declared action",
                )?;
            } else {
                ensure(
                    m.get("action").is_none(),
                    "package",
                    "Declarative migrations cannot execute code",
                )?;
                let steps = m["steps"]
                    .as_array()
                    .ok_or_else(|| Error::new("package", "Migration steps required"))?;
                for step in steps {
                    catalog::keys(step, &["object", "rename", "defaults"])?;
                    ensure(
                        step["object"]
                            .as_str()
                            .is_some_and(|o| p["objects"].get(o).is_some()),
                        "package",
                        "Unknown migration object",
                    )?;
                    for key in ["rename", "defaults"] {
                        if let Some(v) = step.get(key) {
                            ensure(v.is_object(), "package", "Migration maps required")?;
                        }
                    }
                    for (from, to) in step["rename"].as_object().into_iter().flatten() {
                        ensure(
                            schema::name(from) && to.as_str().is_some_and(schema::name),
                            "package",
                            "Invalid field rename",
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn migration<'a>(old: &Value, new: &'a Value) -> Result<Option<&'a Value>> {
    ensure(
        crate::container::enabled(old) == crate::container::enabled(new)
            && crate::services::enabled(old) == crate::services::enabled(new)
            && old["hosting"] == new["hosting"],
        "migration_required",
        "Cannot migrate execution drivers or hosting modes",
    )?;
    let m = new["migrations"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["from"] == old["version"]);
    if m.is_some() {
        for (name, object) in old["objects"].as_object().unwrap() {
            let next = &new["objects"][name];
            ensure(
                next.is_object(),
                "migration_required",
                "Object removal is unsupported",
            )?;
            for rule in ["immutable", "relationships"] {
                ensure(
                    object.get(rule) == next.get(rule),
                    "migration_required",
                    "Rule migrations are unsupported",
                )?;
            }
            // Explicit migrations may extend a lifecycle, but cannot remove old
            // edges or relax existing field protection/immutability/relationships.
            let old_protected = object["protected_fields"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let new_protected = next["protected_fields"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            ensure(
                old_protected.iter().all(|v| new_protected.contains(v))
                    && new_protected.iter().all(|v| {
                        old_protected.contains(v)
                            || object["schema"]["properties"]
                                .get(v.as_str().unwrap())
                                .is_none()
                    }),
                "migration_required",
                "Only new fields may gain protection during migration",
            )?;
            for (field, states) in object["transitions"].as_object().into_iter().flatten() {
                for (from, targets) in states.as_object().unwrap() {
                    ensure(
                        next["transitions"][field][from]
                            .as_array()
                            .is_some_and(|new_targets| {
                                targets
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .all(|t| new_targets.contains(t))
                            }),
                        "migration_required",
                        "Migration cannot remove existing transition edges",
                    )?;
                }
            }
            for field in next["transitions"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, _)| k)
            {
                ensure(
                    object["transitions"].get(field).is_some()
                        || object["schema"]["properties"].get(field).is_none(),
                    "migration_required",
                    "Only new fields may gain transitions during migration",
                )?;
            }
        }
    }
    Ok(m)
}
fn remove(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path)?,
        Ok(_) => std::fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    std::fs::File::open(path)?.sync_all()?;
    Ok(())
}
/// Any uncommitted switch is rolled back before another service operation.
pub(crate) fn recover(root: &Path) -> Result<()> {
    let state = collections::state_dir(root)?;
    // The collection lock proves no updater is still using its isolated stage.
    // SIGKILL skips ScopedSupervisor::drop, so fence/remove staged containers
    // before deleting their state (including when activation never began).
    let stage = state.join("update-stage");
    let staged_root = stage.join("collections/staged");
    if staged_root.join("collection.json").exists() {
        let staged = Runtime::new(&staged_root, "update-recovery")?;
        for service in crate::services::states(&staged_root)? {
            let app = service["app"]
                .as_str()
                .ok_or_else(|| Error::new("integrity", "Invalid staged service identity"))?;
            crate::services::disable(&staged, app)?;
        }
    }
    let journal = state.join("update-journal.json");
    if !journal.exists() {
        remove(&stage)?;
        return Ok(());
    }
    let j: Value = serde_json::from_slice(&std::fs::read(&journal)?)?;
    ensure(j["format"] == 1, "integrity", "Invalid update journal")?;
    for part in PARTS {
        let old = state.join("update-old").join(part);
        let live = state.join(part);
        if old.exists() {
            remove(&live)?;
            std::fs::rename(old, live)?;
        } else if j["existing"][part] == false {
            remove(&live)?;
        }
    }
    sync_dir(&state)?;
    std::fs::remove_file(journal)?;
    sync_dir(&state)?;
    remove(&state.join("update-old"))?;
    remove(&state.join("update-stage"))?;
    Ok(())
}
fn restricted_action(r: &Runtime, p: &Value, action: &str, args: Value) -> Result<Value> {
    let mut restricted = p.clone();
    restricted["permissions"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v != "network.connect" && v != "secrets.read" && v != "app.call");
    restricted["execution"]["secrets"] = json!([]);
    if crate::services::enabled(p) {
        restricted["execution"]["calls"] = json!([]);
    }
    let db = store::open(&r.root)?;
    db.execute(
        "UPDATE apps SET package=?1,digest=?2 WHERE name=?3",
        rusqlite::params![
            restricted.to_string(),
            store::hash(&restricted),
            p["name"].as_str().unwrap()
        ],
    )?;
    drop(db);
    let supervisor = if crate::services::enabled(p) {
        Some(crate::services::ScopedSupervisor::start(
            r,
            p["name"].as_str().unwrap(),
        )?)
    } else {
        None
    };
    let result = r.call(
        "execute",
        json!({"app":p["name"],"action":action,"args":args}),
    )?;
    drop(supervisor);
    store::open(&r.root)?.execute(
        "UPDATE apps SET package=?1,digest=?2 WHERE name=?3",
        rusqlite::params![p.to_string(), store::hash(p), p["name"].as_str().unwrap()],
    )?;
    Ok(result)
}
pub fn install(r: &Runtime, p: &Value, accepted: bool) -> Result<Value> {
    catalog::validate(p)?;
    ensure(
        accepted,
        "permission_review_required",
        "Review permissions before updating",
    )?;
    let name = p["name"].as_str().unwrap();
    let old_raw: String = store::open(&r.root)?.query_row(
        "SELECT package FROM apps WHERE name=?1",
        [name],
        |row| row.get(0),
    )?;
    let old = collections::load(&r.root, &serde_json::from_str(&old_raw)?)?;
    let migration = migration(&old, p)?;
    let state = collections::state_dir(&r.root)?;
    let backups = state.join("recovery");
    std::fs::create_dir_all(&backups)?;
    let backup = backups.join(format!("{}.rhyven", uuid::Uuid::new_v4().simple()));
    crate::recovery::backup(r, &backup)?;
    let stage = state.join("update-stage");
    remove(&stage)?;
    std::fs::create_dir_all(&stage)?;
    // Restore to a private staging home so validation and migrations cannot touch live state.
    crate::recovery::restore_for_update(&stage, "staged", &backup)?;
    let staged_root = stage.join("collections/staged");
    let staged = Runtime::new(&staged_root, &r.actor)?;
    let result = staged.install_direct(p, true, true)?;
    if let Some(m) = migration {
        if crate::container::enabled(p) {
            restricted_action(
                &staged,
                p,
                m["action"].as_str().unwrap(),
                json!({"from_version":old["version"]}),
            )?;
        } else {
            let mut db = store::open(&staged.root)?;
            let tx = db.transaction()?;
            let records = tx
                .prepare("SELECT id,object,record FROM records WHERE app=?1")?
                .query_map([name], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (id, object, raw) in records {
                let mut record: Value = serde_json::from_str(&raw)?;
                for step in m["steps"].as_array().unwrap() {
                    if step["object"] != object {
                        continue;
                    }
                    for (from, to) in step["rename"].as_object().into_iter().flatten() {
                        let to = to.as_str().unwrap();
                        ensure(
                            record["data"].get(to).is_none(),
                            "migration",
                            "Rename target already exists",
                        )?;
                        if let Some(value) = record["data"].as_object_mut().unwrap().remove(from) {
                            record["data"][to] = value;
                        }
                    }
                    for (field, value) in step["defaults"].as_object().into_iter().flatten() {
                        if record["data"].get(field).is_none() {
                            record["data"][field] = value.clone();
                        }
                    }
                }
                record["data"] =
                    schema::validate(record["data"].clone(), &p["objects"][&object]["schema"])?;
                record["revision"] = json!(record["revision"].as_u64().unwrap() + 1);
                tx.execute(
                    "UPDATE records SET record=?1 WHERE id=?2",
                    rusqlite::params![record.to_string(), id],
                )?;
            }
            tx.commit()?;
        }
    }
    if let Some(action) = p["health_action"].as_str() {
        let health = restricted_action(&staged, p, action, json!({}))?;
        ensure(
            health["ok"] == true,
            "app_error",
            "Health action must return ok: true",
        )?;
    }
    // Every retained record must satisfy the new schema, even on additive upgrades.
    let db = store::open(&staged.root)?;
    let records = db
        .prepare("SELECT object,record FROM records WHERE app=?1")?
        .query_map([name], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (object, raw) in records {
        let record: Value = serde_json::from_str(&raw)?;
        schema::validate(record["data"].clone(), &p["objects"][object]["schema"])?;
    }
    // Rebind immutable references to the real home before activating staged state.
    let packages = db
        .prepare("SELECT name,package FROM apps")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (name, raw) in packages {
        let p = collections::load(&staged.root, &serde_json::from_str(&raw)?)?;
        db.execute(
            "UPDATE apps SET package=?1 WHERE name=?2",
            rusqlite::params![collections::save(&r.root, &p)?.to_string(), name],
        )?;
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
    drop(db);
    let live_db = store::open(&r.root)?;
    live_db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(live_db);
    for sidecar in ["state.sqlite3-wal", "state.sqlite3-shm"] {
        remove(&state.join(sidecar))?;
    }
    let old_dir = state.join("update-old");
    remove(&old_dir)?;
    std::fs::create_dir(&old_dir)?;
    let mut existing = json!({});
    for part in PARTS {
        existing[part] = json!(state.join(part).exists());
    }
    store::write(
        &state.join("update-journal.json"),
        &json!({"format":1,"existing":existing,"backup":backup}),
    )?;
    sync_dir(&state)?;
    let activate = (|| -> Result<()> {
        for part in PARTS {
            if state.join(part).exists() {
                std::fs::rename(state.join(part), old_dir.join(part))?;
                sync_dir(&old_dir)?;
                sync_dir(&state)?;
            }
            if staged_root.join(part).exists() {
                std::fs::rename(staged_root.join(part), state.join(part))?;
                sync_dir(&state)?;
            }
        }
        std::fs::remove_file(state.join("update-journal.json"))?;
        sync_dir(&state)?;
        Ok(())
    })();
    if let Err(error) = activate {
        recover(&r.root)?;
        return Err(error);
    }
    remove(&old_dir)?;
    remove(&stage)?;
    let mut result = result;
    result["recovery_backup"] = json!(backup);
    Ok(result)
}
