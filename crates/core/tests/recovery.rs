use agent_market_core::{catalog, collections, recovery, store, Runtime};
use serde_json::{json, Value};
fn package() -> Value {
    catalog::bundled()
        .into_iter()
        .find(|p| p["name"] == "rhyven/work-management")
        .unwrap()
}
#[test]
fn roundtrip_retains_records_receipts_and_container_files_and_rejects_corruption() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "source", "tester").unwrap();
    let p = package();
    r.install(&p, true, false).unwrap();
    let args =
        json!({"app":p["name"],"object":"task","data":{"title":"keep me"},"request_id":"once"});
    let original = r.call("create", args.clone()).unwrap();
    let data = r
        .root
        .join("containers")
        .join(store::hash(&json!("test/app")))
        .join("data/nested");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("binary"), [0, 255, 128]).unwrap();
    std::fs::create_dir(data.join("empty")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(data.join("binary"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
    }
    let db = store::open(&r.root).unwrap();
    db.execute_batch("CREATE TABLE market_requests(id TEXT PRIMARY KEY,body TEXT,status TEXT,result TEXT); INSERT INTO market_requests VALUES('pending','{}','approved',NULL);").unwrap();
    drop(db);
    let archive = home.path().join("backup.rhyven");
    recovery::backup(&r, &archive).unwrap();
    let destination = tempfile::tempdir().unwrap();
    recovery::restore(destination.path(), "restored", &archive, false).unwrap();
    let restored = Runtime::collection(destination.path(), "restored", "tester").unwrap();
    let reference: String = store::open(&restored.root)
        .unwrap()
        .query_row("SELECT package FROM apps", [], |r| r.get(0))
        .unwrap();
    assert!(reference.contains("package_sha256"));
    assert_eq!(restored.call("create", args).unwrap(), original);
    assert_eq!(r.snapshot().unwrap(), restored.snapshot().unwrap());
    assert_eq!(
        std::fs::read(
            restored
                .root
                .join(data.strip_prefix(&r.root).unwrap())
                .join("binary")
        )
        .unwrap(),
        [0, 255, 128]
    );
    let restored_data = restored.root.join(data.strip_prefix(&r.root).unwrap());
    assert!(restored_data.join("empty").is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(restored_data.join("binary"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let status: String = store::open(&restored.root)
        .unwrap()
        .query_row("SELECT status FROM market_requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "invalidated");
    assert!(recovery::restore(destination.path(), "restored", &archive, false).is_err());
    let bytes = std::fs::read(&archive).unwrap();
    let bad_path = home.path().join("bad.rhyven");
    let mut bad = bytes.clone();
    let offset = bad.windows(13).position(|v| v == b"state.sqlite3").unwrap();
    bad[offset..offset + 13].copy_from_slice(b"../escape.bin");
    std::fs::write(&bad_path, &bad).unwrap();
    assert!(recovery::restore(destination.path(), "bad", &bad_path, false).is_err());
    assert!(!destination.path().join("collections/bad").exists());
    let mut bad = bytes.clone();
    let offset = bad
        .windows(15)
        .position(|v| v == b"SQLite format 3")
        .unwrap();
    bad[offset] = 0;
    std::fs::write(&bad_path, &bad).unwrap();
    assert!(recovery::restore(destination.path(), "bad", &bad_path, false).is_err());
    for length in [bytes.len() / 2, bytes.len() - 32] {
        std::fs::write(&bad_path, &bytes[..length]).unwrap();
        assert!(recovery::restore(destination.path(), "bad", &bad_path, false).is_err());
        assert!(!destination.path().join("collections/bad").exists());
    }
    let mut bad = bytes;
    bad.push(0);
    std::fs::write(&bad_path, &bad).unwrap();
    assert!(recovery::restore(destination.path(), "bad", &bad_path, false).is_err());
}
#[test]
fn migration_success_failure_and_interrupted_switch() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "project", "tester").unwrap();
    let mut p = package();
    r.install(&p, true, false).unwrap();
    r.call(
        "create",
        json!({"app":p["name"],"object":"task","data":{"title":"original"}}),
    )
    .unwrap();
    let from = p["version"].clone();
    p["version"] = json!("90.0.0");
    p["objects"]["task"]["schema"]["properties"]["source"] = json!({"type":"string"});
    p["objects"]["task"]["schema"]["required"]
        .as_array_mut()
        .unwrap()
        .push(json!("source"));
    p["migrations"] = json!([{"from":from,"protocol":1,"steps":[{"object":"task","defaults":{"source":"migration"}}]}]);
    let result = r.install(&p, true, true).unwrap();
    let reference: String = store::open(&r.root)
        .unwrap()
        .query_row("SELECT package FROM apps", [], |r| r.get(0))
        .unwrap();
    assert!(reference.contains("package_sha256"));
    assert!(std::path::Path::new(result["recovery_backup"].as_str().unwrap()).exists());
    let record = r
        .call("query", json!({"app":p["name"],"object":"task"}))
        .unwrap();
    assert_eq!(record["items"][0]["data"]["source"], "migration");
    let before = r.snapshot().unwrap();
    p["version"] = json!("91.0.0");
    p["objects"]["task"]["schema"]["properties"]["source"] = json!({"type":"integer"});
    p["migrations"] = json!([{"from":"90.0.0","protocol":1,"steps":[]}]);
    assert!(r.install(&p, true, true).is_err());
    assert_eq!(r.snapshot().unwrap(), before);
    // Simulate process death after the old database has moved but before activation.
    let db = store::open(&r.root).unwrap();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(db);
    std::fs::create_dir(r.root.join("update-old")).unwrap();
    std::fs::rename(
        r.root.join("state.sqlite3"),
        r.root.join("update-old/state.sqlite3"),
    )
    .unwrap();
    store::write(
        &r.root.join("update-journal.json"),
        &json!({"format":1,"existing":{"state.sqlite3":true,"containers":false}}),
    )
    .unwrap();
    assert_eq!(r.snapshot().unwrap(), before);
    assert!(!r.root.join("update-journal.json").exists());
}
#[test]
fn cli_default_is_separate_from_explicit_collection() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(collections::current(home.path()).unwrap(), "global");
    collections::select(home.path(), "project").unwrap();
    assert_eq!(collections::current(home.path()).unwrap(), "project");
    let explicit = Runtime::collection(home.path(), "global", "agent").unwrap();
    assert_eq!(
        explicit.call("rhyven_categories", json!({})).unwrap()["collection"],
        "global"
    );
    assert_eq!(
        collections::list(home.path()).unwrap()["collections"],
        json!(["global", "project"])
    );
}

#[test]
fn backup_waits_for_inflight_collection_operation() {
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "project", "tester").unwrap();
    r.init().unwrap();
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let root = r.root.clone();
    let action = std::thread::spawn(move || {
        let _gate = agent_market_core::maintenance::lock(&root).unwrap();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    held_rx.recv().unwrap();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let out = home.path().join("backup.rhyven");
    let backup = std::thread::spawn(move || {
        recovery::backup(&r, &out).unwrap();
        finished_tx.send(()).unwrap();
    });
    assert!(finished_rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .is_err());
    release_tx.send(()).unwrap();
    finished_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    action.join().unwrap();
    backup.join().unwrap();
}

fn file_digest(path: &std::path::Path) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    hash.finalize().to_vec()
}
#[test]
fn streaming_backup_restore_and_update_exceed_old_size_and_entry_limits() {
    use std::io::{Seek, SeekFrom, Write};
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::collection(home.path(), "large", "tester").unwrap();
    let mut p = package();
    r.install(&p, true, false).unwrap();
    let relative = std::path::PathBuf::from("containers")
        .join(store::hash(&json!("test/data")))
        .join("data");
    let data = r.root.join(&relative);
    std::fs::create_dir_all(&data).unwrap();
    let mut file = std::fs::File::create(data.join("large.bin")).unwrap();
    file.set_len(70 * 1024 * 1024).unwrap();
    file.write_all(b"start").unwrap();
    file.seek(SeekFrom::End(-3)).unwrap();
    file.write_all(b"end").unwrap();
    drop(file);
    for n in 0..10005 {
        std::fs::create_dir(data.join(format!("empty-{n}"))).unwrap();
    }
    let digest = file_digest(&data.join("large.bin"));
    let out = home.path().join("large.rhyven");
    let backup = recovery::backup(&r, &out).unwrap();
    assert_eq!(backup["format"], 3);
    assert!(backup["bytes"].as_u64().unwrap() > 64 * 1024 * 1024);
    assert!(std::fs::metadata(&out).unwrap().len() < 74 * 1024 * 1024);
    recovery::restore(home.path(), "copy", &out, false).unwrap();
    let restored = home.path().join("collections/copy").join(&relative);
    assert_eq!(file_digest(&restored.join("large.bin")), digest);
    assert!(restored.join("empty-10004").is_dir());
    p["version"] = json!("99.0.0");
    r.install(&p, true, true).unwrap();
    assert_eq!(file_digest(&data.join("large.bin")), digest);
    assert!(data.join("empty-10004").is_dir());
    assert!(recovery::backup(&r, &data.join("recursive.rhyven")).is_err());
}

#[test]
fn legacy_json_backup_versions_remain_restorable() {
    use sha2::{Digest, Sha256};
    let home = tempfile::tempdir().unwrap();
    let r = Runtime::new(home.path().join("workspace"), "tester").unwrap();
    r.install(&package(), true, false).unwrap();
    let snapshot = home.path().join("snapshot.sqlite3");
    store::open(&r.root)
        .unwrap()
        .execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()])
        .unwrap();
    let bytes = std::fs::read(snapshot).unwrap();
    for format in [1, 2] {
        let mut entry = json!({"path":"state.sqlite3","size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"data":bytes});
        if format == 2 {
            entry["directory"] = json!(false);
            entry["mode"] = json!(0o600);
        }
        let path = home.path().join(format!("legacy-{format}.rhyven"));
        std::fs::write(
            &path,
            json!({"format":format,"collection":{},"files":[entry]}).to_string(),
        )
        .unwrap();
        recovery::restore(home.path(), &format!("legacy-{format}"), &path, false).unwrap();
        let restored =
            Runtime::collection(home.path(), &format!("legacy-{format}"), "tester").unwrap();
        assert_eq!(restored.snapshot().unwrap(), r.snapshot().unwrap());
    }
}
