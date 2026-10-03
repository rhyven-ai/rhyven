//! Reviewed HTTP connectors. Packages describe calls; they never install or start upstream services.
use crate::{catalog, error::ensure, schema, Error, Result};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

const MAX: usize = 1_048_576;
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

pub fn enabled(p: &Value) -> bool {
    p.get("connector").is_some()
}

pub fn validate(p: &Value) -> Result<()> {
    if !enabled(p) {
        return Ok(());
    }
    catalog::keys(&p["connector"], &["protocol"])?;
    let protocol = p["connector"]["protocol"].as_str().unwrap_or("");
    ensure(
        ["mcp", "http"].contains(&protocol),
        "package",
        "Connector protocol must be mcp or http",
    )?;
    ensure(
        p["hosting"]["mode"] != "local" && p.get("execution").is_none(),
        "package",
        "Connectors require a disclosed external endpoint, not local execution",
    )?;
    ensure(
        p["objects"].as_object().is_some_and(|v| v.is_empty()),
        "package",
        "Connectors expose actions only",
    )?;
    let actions = p["actions"]
        .as_object()
        .ok_or_else(|| Error::new("package", "actions required"))?;
    ensure(
        !actions.is_empty() && actions.len() <= 128,
        "package",
        "Provide 1..128 connector actions",
    )?;
    for action in actions.values() {
        catalog::keys(action, &["description", "keywords", "input", "target"])?;
        ensure(
            action["description"]
                .as_str()
                .is_some_and(|v| !v.trim().is_empty()),
            "package",
            "Action description required",
        )?;
        schema::check(&action["input"], 0)?;
        ensure(
            action["input"]["type"] == "object",
            "package",
            "Connector input must be an object",
        )?;
        if protocol == "mcp" {
            ensure(
                action["target"].as_str().is_some_and(|s| {
                    !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
                }),
                "package",
                "MCP target must be a tool name",
            )?;
        } else {
            let target = &action["target"];
            catalog::keys(
                target,
                &["method", "path", "path_args", "query_args", "body_arg"],
            )?;
            ensure(
                matches!(
                    target["method"].as_str(),
                    Some("GET" | "POST" | "PUT" | "PATCH" | "DELETE")
                ),
                "package",
                "Unsupported HTTP method",
            )?;
            let path = target["path"].as_str().unwrap_or("");
            ensure(
                path.starts_with('/')
                    && !path.contains(['?', '#', '\\', '%'])
                    && !path.contains("//")
                    && !path.split('/').any(|s| s == "." || s == "..")
                    && !path.chars().any(char::is_control),
                "package",
                "HTTP path must be a literal endpoint-relative path",
            )?;
            let mut remaining = path.to_owned();
            let mut seen = std::collections::BTreeSet::new();
            for field in ["path_args", "query_args"] {
                let names = target[field]
                    .as_array()
                    .ok_or_else(|| Error::new("package", "HTTP parameter lists required"))?;
                for name in names {
                    let name = name
                        .as_str()
                        .ok_or_else(|| Error::new("package", "Parameter name must be a string"))?;
                    let property = &action["input"]["properties"][name];
                    ensure(
                        seen.insert(name)
                            && matches!(
                                property["type"].as_str(),
                                Some("string" | "integer" | "number" | "boolean")
                            ),
                        "package",
                        "HTTP parameters must be unique scalar input properties",
                    )?;
                    if field == "path_args" {
                        let placeholder = format!("{{{name}}}");
                        ensure(
                            remaining.contains(&placeholder)
                                && action["input"]["required"]
                                    .as_array()
                                    .is_some_and(|a| a.contains(&json!(name))),
                            "package",
                            "Path arguments must be required and present in path",
                        )?;
                        remaining = remaining.replace(&placeholder, "value");
                    }
                }
            }
            ensure(
                !remaining.contains(['{', '}']),
                "package",
                "Undeclared path placeholder",
            )?;
            if let Some(body) = target.get("body_arg") {
                let name = body.as_str().unwrap_or("");
                ensure(
                    seen.insert(name)
                        && action["input"]["properties"].get(name).is_some()
                        && target["method"] != "GET",
                    "package",
                    "Invalid HTTP body mapping",
                )?;
            }
            ensure(
                seen.len() == action["input"]["properties"].as_object().unwrap().len(),
                "package",
                "Every HTTP input must have a mapping",
            )?;
        }
    }
    Ok(())
}

fn client() -> Result<Client> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| {
            Error::new(
                "connector_unavailable",
                "Cannot initialize connector client",
            )
        })
}
fn authenticated(request: RequestBuilder, p: &Value) -> Result<RequestBuilder> {
    if let Some(env) = p["hosting"]["auth_env"].as_str() {
        let token = std::env::var(env).map_err(|_| {
            Error::new(
                "auth_required",
                format!("Set {env} in the runtime environment"),
            )
        })?;
        ensure(
            !token.is_empty(),
            "auth_required",
            format!("Set {env} in the runtime environment"),
        )?;
        Ok(request.bearer_auth(token))
    } else {
        Ok(request)
    }
}
fn send(request: RequestBuilder) -> Result<Response> {
    let response = request.send().map_err(|e| Error::new(if e.is_timeout() {"connector_timeout"} else {"connector_unavailable"}, "Endpoint request failed; verify availability and credentials. An action may have completed; do not retry writes blindly."))?;
    ensure(
        response.status().is_success(),
        "connector_unavailable",
        format!(
            "Endpoint returned HTTP {}; no automatic retry",
            response.status().as_u16()
        ),
    )?;
    Ok(response)
}
fn bounded_json(response: Response) -> Result<Value> {
    let mut bytes = Vec::new();
    response.take((MAX + 1) as u64).read_to_end(&mut bytes)?;
    ensure(
        bytes.len() <= MAX,
        "connector_protocol",
        "Response exceeds 1 MiB",
    )?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("connector_protocol", "Expected JSON response"))
}

/// One short-lived session per operation. No retries, upstream process launch, or server-initiated calls.
pub struct McpClient {
    client: Client,
    package: Value,
    session: Option<String>,
    version: String,
    next_id: u64,
    pub instructions: String,
}
impl McpClient {
    pub fn connect(package: &Value) -> Result<Self> {
        // Callers must validate a complete package (or importer connection template) first.
        let mut c = Self {
            client: client()?,
            package: package.clone(),
            session: None,
            version: VERSIONS[0].into(),
            next_id: 1,
            instructions: String::new(),
        };
        let init = c.request("initialize", json!({"protocolVersion":VERSIONS[0],"capabilities":{},"clientInfo":{"name":"rhyven-connector","version":env!("CARGO_PKG_VERSION")}}))?;
        let version = init["protocolVersion"].as_str().unwrap_or("");
        ensure(
            VERSIONS.contains(&version),
            "connector_protocol",
            "Upstream must support MCP 2025-03-26, 2025-06-18 or 2025-11-25",
        )?;
        c.version = version.into();
        c.instructions = init["instructions"].as_str().unwrap_or("").to_owned();
        let request = c
            .post()
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        send(authenticated(request, &c.package)?)?;
        Ok(c)
    }
    fn post(&self) -> RequestBuilder {
        let mut r = self
            .client
            .post(self.package["hosting"]["endpoint"].as_str().unwrap())
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", &self.version);
        if let Some(session) = &self.session {
            r = r.header("Mcp-Session-Id", session);
        }
        r
    }
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let response = send(authenticated(
            self.post()
                .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})),
            &self.package,
        )?)?;
        if method == "initialize" {
            self.session = response
                .headers()
                .get("mcp-session-id")
                .map(|v| v.to_str().map(str::to_owned))
                .transpose()
                .map_err(|_| Error::new("connector_protocol", "Invalid MCP session header"))?;
        }
        let sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        let reply = if sse {
            read_sse(response, id)?
        } else {
            bounded_json(response)?
        };
        ensure(
            reply["jsonrpc"] == "2.0"
                && reply["id"] == id
                && reply.get("result").is_some() != reply.get("error").is_some(),
            "connector_protocol",
            "Invalid MCP response envelope",
        )?;
        ensure(
            reply.get("error").is_none(),
            "connector_app",
            "Upstream MCP returned an error; inspect upstream logs",
        )?;
        Ok(reply["result"].clone())
    }
    pub fn tools(&mut self) -> Result<Vec<Value>> {
        let mut tools = Vec::new();
        let mut cursor = Value::Null;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..16 {
            let page = self.request(
                "tools/list",
                if cursor.is_null() {
                    json!({})
                } else {
                    json!({"cursor":cursor})
                },
            )?;
            tools.extend(
                page["tools"]
                    .as_array()
                    .ok_or_else(|| Error::new("connector_protocol", "MCP tools array required"))?
                    .iter()
                    .cloned(),
            );
            ensure(
                tools.len() <= 128,
                "connector_protocol",
                "Import supports at most 128 tools",
            )?;
            cursor = page["nextCursor"].clone();
            if cursor.is_null() {
                return Ok(tools);
            }
            ensure(
                cursor.is_string() && seen.insert(cursor.as_str().unwrap().to_owned()),
                "connector_protocol",
                "Invalid or repeated tools cursor",
            )?;
        }
        Err(Error::new("connector_protocol", "Too many tool pages"))
    }
}
impl Drop for McpClient {
    fn drop(&mut self) {
        if let Some(session) = &self.session {
            let r = self
                .client
                .delete(self.package["hosting"]["endpoint"].as_str().unwrap())
                .header("MCP-Protocol-Version", &self.version)
                .header("Mcp-Session-Id", session)
                .timeout(Duration::from_secs(2));
            if let Ok(r) = authenticated(r, &self.package) {
                let _ = r.send();
            }
        }
    }
}
fn read_sse(response: Response, id: u64) -> Result<Value> {
    let mut reader = BufReader::new(response.take((MAX + 1) as u64));
    let mut total = 0;
    let mut data = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        total += n;
        ensure(
            total <= MAX,
            "connector_protocol",
            "MCP stream exceeds 1 MiB",
        )?;
        if n == 0 || line.trim_end().is_empty() {
            if !data.trim().is_empty() {
                let v: Value = serde_json::from_str(&data)
                    .map_err(|_| Error::new("connector_protocol", "Invalid MCP event JSON"))?;
                ensure(
                    v.get("method").is_none() || v.get("id").is_none(),
                    "connector_protocol",
                    "Server-initiated MCP requests are unsupported",
                )?;
                if v["id"] == id && v.get("method").is_none() {
                    return Ok(v);
                }
                data.clear();
            }
            ensure(
                n != 0,
                "connector_protocol",
                "MCP stream ended without response",
            )?;
        } else if let Some(part) = line.strip_prefix("data:") {
            data.push_str(
                part.strip_prefix(' ')
                    .unwrap_or(part)
                    .trim_end_matches(['\r', '\n']),
            );
            data.push('\n');
        }
    }
}

pub fn call(p: &Value, args: &Value) -> Result<Value> {
    ensure(
        args.get("request_id").is_none(),
        "validation",
        "Connectors do not provide request_id deduplication; use upstream idempotency arguments",
    )?;
    let name = args["action"].as_str().unwrap_or("");
    let action = p["actions"]
        .get(name)
        .ok_or_else(|| Error::new("not_found", "Unknown connector action"))?;
    let input = schema::validate(
        args.get("args").cloned().unwrap_or(json!({})),
        &action["input"],
    )?;
    if p["connector"]["protocol"] == "mcp" {
        let mut c = McpClient::connect(p)?;
        let result = c.request(
            "tools/call",
            json!({"name":action["target"],"arguments":input}),
        )?;
        ensure(
            result["content"].is_array() && result.get("isError").is_none_or(Value::is_boolean),
            "connector_protocol",
            "Invalid MCP tool result",
        )?;
        ensure(
            result["isError"] != true,
            "connector_app",
            format!("Upstream tool reported failure: {}", result),
        )?;
        return Ok(result);
    }
    let target = &action["target"];
    let mut url = reqwest::Url::parse(p["hosting"]["endpoint"].as_str().unwrap()).unwrap();
    let mut path = target["path"].as_str().unwrap().to_owned();
    for name in target["path_args"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        let raw = scalar(&input[name]);
        ensure(
            raw != "."
                && raw != ".."
                && !raw.contains(['/', '\\', '%', '?', '#', '{', '}'])
                && !raw.chars().any(char::is_control)
                && !raw.is_empty(),
            "validation",
            "Path arguments must be single segments without traversal or encoded delimiters",
        )?;
        path = path.replace(&format!("{{{name}}}"), &raw);
    }
    let base = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{base}{path}"));
    for name in target["query_args"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        if let Some(value) = input.get(name) {
            url.query_pairs_mut().append_pair(name, &scalar(value));
        }
    }
    let method = target["method"].as_str().unwrap().parse().unwrap();
    let mut r = client()?
        .request(method, url)
        .header("Accept", "application/json");
    if let Some(name) = target["body_arg"].as_str() {
        if let Some(body) = input.get(name) {
            r = r.json(body);
        }
    }
    let response = send(authenticated(r, p)?)?;
    let status = response.status().as_u16();
    Ok(json!({"status":status,"body":bounded_json(response)?}))
}
fn scalar(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}

pub fn endpoint_host(endpoint: &str) -> Result<String> {
    reqwest::Url::parse(endpoint)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .ok_or_else(|| Error::new("package", "Valid endpoint URL required"))
}
