//! Local drafts and templates. These helpers never activate or publish packages.
use crate::{catalog, composition, error::ensure, store, Error, Result, Runtime};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io::Write, path::Path};
pub fn compose(r: &Runtime, definition: &Path, out: &Path) -> Result<Value> {
    ensure(
        std::fs::metadata(definition)?.len() <= 1_048_576,
        "package",
        "Definition exceeds 1 MiB",
    )?;
    let mut p: Value = serde_json::from_slice(&std::fs::read(definition)?)?;
    let mut permissions: BTreeSet<String> = ["state.read", "app.call"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    for permission in p["permissions"].as_array().into_iter().flatten() {
        permissions.insert(
            permission
                .as_str()
                .ok_or_else(|| Error::new("package", "Permissions must be strings"))?
                .to_owned(),
        );
    }
    let deps = p["dependencies"].as_object_mut().ok_or_else(|| {
        Error::new(
            "package",
            "Define dependency aliases with installed app IDs",
        )
    })?;
    for d in deps.values_mut() {
        let name = d
            .as_str()
            .or_else(|| d["app"].as_str())
            .ok_or_else(|| Error::new("package", "Dependency requires app ID"))?;
        let child = composition::installed(r, name)?;
        for perm in child["permissions"].as_array().unwrap() {
            permissions.insert(perm.as_str().unwrap().to_owned());
        }
        *d = json!({"app":child["name"],"version":child["version"],"sha256":store::hash(&child)});
    }
    p["permissions"] = json!(permissions);
    catalog::validate(&p)?;
    composition::check_dependencies(r, &p)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    file.write_all(serde_json::to_string_pretty(&p)?.as_bytes())?;
    Ok(
        json!({"draft":out,"sha256":store::hash(&p),"permissions":p["permissions"],"activated":false,"published":false}),
    )
}
fn path_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.starts_with('.'))
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/_-.".contains(&c))
}
pub fn frame(path: &Path, out: &Path, apply: bool) -> Result<Value> {
    ensure(
        std::fs::metadata(path)?.len() <= 1_048_576,
        "package",
        "Frame exceeds 1 MiB",
    )?;
    let frame: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    catalog::keys(
        &frame,
        &["format", "name", "version", "files", "pallets", "apps"],
    )?;
    ensure(
        frame["format"] == "rhyven.frame/1"
            && catalog::app_name(frame["name"].as_str().unwrap_or("")),
        "package",
        "Invalid frame identity",
    )?;
    catalog::version(frame["version"].as_str().unwrap_or(""))?;
    let files = frame["files"]
        .as_object()
        .ok_or_else(|| Error::new("package", "Frame files required"))?;
    ensure(
        !files.is_empty() && files.len() <= 128,
        "package",
        "Frame requires 1..128 files",
    )?;
    for (name, value) in files {
        ensure(
            path_ok(name) && name != "frame-provenance.json" && value.is_string(),
            "package",
            "Invalid frame path or contents",
        )?;
        for other in files.keys() {
            ensure(
                !other.starts_with(&format!("{name}/")),
                "package",
                "Conflicting frame paths",
            )?;
        }
    }
    let pallets = frame["pallets"]
        .as_array()
        .ok_or_else(|| Error::new("package", "Frame pallets must be an array"))?;
    ensure(
        pallets.len() <= 16
            && pallets.iter().all(|p| {
                p.as_str().is_some_and(|s| {
                    s.rsplit_once('@').is_some_and(|(name, version)| {
                        catalog::app_name(name) && catalog::version(version).is_ok()
                    })
                })
            }),
        "package",
        "Invalid frame pallet references",
    )?;
    let apps = frame.get("apps").cloned().unwrap_or(json!([]));
    ensure(
        apps.as_array().is_some_and(|a| {
            a.len() <= 16 && a.iter().all(|v| v.as_str().is_some_and(catalog::app_name))
        }),
        "package",
        "Invalid frame app references",
    )?;
    ensure(
        !out.exists() && !out.is_symlink(),
        "conflict",
        "Frame destination already exists; existing files are never overwritten",
    )?;
    if apply {
        let parent = out
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staged = tempfile::tempdir_in(parent)?;
        for (name, text) in files {
            let file = staged.path().join(name);
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(file, text.as_str().unwrap())?;
        }
        store::write(
            &staged.path().join("frame-provenance.json"),
            &json!({"name":frame["name"],"version":frame["version"],"sha256":store::hash(&frame)}),
        )?;
        // Reserve the destination before moving files so a concurrent creator is never replaced.
        std::fs::create_dir(out)?;
        for entry in std::fs::read_dir(staged.path())? {
            let entry = entry?;
            std::fs::rename(entry.path(), out.join(entry.file_name()))?;
        }
    }
    Ok(
        json!({"frame":frame["name"],"version":frame["version"],"files":files.keys().collect::<Vec<_>>(),"pallets":pallets,"apps":apps,"destination":out,"applied":apply,"installed":false,"published":false}),
    )
}
