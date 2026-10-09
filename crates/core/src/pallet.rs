//! Portable source libraries. Saving a pallet does not register or install an app.
use crate::{catalog, error::ensure, schema, store, Error, Result, Runtime};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{io::Read, path::Path, process::Command};

const MAX: usize = 1_048_576;
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 200
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-/.".contains(&b))
        && path.split('/').all(|s| {
            !s.is_empty() && !s.starts_with('.') && !matches!(s, "node_modules" | "__pycache__")
        })
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    crate::runtime::string(v, k)
}
fn read_json(path: &Path) -> Result<Value> {
    ensure(
        std::fs::symlink_metadata(path)?.is_file(),
        "pallet",
        "Expected a regular file",
    )?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure(bytes.len() <= MAX, "pallet", "Pallet exceeds 1 MiB")?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn read(path: &Path) -> Result<Value> {
    let mut p = read_json(&if path.is_dir() {
        path.join("pallet.json")
    } else {
        path.to_path_buf()
    })?;
    if p["files"].is_array() {
        ensure(
            path.is_dir(),
            "pallet",
            "File references require a pallet directory",
        )?;
        ensure(
            p["files"].as_array().unwrap().len() <= 128,
            "pallet",
            "At most 128 files",
        )?;
        let base = std::fs::canonicalize(path)?;
        let mut files = serde_json::Map::new();
        let mut total = 0;
        for name in p["files"].as_array().unwrap() {
            let name = name
                .as_str()
                .ok_or_else(|| Error::new("pallet", "File names must be strings"))?;
            ensure(safe_path(name), "pallet", "Unsafe file path")?;
            let mut cursor = base.clone();
            for part in name.split('/') {
                cursor.push(part);
                ensure(
                    !std::fs::symlink_metadata(&cursor)?.file_type().is_symlink(),
                    "pallet",
                    "Symlink source is not supported",
                )?;
            }
            ensure(
                std::fs::metadata(&cursor)?.is_file(),
                "pallet",
                "Expected a source file",
            )?;
            let mut bytes = Vec::new();
            std::fs::File::open(cursor)?
                .take((MAX + 1) as u64)
                .read_to_end(&mut bytes)?;
            total += bytes.len();
            ensure(total <= MAX, "pallet", "Source exceeds 1 MiB")?;
            let source = String::from_utf8(bytes)
                .map_err(|_| Error::new("pallet", "Source files must be UTF-8"))?;
            ensure(
                files.insert(name.into(), json!(source)).is_none(),
                "pallet",
                "Duplicate file",
            )?;
        }
        p["files"] = json!(files);
    }
    validate(&p)?;
    Ok(p)
}
pub fn validate(p: &Value) -> Result<()> {
    catalog::keys(
        p,
        &[
            "format",
            "name",
            "version",
            "description",
            "language",
            "license",
            "files",
            "exports",
            "tests",
            "dependencies",
        ],
    )?;
    ensure(
        p["format"] == "rhyven.pallet/1",
        "pallet",
        "Expected a portable rhyven.pallet/1 library, not an app",
    )?;
    ensure(
        catalog::app_name(text(p, "name")?),
        "pallet",
        "Use publisher/library",
    )?;
    catalog::version(text(p, "version")?)?;
    ensure(
        !text(p, "description")?.trim().is_empty() && text(p, "description")?.len() <= 500,
        "pallet",
        "Description must be 1..500 bytes",
    )?;
    ensure(
        schema::name(text(p, "language")?),
        "pallet",
        "Language must be an identifier",
    )?;
    if let Some(license) = p.get("license") {
        ensure(
            license.as_str().is_some_and(|s| s.len() <= 100),
            "pallet",
            "Invalid license",
        )?;
    }
    let files = p["files"]
        .as_object()
        .ok_or_else(|| Error::new("pallet", "Source files required"))?;
    ensure(
        !files.is_empty() && files.len() <= 128 && p.to_string().len() <= MAX,
        "pallet",
        "Pallet requires 1..128 files within 1 MiB",
    )?;
    for (path, value) in files {
        ensure(
            safe_path(path)
                && value.is_string()
                && !matches!(
                    path.as_str(),
                    "pallet.json" | "pallet.lock.json" | "run.py" | "run.mjs"
                ),
            "pallet",
            "Unsafe or reserved source file",
        )?;
        ensure(
            !files
                .keys()
                .any(|other| other.starts_with(&format!("{path}/"))),
            "pallet",
            "Conflicting file paths",
        )?;
    }
    if let Some(deps) = p.get("dependencies") {
        ensure(
            deps.as_array().is_some_and(|v| {
                v.len() <= 32 && v.iter().all(|s| s.as_str().is_some_and(|s| s.len() <= 200))
            }),
            "pallet",
            "Dependencies must be up to 32 descriptive strings",
        )?;
    }
    let exports = p["exports"]
        .as_object()
        .ok_or_else(|| Error::new("pallet", "exports required"))?;
    ensure(
        !exports.is_empty() && exports.len() <= 32,
        "pallet",
        "Export 1..32 bricks, mortar functions or stacks",
    )?;
    for (name, e) in exports {
        catalog::keys(
            e,
            &[
                "kind",
                "description",
                "file",
                "symbol",
                "input",
                "output",
                "keywords",
            ],
        )?;
        ensure(
            schema::name(name) && matches!(e["kind"].as_str(), Some("brick" | "mortar" | "stack")),
            "pallet",
            "Invalid export kind/name",
        )?;
        ensure(
            e.to_string().len() <= 8192
                && text(e, "description")?.len() <= 500
                && !text(e, "description")?.trim().is_empty(),
            "pallet",
            "Export contract exceeds limits",
        )?;
        let file = text(e, "file")?;
        ensure(
            files.contains_key(file) && identifier(text(e, "symbol")?),
            "pallet",
            "Export must name a packaged source file and symbol",
        )?;
        if p["language"] == "python" {
            ensure(
                file.ends_with(".py"),
                "pallet",
                "Python exports require .py files",
            )?;
        }
        if p["language"] == "javascript" {
            ensure(
                file.ends_with(".mjs"),
                "pallet",
                "JavaScript exports require ES module .mjs files",
            )?;
        }
        schema::check(&e["input"], 0)?;
        schema::check(&e["output"], 0)?;
        if let Some(tags) = e.get("keywords") {
            ensure(
                tags.as_array().is_some_and(|v| {
                    v.len() <= 16 && v.iter().all(|t| t.as_str().is_some_and(|s| s.len() <= 64))
                }),
                "pallet",
                "Invalid keywords",
            )?;
        }
    }
    let cases = p["tests"]
        .as_array()
        .ok_or_else(|| Error::new("pallet", "tests array required"))?;
    ensure(cases.len() <= 64, "pallet", "At most 64 test cases")?;
    for case in cases {
        catalog::keys(case, &["export", "args", "expect"])?;
        let e = exports
            .get(text(case, "export")?)
            .ok_or_else(|| Error::new("pallet", "Unknown test export"))?;
        schema::validate(case["args"].clone(), &e["input"])?;
        ensure(
            case.get("expect").is_some(),
            "pallet",
            "Test must contain expected output",
        )?;
        schema::validate(case["expect"].clone(), &e["output"])?;
    }
    Ok(())
}
fn db(r: &Runtime) -> Result<rusqlite::Connection> {
    let db = store::open(&r.root)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS pallets(name TEXT NOT NULL, version TEXT NOT NULL, digest TEXT NOT NULL, package TEXT NOT NULL, PRIMARY KEY(name,version))")?;
    Ok(db)
}
fn save_local(r: &Runtime, p: &Value) -> Result<Value> {
    validate(p)?;
    let _guard = crate::maintenance::lock(&r.root)?;
    let db = db(r)?;
    let hash = store::hash(p);
    let name = text(p, "name")?;
    let version = text(p, "version")?;
    let old = db
        .query_row(
            "SELECT digest FROM pallets WHERE name=?1 AND version=?2",
            params![name, version],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(old) = old {
        ensure(
            old == hash,
            "immutable_version",
            "Saved pallet changed; use a new version",
        )?;
    } else {
        let count: u64 = db.query_row("SELECT count(*) FROM pallets", [], |r| r.get(0))?;
        ensure(
            count < 200,
            "pallet",
            "Collection pallet limit is 200 versions",
        )?;
        db.execute(
            "INSERT INTO pallets VALUES(?1,?2,?3,?4)",
            params![name, version, hash, p.to_string()],
        )?;
    }
    Ok(
        json!({"saved":format!("{name}@{version}"),"sha256":hash,"kind":"pallet","installed_app":false,"published":false}),
    )
}
fn resolve_local(r: &Runtime, selector: &str) -> Result<Value> {
    let (name, version) = selector
        .rsplit_once('@')
        .ok_or_else(|| Error::new("pallet", "Use an exact library@version selector"))?;
    let (raw, hash): (String, String) = db(r)?
        .query_row(
            "SELECT package,digest FROM pallets WHERE name=?1 AND version=?2",
            params![name, version],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::new("not_found", "Pallet is not saved in this collection"))?;
    let p: Value = serde_json::from_str(&raw)?;
    validate(&p)?;
    ensure(
        store::hash(&p) == hash,
        "integrity",
        "Saved pallet hash mismatch",
    )?;
    Ok(p)
}
fn packages_local(r: &Runtime) -> Result<Vec<Value>> {
    let db = db(r)?;
    let mut statement =
        db.prepare("SELECT package,digest FROM pallets ORDER BY name,version LIMIT 200")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (raw, hash) = row?;
        let p: Value = serde_json::from_str(&raw)?;
        validate(&p)?;
        ensure(
            store::hash(&p) == hash,
            "integrity",
            "Saved pallet hash mismatch",
        )?;
        Ok(p)
    })
    .collect()
}
pub fn describe(p: &Value, export: Option<&str>, if_hash: Option<&str>) -> Result<Value> {
    let hash = store::hash(p);
    if if_hash == Some(&hash) {
        return Ok(
            json!({"kind":"pallet","name":p["name"],"version":p["version"],"contract_hash":hash,"unchanged":true}),
        );
    }
    let exports = if let Some(name) = export {
        let e = p["exports"]
            .get(name)
            .ok_or_else(|| Error::new("not_found", "Unknown pallet export"))?;
        json!([{ "name":name,"contract":e }])
    } else {
        json!(p["exports"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, e)| json!({"name":name,"kind":e["kind"],"description":e["description"]}))
            .collect::<Vec<_>>())
    };
    Ok(
        json!({"kind":"pallet","name":p["name"],"version":p["version"],"description":p["description"],"language":p["language"],"contract_hash":hash,"exports":exports,"dependencies":p.get("dependencies").cloned().unwrap_or(json!([])),"usage":"Export once, import the ordinary source functions in your project, and reuse by hash. Source is omitted from discovery. This is not an installed app."}),
    )
}
/// Workspace binding is explicit; collections do not imply project directories.
fn scopes(r: &Runtime) -> Result<Vec<(&'static str, Runtime)>> {
    let mut result = Vec::new();
    if let Some(path) = &r.pallet_workspace {
        result.push((
            "workspace",
            Runtime::new(std::fs::canonicalize(path)?, &r.actor)?,
        ));
    }
    result.push(("collection", r.clone()));
    if let Some(home) = crate::collections::home_for(&r.root)? {
        let global = Runtime::collection(home, "global", &r.actor)?;
        if global.root != r.root {
            result.push(("global", global));
        } else if let Some(last) = result.last_mut() {
            last.0 = "global";
        }
    }
    Ok(result)
}
pub fn scope_root(r: &Runtime, scope: &str) -> Result<std::path::PathBuf> {
    let stores = scopes(r)?;
    let scope = if scope == "default" {
        if r.pallet_workspace.is_some() {
            "workspace"
        } else {
            "collection"
        }
    } else {
        scope
    };
    let (_, target) = stores.iter().find(|(s,t)| *s == scope || (scope == "collection" && *s == "global" && r.root == t.root))
        .ok_or_else(|| Error::new("pallet", "Scope unavailable: bind --project for workspace or use a named collection for global"))?;
    Ok(target.root.clone())
}
pub fn save_scoped(r: &Runtime, p: &Value, scope: &str) -> Result<Value> {
    let root = scope_root(r, scope)?;
    let target = Runtime::new(&root, &r.actor)?;
    let mut value = save_local(&target, p)?;
    value["scope"] = json!(if scope == "default" {
        if r.pallet_workspace.is_some() {
            "workspace"
        } else {
            "collection"
        }
    } else {
        scope
    });
    value["location"] = json!(root);
    Ok(value)
}
pub fn save(r: &Runtime, p: &Value) -> Result<Value> {
    save_scoped(r, p, "default")
}
pub fn scoped_packages(r: &Runtime) -> Result<Vec<(String, Value)>> {
    let mut result = Vec::new();
    for (scope, target) in scopes(r)? {
        for p in packages_local(&target)? {
            result.push((scope.to_owned(), p));
        }
    }
    Ok(result)
}
pub fn resolve(r: &Runtime, selector: &str) -> Result<Value> {
    let (scope, selector) = selector
        .split_once("::")
        .map(|(a, b)| (Some(a), b))
        .unwrap_or((None, selector));
    let mut found: Option<Value> = None;
    for (label, target) in scopes(r)? {
        if scope.is_some_and(|s| s != label) {
            continue;
        }
        match resolve_local(&target, selector) {
            Ok(p) => {
                if let Some(old) = &found {
                    ensure(old == &p,"pallet_conflict","Different contents exist for this version; use workspace::, collection:: or global:: before the selector")?;
                } else {
                    found = Some(p);
                }
            }
            Err(e) if e.code == "not_found" => (),
            Err(e) => return Err(e),
        }
    }
    found.ok_or_else(|| {
        Error::new(
            "not_found",
            "Pallet not found in selected workspace, collection or global library",
        )
    })
}
pub fn packages(r: &Runtime) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for (_, p) in scoped_packages(r)? {
        if !result.contains(&p) {
            result.push(p);
        }
    }
    Ok(result)
}
pub fn list(r: &Runtime) -> Result<Value> {
    Ok(json!(scoped_packages(r)?.iter().map(|(scope,p)|json!({"scope":scope,"selector":format!("{scope}::{}@{}",p["name"].as_str().unwrap(),p["version"].as_str().unwrap()),"name":p["name"],"version":p["version"],"language":p["language"],"description":p["description"],"sha256":store::hash(p)})).collect::<Vec<_>>()))
}
const PYTHON_RUNNER: &str = r#"import importlib
import json
from pathlib import Path
import sys

root = Path(__file__).resolve().parent
manifest = json.loads((root / "pallet.json").read_text())
request = json.loads(sys.stdin.readline())
entry = manifest["exports"][request["export"]]
sys.path.insert(0, str(root))
module = importlib.import_module(entry["file"][:-3].replace("/", "."))
result = getattr(module, entry["symbol"])(request["args"])
print(json.dumps({"result": result}, allow_nan=False))
"#;
const JS_RUNNER: &str = r#"import fs from 'node:fs';
import readline from 'node:readline';
const root = new URL('./', import.meta.url);
const manifest = JSON.parse(fs.readFileSync(new URL('pallet.json', root), 'utf8'));
const lines = readline.createInterface({input:process.stdin});
for await (const line of lines) {
  const request=JSON.parse(line);
  const entry=manifest.exports[request.export];
  const module=await import(new URL(entry.file,root));
  const result=await module[entry.symbol](request.args);
  process.stdout.write(JSON.stringify({result})+'\n');
  break;
}
"#;
pub fn export(p: &Value, out: &Path) -> Result<Value> {
    validate(p)?;
    ensure(
        !out.exists() && !out.is_symlink(),
        "conflict",
        "Export destination must be a new directory",
    )?;
    let mut files = p["files"].clone();
    let mut manifest = p.clone();
    manifest["files"] = json!(p["files"].as_object().unwrap().keys().collect::<Vec<_>>());
    files["pallet.json"] = json!(serde_json::to_string_pretty(&manifest)?);
    files["pallet.lock.json"] = json!(
        json!({"format":1,"name":p["name"],"version":p["version"],"sha256":store::hash(p)})
            .to_string()
    );
    if p["language"] == "python" {
        files["run.py"] = json!(PYTHON_RUNNER);
    }
    if p["language"] == "javascript" {
        files["run.mjs"] = json!(JS_RUNNER);
    }
    // Reuse the traversal/overwrite-safe frame writer; generated files are inert.
    let frame = json!({"format":"rhyven.frame/1","name":p["name"],"version":p["version"],"files":files,"pallets":[]});
    let dir = tempfile::tempdir()?;
    let definition = dir.path().join("frame.json");
    std::fs::write(&definition, frame.to_string())?;
    crate::authoring::frame(&definition, out, true)?;
    Ok(
        json!({"exported":out,"sha256":store::hash(p),"requires_rhyven":false,"language":p["language"],"dependencies_installed":false}),
    )
}
fn invoke(
    p: &Value,
    out: &Path,
    name: &str,
    args: Value,
    interpreter: Option<&Path>,
) -> Result<Value> {
    let e = p["exports"]
        .get(name)
        .ok_or_else(|| Error::new("not_found", "Unknown export"))?;
    let args = schema::validate(args, &e["input"])?;
    let (default, launcher) =
        match text(p, "language")? {
            "python" => ("python3", "run.py"),
            "javascript" => ("node", "run.mjs"),
            _ => return Err(Error::new(
                "pallet",
                "This language supports source export; compile/import it with its normal toolchain",
            )),
        };
    ensure(
        p["dependencies"].as_array().is_none_or(|d| d.is_empty()) || interpreter.is_some(),
        "dependency",
        "Pallet dependencies require an explicitly prepared --interpreter; no automatic installs",
    )?;
    let program = match interpreter {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => std::env::current_dir()?.join(path),
        None => crate::script::executable(default)?,
    };
    let mut probe = Command::new(&program);
    probe
        .arg("--version")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    let version = crate::script::run(&mut probe, vec![], 15)?;
    let version = String::from_utf8_lossy(&version);
    let parts: Vec<_> = version
        .trim()
        .trim_start_matches("Python ")
        .trim_start_matches('v')
        .split('.')
        .collect();
    let actual = parts
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .zip(parts.get(1).and_then(|s| s.parse::<u64>().ok()));
    let minimum = if default == "python3" {
        (3, 10)
    } else {
        (20, 0)
    };
    ensure(
        actual.is_some_and(|v| v >= minimum),
        "script_unavailable",
        format!(
            "{default} requires at least {}.{}; select a compatible --interpreter",
            minimum.0, minimum.1
        ),
    )?;
    let mut command = Command::new(&program);
    command
        .arg(out.join(launcher))
        .current_dir(out)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", out)
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1");
    let mut request = serde_json::to_vec(&json!({"export":name,"args":args}))?;
    request.push(b'\n');
    let bytes = crate::script::run(&mut command, request, 30)?;
    let response: Value = serde_json::from_slice(&bytes)?;
    catalog::keys(&response, &["result"])?;
    schema::validate(response["result"].clone(), &e["output"])
}
pub fn run(
    p: &Value,
    name: &str,
    args: Value,
    allow_host: bool,
    interpreter: Option<&Path>,
) -> Result<Value> {
    validate(p)?;
    ensure(
        allow_host,
        "permission_review_required",
        "Review source before --allow-host: unsandboxed code runs as your OS user",
    )?;
    let dir = tempfile::tempdir()?;
    let out = dir.path().join("source");
    export(p, &out)?;
    invoke(p, &out, name, args, interpreter)
}
// Evidence stays outside the portable package and is local to this user's home.
fn evidence_db(r: &Runtime) -> Result<rusqlite::Connection> {
    let target = if let Some(home) = crate::collections::home_for(&r.root)? {
        Runtime::collection(home, "global", &r.actor)?
    } else {
        r.clone()
    };
    let db = store::open(&target.root)?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS pallet_tests(digest TEXT PRIMARY KEY, report TEXT NOT NULL)",
    )?;
    Ok(db)
}
pub fn test_evidence(r: &Runtime, p: &Value, export: Option<&str>) -> Result<Value> {
    if let Some(name) = export {
        ensure(
            p["exports"].get(name).is_some(),
            "not_found",
            "Unknown pallet export",
        )?;
    }
    let raw: Option<String> = evidence_db(r)?
        .query_row(
            "SELECT report FROM pallet_tests WHERE digest=?1",
            [store::hash(p)],
            |row| row.get(0),
        )
        .optional()?;
    let mut report = match raw {
        Some(raw) => serde_json::from_str::<Value>(&raw)?,
        None => return Ok(json!({"status":"not_run","origin":"local","certified":false})),
    };
    if let Some(name) = export {
        let detail = report["exports"][name].clone();
        report.as_object_mut().unwrap().remove("exports");
        report["coverage"] = detail.clone();
        if detail["total"] == 0 {
            report["status"] = json!("not_covered");
        } else if detail["passed"] != detail["total"] {
            report["status"] = json!("failed");
        }
    }
    if export.is_some() {
        report["passed"] = json!(report["status"] == "passed");
    }
    Ok(report)
}
pub fn describe_recorded(
    r: &Runtime,
    p: &Value,
    export: Option<&str>,
    if_hash: Option<&str>,
) -> Result<Value> {
    let mut result = describe(p, export, if_hash)?;
    // An unchanged contract does not imply unchanged test evidence.
    result["tests"] = test_evidence(r, p, export)?;
    Ok(result)
}
pub fn test(p: &Value, allow_host: bool, interpreter: Option<&Path>) -> Result<Value> {
    test_inner(None, p, allow_host, interpreter)
}
pub fn test_recorded(
    r: &Runtime,
    p: &Value,
    allow_host: bool,
    interpreter: Option<&Path>,
) -> Result<Value> {
    test_inner(Some(r), p, allow_host, interpreter)
}
fn test_inner(
    r: Option<&Runtime>,
    p: &Value,
    allow_host: bool,
    interpreter: Option<&Path>,
) -> Result<Value> {
    validate(p)?;
    ensure(
        allow_host,
        "permission_review_required",
        "Pallet tests execute source; review before --allow-host",
    )?;
    let cases = p["tests"].as_array().unwrap();
    ensure(!cases.is_empty(), "pallet", "At least one test required")?;
    let mut coverage = serde_json::Map::new();
    for name in p["exports"].as_object().unwrap().keys() {
        coverage.insert(name.clone(), json!({"total":0,"passed":0}));
    }
    for case in cases {
        let row = &mut coverage[text(case, "export")?];
        row["total"] = json!(row["total"].as_u64().unwrap() + 1);
    }
    let mut passed = 0;
    let mut interpreter_info = json!(null);
    let outcome = (|| -> Result<()> {
        let program = match interpreter {
            Some(path) => std::fs::canonicalize(path)?,
            None => crate::script::executable(if p["language"] == "python" {
                "python3"
            } else {
                "node"
            })?,
        };
        let mut probe = Command::new(&program);
        probe
            .arg("--version")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default());
        let version = crate::script::run(&mut probe, vec![], 15)?;
        interpreter_info = json!({"path":program,"version":String::from_utf8_lossy(&version).trim(),"os":std::env::consts::OS,"arch":std::env::consts::ARCH});
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("source");
        export(p, &out)?;
        for case in cases {
            let name = text(case, "export")?;
            let result = invoke(p, &out, name, case["args"].clone(), Some(&program))?;
            ensure(
                result == case["expect"],
                "conformance",
                format!("Output differs for {name}"),
            )?;
            passed += 1;
            let row = &mut coverage[name];
            row["passed"] = json!(row["passed"].as_u64().unwrap() + 1);
        }
        Ok(())
    })();
    let report = json!({"pallet":p["name"],"sha256":store::hash(p),"passed":outcome.is_ok(),"status":if outcome.is_ok(){"passed"}else{"failed"},"cases":cases.len(),"passed_cases":passed,"exports":coverage,"timestamp":crate::marketplace::now(),"interpreter":interpreter_info,"origin":"local","certified":false,"scope":"declared examples only","error_code":outcome.as_ref().err().map(|e| &e.code)});
    if let Some(r) = r {
        evidence_db(r)?.execute("INSERT INTO pallet_tests VALUES(?1,?2) ON CONFLICT(digest) DO UPDATE SET report=excluded.report", params![store::hash(p),report.to_string()])?;
    }
    outcome?;
    Ok(report)
}

/// Vendor reviewed source into a complete script app. No library becomes an app.
pub fn bundle(
    r: &Runtime,
    app: &Value,
    references: &[(String, String)],
    out: &Path,
) -> Result<Value> {
    catalog::validate(app)?;
    ensure(crate::script::enabled(app),"pallet","Source bundling currently targets Python/JavaScript apps; other toolchains can consume exported source directly")?;
    ensure(
        !references.is_empty() && references.len() <= 16,
        "pallet",
        "Bundle 1..16 pallets",
    )?;
    let mut p = app.clone();
    if p.get("libraries").is_none() {
        p["libraries"] = json!({});
    }
    for (alias, selector) in references {
        ensure(
            identifier(alias) && p["libraries"].get(alias).is_none(),
            "pallet",
            "Library alias must be unique and a module identifier",
        )?;
        let library = resolve(r, selector)?;
        ensure(
            library["language"] == p["execution"]["language"],
            "pallet",
            "Cross-language use needs an explicit adapter or native binding",
        )?;
        ensure(
            library["dependencies"]
                .as_array()
                .is_none_or(|v| v.is_empty())
                || p["execution"].get("dependencies").is_some(),
            "dependency",
            "Declare and lock app dependencies before bundling this library",
        )?;
        let mut hashes = serde_json::Map::new();
        for (file, contents) in library["files"].as_object().unwrap() {
            let path = format!("vendor/{alias}/{file}");
            ensure(
                p["files"].get(&path).is_none(),
                "conflict",
                "Vendored file would overwrite app source",
            )?;
            p["files"][&path] = contents.clone();
            hashes.insert(path, json!(store::hash(contents)));
        }
        p["libraries"][alias] = json!({"name":library["name"],"version":library["version"],"sha256":store::hash(&library),"files":hashes});
    }
    catalog::validate(&p)?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    file.write_all(serde_json::to_string_pretty(&p)?.as_bytes())?;
    Ok(
        json!({"app":p["name"],"package":out,"libraries":p["libraries"],"sha256":store::hash(&p),"installed":false,"published":false}),
    )
}
pub fn validate_libraries(p: &Value) -> Result<()> {
    let Some(libraries) = p.get("libraries") else {
        return Ok(());
    };
    ensure(
        crate::script::enabled(p),
        "pallet",
        "Embedded source libraries require a script app",
    )?;
    let libraries = libraries
        .as_object()
        .ok_or_else(|| Error::new("pallet", "libraries must be an object"))?;
    ensure(
        libraries.len() <= 16,
        "pallet",
        "At most 16 source libraries",
    )?;
    for (alias, library) in libraries {
        catalog::keys(library, &["name", "version", "sha256", "files"])?;
        ensure(
            identifier(alias) && catalog::app_name(text(library, "name")?),
            "pallet",
            "Invalid source library identity",
        )?;
        catalog::version(text(library, "version")?)?;
        ensure(
            text(library, "sha256")?.len() == 64
                && text(library, "sha256")?
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit()),
            "pallet",
            "Expected a pinned pallet hash",
        )?;
        let files = library["files"]
            .as_object()
            .ok_or_else(|| Error::new("pallet", "Library file hashes required"))?;
        ensure(
            !files.is_empty() && files.len() <= 128,
            "pallet",
            "Library must name 1..128 files",
        )?;
        for (path, hash) in files {
            ensure(
                safe_path(path)
                    && path.starts_with(&format!("vendor/{alias}/"))
                    && p["files"][path].is_string()
                    && store::hash(&p["files"][path]) == *hash,
                "integrity",
                "Vendored library file differs from its recorded hash",
            )?;
        }
    }
    Ok(())
}

pub fn init(name: &str, language: &str, out: &Path) -> Result<Value> {
    ensure(catalog::app_name(name), "pallet", "Use publisher/library")?;
    let (file,source)=match language{
        "python"=>("capabilities.py","def transform(args):\n    return {\"value\": args[\"value\"]}\n"),
        "javascript"=>("capabilities.mjs","export function transform(args) { return {value: args.value}; }\n"),
        _=>return Err(Error::new("pallet","Scaffolding supports python or javascript; other source languages can supply pallet.json directly"))
    };
    let contract = json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false});
    let p = json!({"format":"rhyven.pallet/1","name":name,"version":"0.1.0","description":"Reusable source library","language":language,"files":{file:source},"exports":{"transform":{"kind":"brick","description":"Return a supplied value","file":file,"symbol":"transform","input":contract,"output":contract}},"tests":[{"export":"transform","args":{"value":"sample"},"expect":{"value":"sample"}}],"dependencies":[]});
    export(&p, out)
}
