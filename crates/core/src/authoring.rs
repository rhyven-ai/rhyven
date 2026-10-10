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
    if let Some(actions) = p["actions"].as_object_mut() {
        for action in actions.values_mut() {
            if composition::is_workflow(action) {
                action["operation"] = json!("workflow");
            }
        }
    }
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
