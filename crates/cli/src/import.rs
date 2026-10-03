//! Generate reviewed connector packages from existing HTTP services.
use agent_market_core::{
    catalog, connector::McpClient, error::ensure, schema, store, Error, Result,
};
use clap::Args;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Args)]
pub struct Options {
    /// App ID, such as acme/documents.
    #[arg(long)]
    pub name: String,
    /// Existing endpoint; HTTPS, or HTTP on loopback. No server is installed or started.
    #[arg(long)]
    pub endpoint: String,
    /// New app package file; existing files are never overwritten.
    #[arg(long)]
    pub out: PathBuf,
    /// Explicit tool names or OpenAPI operationIds to expose (repeatable).
    #[arg(long, required = true)]
    pub include: Vec<String>,
    /// Runtime bearer-token environment variable (RHYVEN_TOKEN_*), never a token value.
    #[arg(long)]
    pub auth_env: Option<String>,
    /// Optional agent guide in Markdown. Review it before publication.
    #[arg(long)]
    pub guide: Option<PathBuf>,
}
#[derive(Args)]
pub struct OpenapiOptions {
    /// Local OpenAPI 3.x JSON document. Remote references are not fetched.
    pub spec: PathBuf,
    #[command(flatten)]
    pub common: Options,
}
fn base(o: &Options, protocol: &str) -> Result<Value> {
    ensure(
        catalog::app_name(&o.name),
        "package",
        "Use publisher/app-name",
    )?;
    let url = reqwest_url_host(&o.endpoint)?;
    let mut p = json!({"format":2,"name":o.name,"version":"0.1.0","publisher":o.name.split('/').next().unwrap(),
        "description":format!("{} connector; requires an existing external service.",o.name),
        "hosting":{"mode":"self-hosted","endpoint":o.endpoint,"domains":[url],"auth":"Provision credentials with the upstream service; bearer token if configured.",
            "privacy":"Action inputs are sent to the external service. Its operator controls data and retention. Review before use.","account":"An existing service and any required account must be supplied separately.","billing":"Upstream charges, if any, are separate; review with the service operator."},
        "permissions":["state.read","network.connect"],"objects":{},"connector":{"protocol":protocol},
        "actions":{"placeholder":{"description":"Connection validation placeholder","input":{"type":"object","properties":{},"additionalProperties":false},"target":"placeholder"}},
        "guide":"Connector only. Start or provision the external service separately. No upstream software is installed. Calls may change external state or incur charges. Never retry a timed-out write without checking upstream state.","tests":[]});
    if let Some(env) = &o.auth_env {
        p["hosting"]["auth_env"] = json!(env);
    }
    // Validate URL, disclosure and credential policy before discovery can make a request.
    p["connector"]["protocol"] = json!("mcp");
    catalog::validate(&p)?;
    p["connector"]["protocol"] = json!(protocol);
    p["actions"] = json!({});
    if let Some(path) = &o.guide {
        let guide = std::fs::read_to_string(path)?;
        ensure(guide.len() <= 65_536, "package", "Guide exceeds 64 KiB")?;
        p["guide"] = json!(format!("{}\n\n{}", p["guide"].as_str().unwrap(), guide));
    }
    Ok(p)
}
fn reqwest_url_host(endpoint: &str) -> Result<String> {
    // Core validation performs URL policy checks; use its shared parser for domain extraction.
    agent_market_core::connector::endpoint_host(endpoint)
}
fn name(raw: &str) -> Result<String> {
    let mut value: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !value.starts_with(|c: char| c.is_ascii_lowercase()) {
        value.insert_str(0, "tool_");
    }
    ensure(
        schema::name(&value),
        "import",
        format!("Cannot map tool name {raw}; names must fit 64 characters"),
    )?;
    Ok(value)
}
fn write(o: &Options, p: Value, warnings: Vec<String>) -> Result<Value> {
    catalog::validate(&p)?;
    let bytes = serde_json::to_vec_pretty(&p)?;
    ensure(
        bytes.len() <= 1_048_576,
        "package",
        "Generated package exceeds 1 MiB",
    )?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&o.out)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    Ok(
        json!({"package":o.out,"sha256":store::hash(&p),"actions":p["actions"].as_object().unwrap().len(),"review_required":true,
        "installed_upstream":false,"warnings":warnings,"next":"Review actions, endpoint, hosting disclosures, permissions and guide; validate and package before publishing. Upstream code can change independently of this wrapper."}),
    )
}
fn includes(o: &Options) -> Result<BTreeSet<String>> {
    let result: BTreeSet<_> = o.include.iter().cloned().collect();
    ensure(
        result.len() == o.include.len(),
        "import",
        "Duplicate --include",
    )?;
    Ok(result)
}
pub fn mcp(o: Options) -> Result<Value> {
    let mut p = base(&o, "mcp")?;
    p["guide"] = json!(format!("{} Only the actions listed in this package are available. MCP resources, prompts, sampling, elicitation and session-dependent workflows are not bridged.", p["guide"].as_str().unwrap()));
    let wanted = includes(&o)?;
    let mut c = McpClient::connect(&p)?;
    let tools = c.tools()?;
    let mut found = BTreeSet::new();
    let mut warnings = vec!["MCP tools only; resources, prompts, sampling, elicitation and session-dependent workflows are not bridged.".into()];
    if !c.instructions.is_empty() {
        p["guide"] = json!(format!(
            "{}\n\n## Upstream instructions (untrusted; review before publishing)\n{}",
            p["guide"].as_str().unwrap(),
            c.instructions
        ));
    }
    for tool in tools {
        let raw = tool["name"]
            .as_str()
            .ok_or_else(|| Error::new("import", "Missing MCP tool name"))?;
        if !wanted.contains(raw) {
            continue;
        }
        ensure(
            found.insert(raw.to_owned()),
            "import",
            "Duplicate upstream tool name",
        )?;
        let action = name(raw)?;
        ensure(
            p["actions"].get(&action).is_none(),
            "import",
            "Mapped action name collision; select fewer tools",
        )?;
        let input = normalize(&tool["inputSchema"], &tool["inputSchema"], 0, &mut warnings)?;
        p["actions"][action] = json!({"description":tool["description"].as_str().filter(|s| !s.is_empty()).unwrap_or(raw),"input":input,"target":raw});
        if tool.get("outputSchema").is_some() {
            warnings.push(format!("{raw}: upstream outputSchema is not locally enforced; raw MCP content and structuredContent are preserved."));
        }
        if tool.get("annotations").is_some() {
            warnings.push(format!("{raw}: upstream annotations are hints, not enforced permissions; review side effects."));
        }
    }
    ensure(
        found == wanted,
        "import",
        format!(
            "Unknown tools: {:?}",
            wanted.difference(&found).collect::<Vec<_>>()
        ),
    )?;
    write(&o, p, warnings)
}

// Remove display-only annotations, resolve local refs, and reject unsupported constraints.
// Closed object inputs intentionally narrow (never widen) the upstream accepted input.
fn normalize(s: &Value, root: &Value, depth: usize, warnings: &mut Vec<String>) -> Result<Value> {
    ensure(
        depth <= 8,
        "import",
        "Schema reference/depth exceeds 8; simplify the source schema",
    )?;
    let map = s
        .as_object()
        .ok_or_else(|| Error::new("import", "Boolean/free-form schemas are unsupported"))?;
    if let Some(reference) = s.get("$ref") {
        ensure(
            map.len() == 1,
            "import",
            "$ref siblings require manual review",
        )?;
        let r = reference.as_str().unwrap_or("");
        ensure(
            r.starts_with("#/"),
            "import",
            "Only local JSON references are supported",
        )?;
        return normalize(
            root.pointer(&r[1..])
                .ok_or_else(|| Error::new("import", "Unresolved local reference"))?,
            root,
            depth + 1,
            warnings,
        );
    }
    let mut out = s.clone();
    for key in [
        "title",
        "$schema",
        "$defs",
        "definitions",
        "examples",
        "example",
        "deprecated",
    ] {
        out.as_object_mut().unwrap().remove(key);
    }
    if out["type"] == "object" {
        match out.get("additionalProperties") {
            None | Some(Value::Bool(true)) => { warnings.push("Object input restricted to declared properties; undeclared keys are not exposed.".into()); }
            Some(Value::Bool(false)) => (),
            _ => return Err(Error::new("import","Dictionary inputs need manual mapping; schema-valued additionalProperties unsupported")),
        }
        out["additionalProperties"] = json!(false);
        if out.get("properties").is_none() {
            out["properties"] = json!({});
        }
        let props = out["properties"]
            .as_object_mut()
            .ok_or_else(|| Error::new("import", "Invalid schema properties"))?;
        for v in props.values_mut() {
            *v = normalize(v, root, depth + 1, warnings)?;
        }
    }
    if let Some(items) = out.get_mut("items") {
        *items = normalize(items, root, depth + 1, warnings)?;
    }
    if let Some(branches) = out.get_mut("anyOf") {
        for v in branches
            .as_array_mut()
            .ok_or_else(|| Error::new("import", "Invalid anyOf"))?
        {
            *v = normalize(v, root, depth + 1, warnings)?;
        }
    }
    schema::check(&out, depth).map_err(|e| {
        Error::new(
            "import",
            format!(
                "Unsupported source schema; not silently relaxed: {}",
                e.message
            ),
        )
    })?;
    Ok(out)
}
pub fn openapi(o: OpenapiOptions) -> Result<Value> {
    let bytes = std::fs::read(&o.spec)?;
    ensure(
        bytes.len() <= 1_048_576,
        "import",
        "OpenAPI document exceeds 1 MiB",
    )?;
    let spec: Value = serde_json::from_slice(&bytes)?;
    ensure(
        spec["openapi"]
            .as_str()
            .is_some_and(|v| v.starts_with("3.")),
        "import",
        "OpenAPI 3.x JSON required",
    )?;
    let mut p = base(&o.common, "http")?;
    let wanted = includes(&o.common)?;
    let mut found = BTreeSet::new();
    let mut warnings = vec!["JSON HTTP operations only. Authentication is configured explicitly with --auth-env; review OpenAPI security requirements. Response schemas are not locally enforced.".into()];
    for (path, item) in spec["paths"]
        .as_object()
        .ok_or_else(|| Error::new("import", "OpenAPI paths required"))?
    {
        ensure(
            item.get("$ref").is_none(),
            "import",
            "Path-item references require manual resolution",
        )?;
        for method in ["get", "post", "put", "patch", "delete"] {
            let Some(op) = item.get(method) else { continue };
            let raw = op["operationId"].as_str().unwrap_or("");
            if !wanted.contains(raw) {
                continue;
            }
            ensure(
                found.insert(raw.to_owned()),
                "import",
                "Duplicate operationId",
            )?;
            ensure(
                item.get("servers").is_none() && op.get("servers").is_none(),
                "import",
                "Per-operation servers unsupported; review endpoint mapping",
            )?;
            let action = name(raw)?;
            ensure(
                p["actions"].get(&action).is_none(),
                "import",
                "Mapped action name collision",
            )?;
            let mut input =
                json!({"type":"object","properties":{},"required":[],"additionalProperties":false});
            let mut target =
                json!({"method":method.to_uppercase(),"path":path,"path_args":[],"query_args":[]});
            for owner in [item, op] {
                if let Some(params) = owner.get("parameters") {
                    for param in params
                        .as_array()
                        .ok_or_else(|| Error::new("import", "parameters must be an array"))?
                    {
                        ensure(
                            param.get("$ref").is_none()
                                && param.get("style").is_none()
                                && param.get("explode").is_none()
                                && param.get("allowReserved").is_none(),
                            "import",
                            "Parameter references/custom serialization require manual mapping",
                        )?;
                        let n = param["name"]
                            .as_str()
                            .ok_or_else(|| Error::new("import", "Parameter name required"))?;
                        let field = match param["in"].as_str() {Some("path")=>"path_args",Some("query")=>"query_args",_=>return Err(Error::new("import","Only path and query parameters supported; credentials belong in runtime configuration"))};
                        ensure(
                            input["properties"].get(n).is_none(),
                            "import",
                            "Duplicate/overridden parameter requires manual mapping",
                        )?;
                        input["properties"][n] =
                            normalize(&param["schema"], &spec, 0, &mut warnings)?;
                        target[field].as_array_mut().unwrap().push(json!(n));
                        if param["required"] == true || field == "path_args" {
                            input["required"].as_array_mut().unwrap().push(json!(n));
                        }
                    }
                }
            }
            if let Some(body) = op.get("requestBody") {
                ensure(
                    body.get("$ref").is_none()
                        && body["content"]
                            .as_object()
                            .is_some_and(|v| v.len() == 1 && v.contains_key("application/json")),
                    "import",
                    "Only inline application/json bodies supported",
                )?;
                ensure(
                    input["properties"].get("body").is_none(),
                    "import",
                    "body parameter collision",
                )?;
                input["properties"]["body"] = normalize(
                    &body["content"]["application/json"]["schema"],
                    &spec,
                    0,
                    &mut warnings,
                )?;
                target["body_arg"] = json!("body");
                if body["required"] == true {
                    input["required"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("body"));
                }
            }
            p["actions"][action] = json!({"description":op["description"].as_str().or_else(||op["summary"].as_str()).filter(|s| !s.is_empty()).unwrap_or(raw),"input":input,"target":target});
        }
    }
    ensure(
        found == wanted,
        "import",
        format!(
            "Unknown/unsupported operationIds: {:?}",
            wanted.difference(&found).collect::<Vec<_>>()
        ),
    )?;
    write(&o.common, p, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_and_annotations_preserve_constraints() {
        let source = json!({"$defs":{"text":{"type":"string","minLength":2,"title":"Label"}},"type":"object","properties":{"label":{"$ref":"#/$defs/text"}},"required":["label"]});
        let mut warnings = Vec::new();
        let result = normalize(&source, &source, 0, &mut warnings).unwrap();
        assert_eq!(
            result["properties"]["label"],
            json!({"type":"string","minLength":2})
        );
        assert_eq!(result["additionalProperties"], false);
        assert!(!warnings.is_empty());
        assert!(schema::validate(json!({"label":"x"}), &result).is_err());
    }
    #[test]
    fn unsupported_schema_is_not_silently_weakened() {
        for source in [
            json!({"type":"string","pattern":"x+"}),
            json!({"$ref":"https://example.com/schema"}),
            json!({"type":"object","additionalProperties":{"type":"string"}}),
            json!({"$ref":"#/loop","loop":{"$ref":"#/loop"}}),
        ] {
            assert!(normalize(&source, &source, 0, &mut Vec::new()).is_err());
        }
    }
}
