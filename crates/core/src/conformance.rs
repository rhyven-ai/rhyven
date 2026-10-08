//! Contract cases exercise package behavior through the same public runtime API.
use crate::{catalog, error::ensure, Error, Result, Runtime};
use serde_json::{json, Value};
pub fn run(package: &Value) -> Result<Value> {
    run_with_execution(package, false)
}
pub fn run_with_execution(package: &Value, allow_container: bool) -> Result<Value> {
    run_with_permissions(package, allow_container, false)
}
pub fn run_with_permissions(
    package: &Value,
    allow_container: bool,
    allow_host: bool,
) -> Result<Value> {
    run_with_dependencies(package, &[], allow_container, allow_host)
}
/// Fixtures are validated and installed in dependency order in an isolated collection.
pub fn run_with_dependencies(
    package: &Value,
    dependencies: &[Value],
    allow_container: bool,
    allow_host: bool,
) -> Result<Value> {
    catalog::validate(package)?;
    ensure(!crate::container::enabled(package) || allow_container, "permission_review_required", "Container tests execute publisher code. Review the package and use app test --allow-container")?;
    ensure(!(crate::script::enabled(package) || crate::native::enabled(package)) || allow_host, "permission_review_required", "Script tests run unsandboxed publisher code. Review the package and use app test --allow-host")?;
    ensure(
        package["hosting"]["mode"] == "local",
        "conformance",
        "Remote testing requires an explicitly configured endpoint; no network during package test",
    )?;
    let dir = tempfile::tempdir()?;
    let runtime = Runtime::new(dir.path(), "conformance")?;
    ensure(
        dependencies.len() <= 32,
        "conformance",
        "Too many dependency fixtures",
    )?;
    for dependency in dependencies {
        catalog::validate(dependency)?;
        ensure(
            dependency["hosting"]["mode"] == "local",
            "conformance",
            "Only local dependency fixtures are supported",
        )?;
        ensure(
            !crate::container::enabled(dependency) || allow_container,
            "permission_review_required",
            "Dependency tests require --allow-container",
        )?;
        ensure(
            !(crate::script::enabled(dependency) || crate::native::enabled(dependency))
                || allow_host,
            "permission_review_required",
            "Dependency tests require --allow-host",
        )?;
        ensure(
            !crate::services::enabled(dependency),
            "conformance",
            "Service dependencies require a separately managed integration test",
        )?;
        runtime.install(dependency, true, false)?;
    }
    runtime.install(package, true, false)?;
    let _supervisor = if crate::services::enabled(package) {
        Some(crate::services::ScopedSupervisor::start(
            &runtime,
            package["name"].as_str().unwrap(),
        )?)
    } else {
        None
    };
    let mut results = vec![];
    let cases = package["tests"].as_array().unwrap();
    ensure(
        !cases.is_empty(),
        "conformance",
        "At least one contract case required",
    )?;
    for case in cases {
        catalog::keys(case, &["operation", "args", "expect", "error"])?;
        let mut args = references(&case["args"], &results)?;
        args["app"] = package["name"].clone();
        let result = runtime.call(case["operation"].as_str().unwrap_or(""), args);
        if let Some(code) = case["error"].as_str() {
            let error = result.err().ok_or_else(|| {
                Error::new("conformance", "Expected failure but operation succeeded")
            })?;
            ensure(
                error.code == code,
                "conformance",
                format!("Expected {code}, got {}", error.code),
            )?;
            results.push(json!({"error":code}));
        } else {
            let result = result?;
            for (path, expected) in case["expect"].as_object().into_iter().flatten() {
                ensure(
                    at(&result, path) == Some(expected),
                    "conformance",
                    format!("Expectation failed: {path}"),
                )?;
            }
            results.push(result);
        }
    }
    Ok(json!({"app":package["name"],"cases":cases.len(),"passed":true,"certified":false}))
}
fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| v.get(k))
}
fn references(v: &Value, results: &[Value]) -> Result<Value> {
    if let Some(path) = v["$result"].as_str() {
        let (index, path) = path
            .split_once('.')
            .ok_or_else(|| Error::new("conformance", "Invalid result reference"))?;
        return index
            .parse::<usize>()
            .ok()
            .and_then(|i| results.get(i))
            .and_then(|v| at(v, path))
            .cloned()
            .ok_or_else(|| Error::new("conformance", "Missing result reference"));
    }
    match v {
        Value::Object(map) => Ok(Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), references(v, results)?)))
                .collect::<Result<_>>()?,
        )),
        Value::Array(values) => Ok(Value::Array(
            values
                .iter()
                .map(|v| references(v, results))
                .collect::<Result<_>>()?,
        )),
        _ => Ok(v.clone()),
    }
}

/// Copy installed contracts, never live app data, for explicitly requested local tests.
pub fn dependency_fixtures(runtime: &Runtime, package: &Value) -> Result<Vec<Value>> {
    crate::composition::check_dependencies(runtime, package)?;
    fn visit(
        r: &Runtime,
        p: &Value,
        seen: &mut std::collections::BTreeSet<String>,
        out: &mut Vec<Value>,
    ) -> Result<()> {
        for (_, pin) in p["dependencies"].as_object().into_iter().flatten() {
            let name = pin["app"].as_str().unwrap();
            if seen.insert(name.to_owned()) {
                let child = crate::composition::installed(r, name)?;
                visit(r, &child, seen, out)?;
                out.push(child);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(
        runtime,
        package,
        &mut std::collections::BTreeSet::new(),
        &mut out,
    )?;
    Ok(out)
}
