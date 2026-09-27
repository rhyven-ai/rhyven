//! Atomic state, audit and retry receipts live in the same SQLite transaction.
use crate::Result;
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};

pub fn open(root: &Path) -> Result<Connection> {
    let state = crate::collections::state_dir(root)?;
    private_dir(&state)?;
    let path = state.join("state.sqlite3");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        crate::error::ensure(
            !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "io",
            "State database cannot be a symlink",
        )?;
    }
    let db = Connection::open(state.join("state.sqlite3"))?;
    db.busy_timeout(Duration::from_secs(10))?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
      CREATE TABLE IF NOT EXISTS apps (name TEXT PRIMARY KEY, package TEXT NOT NULL, digest TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 1);
      CREATE TABLE IF NOT EXISTS records (app TEXT NOT NULL, object TEXT NOT NULL, id TEXT PRIMARY KEY, record TEXT NOT NULL);
      CREATE INDEX IF NOT EXISTS records_scope ON records(app,object);
      CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY AUTOINCREMENT, app TEXT NOT NULL, event TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS receipts (actor TEXT NOT NULL, request TEXT NOT NULL, fingerprint TEXT NOT NULL, result TEXT NOT NULL, PRIMARY KEY(actor,request));")?;
    Ok(db)
}
pub fn hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
pub fn write(path: &Path, value: &Value) -> Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| crate::Error::new("io", e.to_string()))?;
    Ok(())
}

/// New state is private to its OS owner; existing directory permissions are preserved.
pub(crate) fn private_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

#[cfg(all(test, unix))]
mod security_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn new_state_is_private_and_database_symlinks_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let db = open(&root).unwrap();
        assert_eq!(
            std::fs::metadata(root.join(".rhyven"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        assert_eq!(
            std::fs::metadata(root.join(".rhyven/state.sqlite3"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        drop(db);
        let second = temp.path().join("second");
        private_dir(&second.join(".rhyven")).unwrap();
        let victim = temp.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        symlink(&victim, second.join(".rhyven/state.sqlite3")).unwrap();
        assert!(open(&second).is_err());
        assert_eq!(std::fs::read(victim).unwrap(), b"keep");
    }
}
