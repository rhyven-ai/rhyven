//! Named state boundaries with an immutable, user-wide package store.
use crate::{catalog, error::ensure, store, Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

pub fn create(home: &Path, name: &str) -> Result<PathBuf> {
    ensure(
        valid_name(name),
        "collection",
        "Collection must contain 1-64 lowercase letters, digits, hyphens or underscores",
    )?;
    store::private_dir(home)?;
    let home = std::fs::canonicalize(home)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("collections.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let parent = home.join("collections");
    store::private_dir(&parent)?;
    ensure(
        !std::fs::symlink_metadata(&parent)?.file_type().is_symlink(),
        "collection",
        "Collections directory cannot be a symlink",
    )?;
    let root = parent.join(name);
    store::private_dir(&root)?;
    ensure(
        !std::fs::symlink_metadata(&root)?.file_type().is_symlink(),
        "collection",
        "Collection cannot be a symlink",
    )?;
    let marker = root.join("collection.json");
    let manifest = json!({"format":1,"name":name});
    if !marker.exists() {
        ensure(
            std::fs::read_dir(&root)?.next().is_none(),
            "collection",
            "Refusing to adopt a nonempty directory without a collection manifest",
        )?;
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new_in(&root)?;
        tmp.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
        tmp.as_file().sync_all()?;
        match tmp.persist_noclobber(&marker) {
            Ok(_) => (),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(Error::new("io", e.to_string())),
        }
    }
    home_for(&root)?;
    Ok(root)
}

pub fn home_for(root: &Path) -> Result<Option<PathBuf>> {
    let path = root.join("collection.json");
    if !path.exists() {
        return Ok(None);
    }
    ensure(
        !std::fs::symlink_metadata(&path)?.file_type().is_symlink()
            && std::fs::metadata(&path)?.len() <= 4096,
        "collection",
        "Invalid collection manifest",
    )?;
    let manifest: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let name = root.file_name().and_then(|n| n.to_str()).unwrap_or("");
    ensure(
        valid_name(name) && manifest == json!({"format":1,"name":name}),
        "collection",
        "Collection manifest does not match its directory",
    )?;
    let parent = root
        .parent()
        .ok_or_else(|| Error::new("collection", "Invalid collection path"))?;
    ensure(
        parent.file_name().is_some_and(|n| n == "collections")
            && !std::fs::symlink_metadata(root)?.file_type().is_symlink()
            && !std::fs::symlink_metadata(parent)?.file_type().is_symlink(),
        "collection",
        "Invalid collection location",
    )?;
    Ok(Some(
        parent
            .parent()
            .ok_or_else(|| Error::new("collection", "Missing Rhyven home"))?
            .to_path_buf(),
    ))
}

pub fn state_dir(root: &Path) -> Result<PathBuf> {
    Ok(if home_for(root)?.is_some() {
        root.to_path_buf()
    } else {
        root.join(".rhyven")
    })
}
pub fn registry_dir(root: &Path) -> Result<PathBuf> {
    Ok(match home_for(root)? {
        Some(home) => home.join("registry-cache"),
        None => root.join(".rhyven"),
    })
}
pub fn scope(root: &Path) -> Result<Value> {
    Ok(
        json!({"collection":if home_for(root)?.is_some() {root.file_name().and_then(|v|v.to_str())} else {None},"workspace":root}),
    )
}

/// Store packages once; collection databases retain only this immutable reference.
pub fn save(root: &Path, package: &Value) -> Result<Value> {
    let Some(home) = home_for(root)? else {
        return Ok(package.clone());
    };
    save_in_home(&home, package)
}

pub(crate) fn save_in_home(home: &Path, package: &Value) -> Result<Value> {
    catalog::validate(package)?;
    let digest = store::hash(package);
    let dir = home.join("packages/sha256");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{digest}.json"));
    let reference = json!({"package_sha256":digest});
    use std::io::Write;
    if !path.exists() {
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        tmp.write_all(serde_json::to_string(package)?.as_bytes())?;
        tmp.as_file().sync_all()?;
        match tmp.persist_noclobber(&path) {
            Ok(_) => (),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(Error::new("io", e.to_string())),
        }
    }
    ensure(
        serde_json::from_slice::<Value>(&std::fs::read(&path)?)? == *package,
        "integrity",
        "Shared package content mismatch",
    )?;
    Ok(reference)
}

pub fn load(root: &Path, value: &Value) -> Result<Value> {
    let Some(digest) = value.get("package_sha256") else {
        return Ok(value.clone());
    };
    let digest = digest.as_str().unwrap_or("");
    ensure(
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "integrity",
        "Invalid package reference",
    )?;
    let home = home_for(root)?
        .ok_or_else(|| Error::new("integrity", "Package reference outside a collection"))?;
    let path = home.join("packages/sha256").join(format!("{digest}.json"));
    ensure(
        std::fs::metadata(&path)?.len() <= 1_048_576,
        "integrity",
        "Shared package too large",
    )?;
    let p: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure(
        store::hash(&p) == digest,
        "integrity",
        "Shared package checksum mismatch",
    )?;
    catalog::validate(&p)?;
    Ok(p)
}

/// A shell default; agent configurations always pin an explicit collection.
pub fn current(home: &Path) -> Result<String> {
    let path = home.join("default-collection.json");
    if !path.exists() {
        return Ok("global".into());
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let name = value["collection"].as_str().unwrap_or("");
    ensure(
        valid_name(name),
        "configuration",
        "Invalid default collection",
    )?;
    Ok(name.into())
}
pub fn select(home: &Path, name: &str) -> Result<Value> {
    create(home, name)?;
    store::write(
        &home.join("default-collection.json"),
        &json!({"collection":name}),
    )?;
    Ok(json!({"collection":name,"affects":"CLI default only; configured agents remain pinned"}))
}
pub fn list(home: &Path) -> Result<Value> {
    let parent = home.join("collections");
    let mut names = vec![];
    if parent.exists() {
        for entry in std::fs::read_dir(parent)? {
            let path = entry?.path();
            if path.join("collection.json").exists() && home_for(&path)?.is_some() {
                names.push(path.file_name().unwrap().to_string_lossy().to_string());
            }
        }
    }
    names.sort();
    Ok(json!({"default":current(home)?,"collections":names}))
}
