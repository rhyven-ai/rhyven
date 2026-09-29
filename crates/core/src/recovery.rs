//! Streaming collection archives with consistent SQLite snapshots and legacy readers.
use crate::{catalog, collections, error::ensure, store, Result, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path},
};
const LEGACY_MAX: u64 = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    size: u64,
    sha256: String,
    data: Vec<u8>,
    #[serde(default)]
    directory: bool,
    #[serde(default)]
    mode: Option<u32>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    format: u32,
    collection: Value,
    files: Vec<Entry>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn backup(runtime: &Runtime, out: &Path) -> Result<Value> {
    let _gate = crate::maintenance::lock(&runtime.root)?;
    let _services = crate::services::Suspension::new(runtime)?;
    let state = collections::state_dir(&runtime.root)?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = std::fs::canonicalize(parent)?;
    ensure(
        !parent.starts_with(std::fs::canonicalize(&state)?.join("containers")),
        "backup",
        "Backup destination cannot be inside container data",
    )?;
    let temp = tempfile::tempdir_in(&parent)?;
    let snapshot = temp.path().join("state.sqlite3");
    let db = store::open(&runtime.root)?;
    db.execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()])?;
    let copy = rusqlite::Connection::open(&snapshot)?;
    {
        let mut stmt = copy.prepare("SELECT name,package FROM apps ORDER BY name")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let raw: String = row.get(1)?;
            let package = collections::load(&runtime.root, &serde_json::from_str(&raw)?)?;
            copy.execute(
                "UPDATE apps SET package=?2 WHERE name=?1",
                rusqlite::params![name, package.to_string()],
            )?;
        }
    }
    copy.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
    drop(copy);
    let scope = collections::scope(&runtime.root)?;
    let total = crate::recovery_archive::write(&state, &snapshot, out, scope.clone())?;
    Ok(json!({"backup":out,"format":3,"bytes":total,"scope":scope}))
}

pub(crate) fn safe_path(path: &str, directory: bool) -> bool {
    if path == "state.sqlite3" && !directory {
        return true;
    }
    let parts: Vec<_> = path.split('/').collect();
    !parts.is_empty()
        && parts[0] == "containers"
        && (parts.len() == 1 || crate::container::digest(parts[1]))
        && (parts.len() <= 2 || parts[2] == "data")
        && (directory || parts.len() >= 4)
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && parts
            .iter()
            .all(|p| !p.is_empty() && *p != "." && *p != "..")
}
pub fn restore(home: &Path, name: &str, archive: &Path, accept_permissions: bool) -> Result<Value> {
    restore_inner(home, name, archive, accept_permissions, true)
}
pub(crate) fn restore_for_update(home: &Path, name: &str, archive: &Path) -> Result<Value> {
    restore_inner(home, name, archive, true, false)
}
fn restore_inner(
    home: &Path,
    name: &str,
    archive: &Path,
    accept_permissions: bool,
    invalidate: bool,
) -> Result<Value> {
    ensure(
        collections::valid_name(name),
        "collection",
        "Invalid target collection",
    )?;
    std::fs::create_dir_all(home)?;
    let home = std::fs::canonicalize(home)?;
    let gate = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("collections.lock"))?;
    fs2::FileExt::lock_exclusive(&gate)?;
    let parent = home.join("collections");
    std::fs::create_dir_all(&parent)?;
    ensure(
        !std::fs::symlink_metadata(&parent)?.file_type().is_symlink(),
        "collection",
        "Collections cannot be a symlink",
    )?;
    let target = parent.join(name);
    ensure(
        !target.try_exists()? && std::fs::symlink_metadata(&target).is_err(),
        "collection",
        "Restore requires a new collection",
    )?;
    let staging = tempfile::tempdir_in(&parent)?;
    let index = crate::recovery_archive::Index::new(&parent)?;
    let mut file = std::fs::File::open(archive)?;
    let mut magic = [0u8; 16];
    let streaming = file.read_exact(&mut magic).is_ok() && &magic == crate::recovery_archive::MAGIC;
    file.seek(SeekFrom::Start(0))?;
    if streaming {
        crate::recovery_archive::extract(file, staging.path(), &index)?;
    } else {
        extract_legacy(file, staging.path(), &index)?;
    }
    let db = rusqlite::Connection::open(staging.path().join("state.sqlite3"))?;
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    ensure(integrity == "ok", "integrity", "Invalid SQLite snapshot")?;
    let mut package_count = 0u64;
    {
        let mut stmt = db.prepare("SELECT package,digest FROM apps ORDER BY name")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let raw: String = row.get(0)?;
            let digest: String = row.get(1)?;
            let p: Value = serde_json::from_str(&raw)?;
            catalog::validate(&p)?;
            ensure(
                store::hash(&p) == digest,
                "integrity",
                "Package hash mismatch",
            )?;
            if crate::container::enabled(&p) {
                ensure(accept_permissions, "permission_review_required", "Container restore requires --accept-permissions after reviewing the backup's packages")?;
                crate::container::prepare(&p)?;
            }
            if crate::script::enabled(&p) {
                ensure(accept_permissions, "permission_review_required", "Script restore requires --accept-permissions; host.execute grants unsandboxed host access")?;
                crate::script::prepare_home(&home, &p)?;
            }
            if crate::services::enabled(&p) {
                // Never replay background work from a restored collection, even
                // when the package normally starts on its first action call.
                let services = staging.path().join("services");
                std::fs::create_dir_all(&services)?;
                store::write(
                    &services.join(format!("{}.json", store::hash(&p["name"]))),
                    &json!({"app":p["name"],"desired":"stopped","state":"stopped","explicitly_stopped":true,"suspended":false}),
                )?;
                #[cfg(unix)]
                std::fs::File::open(&services)?.sync_all()?;
            }
            // Rebind one package at a time into this home's immutable store.
            let reference = collections::save_in_home(&home, &p)?;
            db.execute(
                "UPDATE apps SET package=?1 WHERE name=?2",
                rusqlite::params![reference.to_string(), p["name"].as_str().unwrap()],
            )?;
            package_count += 1;
        }
    }
    let requests: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='market_requests')",
        [],
        |r| r.get(0),
    )?;
    if requests && invalidate {
        db.execute(
            "UPDATE market_requests SET status='invalidated' WHERE status!='done'",
            [],
        )?;
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
    drop(db);
    store::write(
        &staging.path().join("collection.json"),
        &json!({"format":1,"name":name}),
    )?;
    index.finish(staging.path())?;
    std::fs::rename(staging.path(), &target)?;
    #[cfg(unix)]
    std::fs::File::open(&parent)?.sync_all()?;
    Ok(
        json!({"restored":name,"workspace":target,"apps":package_count,"pending_approvals_invalidated":true}),
    )
}

fn extract_legacy(
    file: std::fs::File,
    staging: &Path,
    index: &crate::recovery_archive::Index,
) -> Result<()> {
    ensure(
        file.metadata()?.len() <= LEGACY_MAX * 5,
        "backup",
        "Archive too large",
    )?;
    let mut raw = vec![];
    file.take(LEGACY_MAX * 5 + 1).read_to_end(&mut raw)?;
    ensure(
        raw.len() as u64 <= LEGACY_MAX * 5,
        "backup",
        "Archive too large",
    )?;
    let mut archive: Archive = serde_json::from_slice(&raw)?;
    ensure(
        (archive.format == 1 || archive.format == 2) && archive.files.len() <= 10000,
        "backup",
        "Unsupported archive",
    )?;
    let mut paths = std::collections::BTreeSet::new();
    let mut total = 0u64;
    for entry in &archive.files {
        total = total.saturating_add(entry.size);
        ensure(
            safe_path(&entry.path, entry.directory)
                && (!entry.directory || entry.data.is_empty())
                && entry.mode.is_none_or(|mode| mode & !0o777 == 0)
                && paths.insert(&entry.path)
                && entry.size == entry.data.len() as u64
                && hash(&entry.data) == entry.sha256
                && total <= LEGACY_MAX,
            "integrity",
            "Invalid archive path, size, checksum or duplicate entry",
        )?;
    }
    ensure(
        paths.contains(&"state.sqlite3".to_owned()),
        "backup",
        "Archive has no database",
    )?;
    archive.files.sort_by_key(|entry| entry.path.len());
    for entry in &archive.files {
        index.register(&entry.path, entry.mode)?;
        let path = staging.join(&entry.path);
        if entry.directory {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&entry.data)?;
        file.sync_all()?;
    }
    Ok(())
}
