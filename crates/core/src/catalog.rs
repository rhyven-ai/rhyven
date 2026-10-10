//! App package validation, catalog lookup and immutable version publication.
use crate::{error::ensure, schema, store, Error, Result};
use serde_json::{json, Value};
use std::path::Path;

pub fn app_name(s: &str) -> bool {
    let parts: Vec<_> = s.split('/').collect();
    parts.len() == 2 && parts.iter().all(|s| schema::name(s))
}
pub fn display_name(p: &Value) -> &str {
    p["display_name"]
        .as_str()
        .or_else(|| p["name"].as_str())
        .unwrap_or("")
}
pub fn publisher_label(p: &Value) -> &str {
    match p["publisher"].as_str().unwrap_or("") {
        "rhyven" => "Rhyven",
        publisher => publisher,
    }
}
pub fn version(s: &str) -> Result<[u64; 3]> {
    let parts = s
        .split('.')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::new("package", "Version must be numeric major.minor.patch"))?;
    parts
        .try_into()
        .map_err(|_| Error::new("package", "Version must be major.minor.patch"))
}
pub fn keys(value: &Value, allowed: &[&str]) -> Result<()> {
    let map = value
        .as_object()
        .ok_or_else(|| Error::new("validation", "Expected object"))?;
    for key in map.keys() {
        ensure(
            allowed.contains(&key.as_str()),
            "validation",
            format!("Unsupported field: {key}"),
        )?;
    }
    Ok(())
}
pub fn validate(p: &Value) -> Result<()> {
    keys(
        p,
        &[
            "dependencies",
            "format",
            "name",
            "display_name",
            "version",
            "description",
            "publisher",
            "source",
            "hosting",
            "permissions",
            "objects",
            "actions",
            "guide",
            "tests",
            "execution",
            "migrations",
            "health_action",
            "connector",
            "files",
            "libraries",
        ],
    )?;
    ensure(p["format"] == 2, "package", "Expected package format 2")?;
    crate::execution::validate(p)?;
    if crate::script::enabled(p) {
        crate::script::validate_files(p)?;
    } else {
        ensure(
            p.get("files").is_none(),
            "package",
            "Embedded files require script execution",
        )?;
    }
    crate::vendored::validate_libraries(p)?;
    crate::updates::validate(p)?;
    crate::composition::validate(p)?;
    let composition = crate::composition::enabled(p);
    let container = crate::container::enabled(p);
    let executable = crate::execution::enabled(p);
    let connector = crate::connector::enabled(p);
    crate::connector::validate(p)?;
    ensure(
        app_name(p["name"].as_str().unwrap_or("")),
        "package",
        "Use publisher/app-name",
    )?;
    version(p["version"].as_str().unwrap_or(""))?;
    if let Some(name) = p.get("display_name") {
        ensure(
            name.as_str().is_some_and(|s| {
                !s.trim().is_empty() && s.chars().count() <= 120 && !s.chars().any(char::is_control)
            }),
            "package",
            "display_name must be 1–120 characters without control characters",
        )?;
    }
    for key in ["description", "publisher", "guide"] {
        ensure(
            p[key].as_str().is_some_and(|s| !s.trim().is_empty()),
            "package",
            format!("Missing {key}"),
        )?;
    }
    keys(
        &p["hosting"],
        &[
            "mode", "endpoint", "auth_env", "auth", "privacy", "account", "billing", "domains",
        ],
    )?;
    let mode = p["hosting"]["mode"].as_str().unwrap_or("");
    ensure(
        ["local", "self-hosted", "remote"].contains(&mode),
        "package",
        "Unknown hosting mode",
    )?;
    let permissions = p["permissions"]
        .as_array()
        .ok_or_else(|| Error::new("package", "permissions array required"))?;
    let allowed = [
        "state.read",
        "state.write",
        "files.read",
        "files.write",
        "network.connect",
        "container.execute",
        "host.execute",
        "secrets.read",
        "service.run",
        "app.call",
    ];
    let mut seen = std::collections::BTreeSet::new();
    for permission in permissions {
        let s = permission.as_str().unwrap_or("");
        ensure(
            allowed.contains(&s) && seen.insert(s),
            "permission",
            "Unsupported or duplicate permission",
        )?;
    }
    ensure(
        composition || permissions.contains(&json!("service.run")) == crate::services::enabled(p),
        "permission",
        "Service mode requires service.run; other apps cannot request it",
    )?;
    let peer_calls = p["execution"]["calls"]
        .as_array()
        .is_some_and(|v| !v.is_empty());
    ensure(
        permissions.contains(&json!("app.call")) == (peer_calls || composition),
        "permission",
        "app.call must match declared service peer calls",
    )?;
    ensure(
        composition || permissions.contains(&json!("container.execute")) == container,
        "permission",
        "Container apps require container.execute; declarative apps cannot request it",
    )?;
    ensure(
        !permissions.contains(&json!("secrets.read")) || container || composition,
        "permission",
        "Secrets require container execution",
    )?;
    ensure(
        composition
            || permissions.contains(&json!("host.execute"))
                == (crate::script::enabled(p) || crate::native::enabled(p)),
        "permission",
        "Script apps require host.execute (unsandboxed access as your OS user)",
    )?;
    if executable {
        ensure(
            mode == "local",
            "package",
            "Executable apps require local hosting; use serve for shared access",
        )?;
        let secrets = p["execution"]["secrets"]
            .as_array()
            .is_some_and(|v| !v.is_empty());
        ensure(
            !secrets || permissions.contains(&json!("secrets.read")),
            "permission",
            "Declared secrets require secrets.read",
        )?;
    }
    ensure(
        permissions.contains(&json!("state.read")),
        "permission",
        "state.read required",
    )?;
    if mode != "local" {
        ensure(
            permissions.contains(&json!("network.connect")),
            "permission",
            "Remote apps require network.connect",
        )?;
        let url = reqwest::Url::parse(p["hosting"]["endpoint"].as_str().unwrap_or(""))
            .map_err(|_| Error::new("package", "Valid remote endpoint required"))?;
        ensure(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "package",
            "Endpoint must not embed credentials, query or fragment",
        )?;
        ensure(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && ["127.0.0.1", "localhost", "[::1]"].contains(&url.host_str().unwrap_or(""))),
            "package",
            "HTTPS required except loopback development",
        )?;
        for field in ["auth", "privacy", "account", "billing"] {
            ensure(
                p["hosting"][field]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty()),
                "package",
                format!("Remote disclosure required: {field}"),
            )?;
        }
        let domains = p["hosting"]["domains"]
            .as_array()
            .ok_or_else(|| Error::new("package", "domains required"))?;
        ensure(
            domains.len() == 1 && domains[0].as_str() == url.host_str(),
            "package",
            "Declare exactly the endpoint domain; redirects disabled",
        )?;
        if let Some(env) = p["hosting"].get("auth_env") {
            let env = env.as_str().unwrap_or("");
            ensure(
                env.starts_with("RHYVEN_TOKEN_")
                    && env
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
                "package",
                "Auth variable must use RHYVEN_TOKEN_ prefix",
            )?;
        }
    } else {
        ensure(
            executable || composition || !permissions.contains(&json!("network.connect")),
            "permission",
            "Local declarative apps cannot use network",
        )?;
        ensure(
            p["hosting"].as_object().unwrap().len() == 1,
            "package",
            "Local hosting only takes mode",
        )?;
    }
    let objects = p["objects"]
        .as_object()
        .ok_or_else(|| Error::new("package", "objects required"))?;
    ensure(
        (executable
            || connector
            || composition
            || p["actions"]
                .as_object()
                .is_some_and(|a| a.values().any(crate::files::is_action))
            || !objects.is_empty())
            && objects.len() <= 32,
        "package",
        "Provide 1..32 objects",
    )?;
    for (name, object) in objects {
        ensure(schema::name(name), "package", "Invalid object name")?;
        keys(
            object,
            &[
                "schema",
                "immutable",
                "relationships",
                "protected_fields",
                "transitions",
                "search_fields",
                "supersession_field",
            ],
        )?;
        schema::check(&object["schema"], 0)?;
        ensure(
            object["schema"]["type"] == "object",
            "package",
            "Object schema must be object",
        )?;
        if let Some(v) = object.get("immutable") {
            ensure(v.is_boolean(), "package", "immutable must be boolean")?;
        }
        let props = object["schema"]["properties"].as_object().unwrap();
        crate::query::validate_object(name, object)?;
        if let Some(fields) = object.get("protected_fields") {
            for field in fields
                .as_array()
                .ok_or_else(|| Error::new("package", "protected_fields must be array"))?
            {
                let f = field.as_str().unwrap_or("");
                ensure(
                    props.get(f).is_some_and(|s| s.get("default").is_some()),
                    "package",
                    "Protected fields require defaults",
                )?;
            }
        }
        if let Some(rels) = object.get("relationships") {
            for (field, target) in rels
                .as_object()
                .ok_or_else(|| Error::new("package", "relationships must be object"))?
            {
                keys(target, &["app", "object"])?;
                ensure(
                    props.get(field).is_some_and(|s| s["type"] == "string"),
                    "package",
                    "Relationship fields must be strings",
                )?;
                if let Some(app) = target.get("app") {
                    ensure(
                        app_name(app.as_str().unwrap_or("")),
                        "package",
                        "Invalid relationship app",
                    )?;
                }
                if target.get("app").is_none() || target["app"] == p["name"] {
                    ensure(
                        objects.contains_key(target["object"].as_str().unwrap_or("")),
                        "package",
                        "Unknown relationship object",
                    )?;
                }
            }
        }
        if let Some(transitions) = object.get("transitions") {
            for (field, states) in transitions
                .as_object()
                .ok_or_else(|| Error::new("package", "transitions must be object"))?
            {
                ensure(
                    props.get(field).is_some_and(|s| s["type"] == "string"),
                    "package",
                    "Transitions require string field",
                )?;
                let choices = props[field]["enum"]
                    .as_array()
                    .ok_or_else(|| Error::new("package", "Transition field requires enum"))?;
                for (from, to) in states
                    .as_object()
                    .ok_or_else(|| Error::new("package", "Transitions must map states to arrays"))?
                {
                    ensure(
                        choices.contains(&json!(from)),
                        "package",
                        "Unknown transition origin",
                    )?;
                    let to = to.as_array().ok_or_else(|| {
                        Error::new("package", "Transition targets must be arrays")
                    })?;
                    ensure(
                        to.iter().all(|v| choices.contains(v)),
                        "package",
                        "Unknown transition target",
                    )?;
                }
            }
        }
    }
    let actions = p["actions"]
        .as_object()
        .ok_or_else(|| Error::new("package", "actions required"))?;
    ensure(
        !executable || !actions.is_empty(),
        "package",
        "Executable apps require at least one action",
    )?;
    for (name, action) in actions {
        ensure(schema::name(name), "package", "Invalid action name")?;
        if let Some(keywords) = action.get("keywords") {
            ensure(
                keywords.as_array().is_some_and(|v| {
                    v.len() <= 16
                        && v.iter().all(|k| {
                            k.as_str()
                                .is_some_and(|s| !s.trim().is_empty() && s.len() <= 64)
                        })
                }),
                "package",
                "Action keywords must be at most 16 nonempty strings of at most 64 bytes",
            )?;
        }
        if crate::composition::is_workflow(action) {
            crate::composition::validate_action(p, action)?;
            continue;
        }
        if crate::files::is_action(action) {
            crate::files::validate_action(p, action)?;
            continue;
        }
        if connector {
            continue;
        }
        if executable {
            keys(action, &["description", "keywords", "input", "output"])?;
            ensure(
                action["description"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()),
                "package",
                "Action description required",
            )?;
            for field in ["input", "output"] {
                schema::check(&action[field], 0)?;
                ensure(
                    action[field]["type"] == "object",
                    "package",
                    "Executable action inputs and outputs must be objects",
                )?;
            }
            continue;
        }
        keys(
            action,
            &[
                "description",
                "keywords",
                "input",
                "object",
                "operation",
                "set",
                "guard",
                "id_arg",
                "expressions",
                "condition",
            ],
        )?;
        schema::check(&action["input"], 0)?;
        ensure(
            action["input"]["type"] == "object",
            "package",
            "Action input must be object",
        )?;
        let object = action["object"].as_str().unwrap_or("");
        ensure(
            objects.contains_key(object),
            "package",
            "Unknown action object",
        )?;
        let operation = action["operation"].as_str().unwrap_or("");
        ensure(
            ["create", "update", "get", "query"].contains(&operation),
            "package",
            "Unsupported declarative operation",
        )?;
        ensure(
            operation == "update" || action.get("guard").is_none(),
            "package",
            "Guards are supported only for update actions",
        )?;
        ensure(
            operation != "get" || action.get("set").is_none(),
            "package",
            "Get actions do not accept set",
        )?;
        if ["get", "update"].contains(&operation) {
            let id = action["id_arg"].as_str().unwrap_or("id");
            ensure(
                action["input"]["properties"][id]["type"] == "string",
                "package",
                "Action requires id input",
            )?;
        }
        if operation == "update" {
            ensure(
                action["input"]["properties"]["expected_revision"]["type"] == "integer",
                "package",
                "Update actions require expected_revision input",
            )?;
        }
        crate::expressions::validate_action(action, &objects[object]["schema"])?;
        ensure(
            p["hosting"]["mode"] == "local"
                || (action.get("expressions").is_none() && action.get("condition").is_none()),
            "expression",
            "Expression actions currently require local hosting",
        )?;
        for key in ["set", "guard"] {
            if let Some(mapping) = action.get(key) {
                for (field, template) in mapping
                    .as_object()
                    .ok_or_else(|| Error::new("package", "set/guard must be objects"))?
                {
                    ensure(
                        objects[object]["schema"]["properties"].get(field).is_some(),
                        "package",
                        "Unknown action target field",
                    )?;
                    if let Some(arg) = template.get("$arg") {
                        ensure(
                            template.as_object().is_some_and(|o| o.len() == 1)
                                && action["input"]["properties"]
                                    .get(arg.as_str().unwrap_or(""))
                                    .is_some(),
                            "package",
                            "Invalid argument reference",
                        )?;
                    }
                }
            }
        }
    }
    ensure(p["tests"].is_array(), "package", "tests array required")?;
    for case in p["tests"].as_array().unwrap() {
        keys(case, &["operation", "args", "expect", "error"])?;
        ensure(
            case["args"].is_object(),
            "package",
            "Test args must be an object",
        )?;
        ensure(
            ["query", "get", "create", "update", "execute"]
                .contains(&case["operation"].as_str().unwrap_or("")),
            "package",
            "Unknown test operation",
        )?;
        if let Some(expect) = case.get("expect") {
            ensure(
                expect.is_object(),
                "package",
                "Test expect must be an object",
            )?;
        }
        if let Some(error) = case.get("error") {
            ensure(
                error.is_string(),
                "package",
                "Test error must be a code string",
            )?;
        }
    }
    Ok(())
}
pub fn read(path: &Path) -> Result<Value> {
    let directory = path.is_dir().then(|| path.to_owned());
    let path = if path.is_dir() {
        path.join("app.json")
    } else {
        path.to_owned()
    };
    ensure(
        std::fs::metadata(&path)?.len() <= 1_048_576,
        "package",
        "Package exceeds 1 MiB",
    )?;
    let mut value = serde_json::from_slice(&std::fs::read(path)?)?;
    if crate::script::enabled(&value) {
        if let Some(directory) = directory {
            crate::script::bundle(&directory, &mut value)?;
        }
    }
    validate(&value)?;
    Ok(value)
}
pub fn bundled() -> Vec<Value> {
    [
        include_str!("../../../catalog/work-management.json"),
        include_str!("../../../catalog/project-knowledge.json"),
        include_str!("../../../catalog/error-management.json"),
        include_str!("../../../catalog/ci-management.json"),
        include_str!("../../../catalog/inventory.json"),
    ]
    .iter()
    .map(|s| serde_json::from_str(s).expect("bundled package JSON"))
    .collect()
}
pub fn list(root: &Path) -> Result<Vec<Value>> {
    let mut all = bundled();
    for p in crate::registry::cached(root)? {
        if let Some(old) = all
            .iter()
            .find(|v| v["name"] == p["name"] && v["version"] == p["version"])
        {
            ensure(old == &p, "integrity", "Conflicting registry version")?;
        } else {
            all.push(p);
        }
    }
    let dir = crate::collections::registry_dir(root)?.join("registry");
    if dir.exists() {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|s| s == "json") {
                let p = read_cached(root, &path)?;
                if let Some(existing) = all
                    .iter()
                    .find(|v| v["name"] == p["name"] && v["version"] == p["version"])
                {
                    ensure(existing == &p, "integrity", "Conflicting registry version")?;
                } else {
                    all.push(p);
                }
            }
        }
    }
    all.sort_by_key(|v| {
        (
            v["name"].as_str().unwrap().to_owned(),
            version(v["version"].as_str().unwrap()).unwrap(),
        )
    });
    Ok(all)
}
pub fn resolve(root: &Path, selector: &str) -> Result<Value> {
    if Path::new(selector).exists() {
        return read(Path::new(selector));
    }
    let (name, version) = selector
        .split_once('@')
        .map_or((selector, None), |(a, b)| (a, Some(b)));
    list(root)?
        .into_iter()
        .rev()
        .find(|p| p["name"] == name && version.is_none_or(|v| p["version"] == v))
        .ok_or_else(|| Error::new("not_found", "Package not found"))
}
pub fn publish(root: &Path, p: &Value) -> Result<Value> {
    validate(p)?;
    for old in list(root)? {
        if old["name"] == p["name"] && old["version"] == p["version"] {
            ensure(
                old == *p,
                "immutable_version",
                "Version already published with different content",
            )?;
        }
    }
    let dir = crate::collections::registry_dir(root)?.join("registry");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!(
        "{}.json",
        store::hash(&json!([p["name"], p["version"]]))
    ));
    // No overwrite: concurrent publishers cannot silently replace an immutable version.
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
    tmp.write_all(serde_json::to_string_pretty(&crate::collections::save(root, p)?)?.as_bytes())?;
    tmp.as_file().sync_all()?;
    match tmp.persist_noclobber(&file) {
        Ok(_) => (),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => ensure(
            read_cached(root, &file)? == *p,
            "immutable_version",
            "Concurrent conflicting publication",
        )?,
        Err(e) => return Err(Error::new("io", e.to_string())),
    }
    Ok(
        json!({"published":p["name"], "version":p["version"], "sha256":store::hash(p), "registry":dir, "trust":"Unverified", "scope":"local registry; no remote push"}),
    )
}

fn read_cached(root: &Path, path: &Path) -> Result<Value> {
    ensure(
        std::fs::metadata(path)?.len() <= 1_048_576,
        "package",
        "Package exceeds 1 MiB",
    )?;
    let p = crate::collections::load(root, &serde_json::from_slice(&std::fs::read(path)?)?)?;
    validate(&p)?;
    Ok(p)
}
