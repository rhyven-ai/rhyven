//! Read existing execution evidence without invoking or retrying an action.
use crate::{catalog, collections, error::ensure, store, Error, Result, Runtime};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub fn identity(r: &Runtime) -> Result<Value> {
    Ok(json!({
        "version":env!("CARGO_PKG_VERSION"),
        "executable":std::env::current_exe()?,
        "process_id":std::process::id(),
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "actor":r.actor,"scope":collections::scope(&r.root)?
    }))
}

/// Called in the same transaction as the receipt. Encoding is explicit: app
/// results may themselves contain fields named `status`, `result` or `error`.
pub(crate) fn context(
    db: &rusqlite::Connection,
    actor: &str,
    request: &str,
    app: &str,
    operation: &str,
    encoding: &str,
) -> Result<()> {
    db.execute(
        "INSERT INTO receipt_context(actor,request,app,operation,encoding) VALUES(?1,?2,?3,?4,?5)",
        params![actor, request, app, operation, encoding],
    )?;
    Ok(())
}

pub fn request_report(r: &Runtime, args: Value) -> Result<Value> {
    catalog::keys(&args, &["request_id"])?;
    let id = crate::runtime::string(&args, "request_id")?;
    ensure(
        !id.is_empty() && id.len() <= 128,
        "validation",
        "Invalid request_id",
    )?;
    let _guard = crate::maintenance::lock(&r.root)?;
    let mut db = store::open(&r.root)?;
    let tx = db.transaction()?;
    let receipt: Option<String> = tx
        .query_row(
            "SELECT result FROM receipts WHERE actor=?1 AND request=?2",
            params![r.actor, id],
            |row| row.get(0),
        )
        .optional()?;
    let context: Option<(String, String, String)> = tx
        .query_row(
            "SELECT app,operation,encoding FROM receipt_context WHERE actor=?1 AND request=?2",
            params![r.actor, id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let has_runs: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='stack_runs')",
        [],
        |row| row.get(0),
    )?;
    let workflow: Option<String> = if has_runs {
        tx.query_row(
            "SELECT report FROM stack_runs WHERE id=?1 AND actor=?2",
            params![store::hash(&json!([r.actor, id])), r.actor],
            |row| row.get(0),
        )
        .optional()?
    } else {
        None
    };
    let mut report = json!({"request_id":id,"actor":r.actor,"scope":collections::scope(&r.root)?,
        "status":"not_recorded","guidance":"No durable evidence is not proof that an external action did not run. This operation never retries work."});
    if let Some(raw) = receipt {
        let saved: Value = serde_json::from_str(&raw)?;
        report["status"] = json!("recorded_unclassified");
        report["receipt"] = saved.clone();
        if let Some((app, operation, encoding)) = context {
            report["app"] = json!(app);
            report["operation"] = json!(operation);
            if encoding == "raw" {
                report["status"] = json!("complete");
                report["result"] = saved;
            } else if encoding == "envelope" {
                report["status"] = json!(match saved["status"].as_str() {
                    Some("complete") => "complete",
                    Some("error") => "error",
                    _ => "unknown",
                });
                if saved["status"] == "complete" {
                    report["result"] = saved["result"].clone();
                }
                if saved["status"] == "error" {
                    report["error"] = saved["error"].clone();
                }
            }
        }
    }
    if let Some(raw) = workflow {
        let run: Value = serde_json::from_str(&raw)?;
        report["status"] = json!(if run["status"] == "complete" {
            "complete"
        } else {
            "partial_or_unknown"
        });
        report["workflow"] = run;
    }
    // Keep reports within the transport budget without silently clipping evidence.
    if report.to_string().len() > 262_144 {
        report.as_object_mut().unwrap().remove("receipt");
        report.as_object_mut().unwrap().remove("result");
        report.as_object_mut().unwrap().remove("workflow");
        report.as_object_mut().unwrap().remove("error");
        report["details_omitted"] = json!("Stored evidence exceeds the 256 KiB report limit");
    }
    Ok(report)
}

/// Ordered collection audit summaries, never a claim that external app data is
/// fully observed. Cursor advancement and reads use one SQLite snapshot.
pub fn changes(r: &Runtime, args: Value) -> Result<Value> {
    catalog::keys(&args, &["after", "limit", "app"])?;
    let number = |key: &str, default: u64, max: u64| -> Result<u64> {
        args.get(key)
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n <= max)
                    .ok_or_else(|| Error::new("validation", format!("Invalid {key}")))
            })
            .unwrap_or(Ok(default))
    };
    let after = number("after", 0, i64::MAX as u64)?;
    let limit = number("limit", 50, 100)?;
    ensure(limit > 0, "validation", "limit must be 1..100")?;
    let app = args
        .get("app")
        .map(|v| {
            v.as_str()
                .filter(|s| catalog::app_name(s))
                .ok_or_else(|| Error::new("validation", "Invalid app"))
        })
        .transpose()?;
    let _guard = crate::maintenance::lock(&r.root)?;
    let mut db = store::open(&r.root)?;
    let tx = db.transaction()?;
    let high: u64 = tx.query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |row| {
        row.get(0)
    })?;
    ensure(
        after <= high,
        "validation",
        "Cursor is ahead of this collection; verify the scope",
    )?;
    let mut stmt = tx.prepare("SELECT seq,app,event FROM events WHERE seq>?1 AND seq<=?2 AND (?3 IS NULL OR app=?3) ORDER BY seq LIMIT ?4")?;
    let rows = stmt.query_map(params![after, high, app, limit + 1], |row| {
        Ok((
            row.get::<_, u64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut items = Vec::new();
    let mut next = after;
    let mut more = false;
    for row in rows {
        let (seq, app, raw) = row?;
        if items.len() == limit as usize {
            more = true;
            break;
        }
        let event: Value = serde_json::from_str(&raw)?;
        items.push(json!({"cursor":seq,"app":app,"operation":event["operation"],"action":event["action"],
            "actor":event["actor"],"request_id":event["request_id"],"record_id":event["record"]["id"],
            "object":event["record"]["object"],"revision":event["record"]["revision"],
            "file_id":event["file_id"],"time":event.get("time").unwrap_or(&event["record"]["updated_at"])}));
        next = seq;
    }
    if !more {
        next = high;
    }
    Ok(
        json!({"scope":collections::scope(&r.root)?,"app":app,"items":items,"next_cursor":next,
        "has_more":more,"through_cursor":high,"coverage":"Rhyven audit events; external changes inside apps are not automatically observed"}),
    )
}
