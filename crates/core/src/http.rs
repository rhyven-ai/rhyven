//! Authenticated REST server and client for shared runtime access.
use crate::{error::ensure, Error, Result, Runtime};
use reqwest::{blocking::Client, Url};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MAX_HEADER: usize = 32 * 1024;
const MAX_BODY: usize = 1_048_576;
const MAX_CONNECTIONS: usize = 32;

#[derive(Clone)]
pub struct HttpClient {
    base: Url,
    token: String,
    client: Client,
    management_token: Option<String>,
}

impl HttpClient {
    pub fn new(endpoint: &str, token: String) -> Result<Self> {
        let mut base =
            Url::parse(endpoint).map_err(|_| Error::new("network", "Invalid shared server URL"))?;
        ensure(
            matches!(base.scheme(), "https" | "http")
                && base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none()
                && !token.is_empty(),
            "network",
            "Shared server URL must be HTTP(S) without embedded credentials and token is required",
        )?;
        if base.path() != "/" {
            base.set_path("/");
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| Error::new("network", "Could not initialize HTTP client"))?;
        Ok(Self {
            base,
            token,
            client,
            management_token: std::env::var("RHYVEN_MANAGEMENT_TOKEN").ok(),
        })
    }
    pub fn approve(&self, id: &str, digest: &str, accept: bool) -> Result<Value> {
        ensure(
            self.management_token.is_some(),
            "permission",
            "Shared approval requires RHYVEN_MANAGEMENT_TOKEN in the trusted host",
        )?;
        self.send(
            "POST",
            &format!("approvals/{}", segment(id)?),
            Some(json!({"digest":digest,"accept":accept})),
        )
    }
    pub fn apps(&self) -> Result<Value> {
        self.get("apps")
    }
    pub fn describe(&self, app: &str) -> Result<Value> {
        let (publisher, name) = app_parts(app)?;
        self.get(&format!("apps/{publisher}/{name}"))
    }
    pub fn call(&self, operation: &str, args: Value) -> Result<Value> {
        match operation {
            "rhyven_categories" => self.get("categories"),
            "rhyven_describe" => {
                let (p, n) = app_parts(string(&args, "category")?)?;
                self.get(&format!("categories/{p}/{n}"))
            }
            "rhyven_call" => {
                let (p, n) = app_parts(string(&args, "category")?)?;
                self.send(
                    "POST",
                    &format!(
                        "categories/{p}/{n}/functions/{}",
                        segment(string(&args, "function")?)?
                    ),
                    Some(args["args"].clone()),
                )
            }
            "list_apps" => self.apps(),
            "describe_app" => self.describe(string(&args, "app")?),
            "query" => {
                let (p, n) = app_parts(string(&args, "app")?)?;
                self.send("POST", &format!("apps/{p}/{n}/query"), Some(args))
            }
            "get" => {
                let (p, n) = app_parts(string(&args, "app")?)?;
                self.get(&format!(
                    "apps/{p}/{n}/objects/{}/{}",
                    string(&args, "object")?,
                    encode(string(&args, "id")?)
                ))
            }
            "create" => {
                let (p, n) = app_parts(string(&args, "app")?)?;
                self.send(
                    "POST",
                    &format!("apps/{p}/{n}/objects/{}", string(&args, "object")?),
                    Some(args),
                )
            }
            "update" => {
                let (p, n) = app_parts(string(&args, "app")?)?;
                self.send(
                    "PATCH",
                    &format!(
                        "apps/{p}/{n}/objects/{}/{}",
                        string(&args, "object")?,
                        encode(string(&args, "id")?)
                    ),
                    Some(args),
                )
            }
            "execute" => {
                let (p, n) = app_parts(string(&args, "app")?)?;
                self.send(
                    "POST",
                    &format!("apps/{p}/{n}/actions/{}", string(&args, "action")?),
                    Some(args),
                )
            }
            _ => Err(Error::new("unknown_operation", operation)),
        }
    }
    fn get(&self, path: &str) -> Result<Value> {
        self.send("GET", path, None)
    }
    fn send(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let url = self
            .base
            .join(path)
            .map_err(|_| Error::new("network", "Invalid API route"))?;
        let request = match method {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PATCH" => self.client.patch(url),
            _ => unreachable!(),
        }
        .bearer_auth(&self.token)
        .header("X-Rhyven-Actor", "mcp");
        let request = if let Some(token) = &self.management_token {
            request.header("X-Rhyven-Management", token)
        } else {
            request
        };
        let response = if let Some(body) = body {
            request.json(&body).send()
        } else {
            request.send()
        }
        .map_err(|e| Error::new("network", format!("Shared server request failed: {e}")))?;
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .take((MAX_BODY + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::new("network", "Shared server response failed"))?;
        ensure(
            bytes.len() <= MAX_BODY,
            "network",
            "Shared server response exceeds 1 MiB",
        )?;
        let value: Value = serde_json::from_slice(&bytes)?;
        if !status.is_success() {
            let error: Error = serde_json::from_value(value).unwrap_or_else(|_| {
                Error::new(
                    "network",
                    format!("Shared server returned HTTP {}", status.as_u16()),
                )
            });
            return Err(error);
        }
        Ok(value)
    }
}

pub fn serve(
    runtime: Runtime,
    host: &str,
    port: u16,
    token: String,
    allow_insecure_network: bool,
) -> Result<()> {
    ensure(
        !token.is_empty(),
        "authentication",
        "Shared server token is required",
    )?;
    let management = std::env::var("RHYVEN_MANAGEMENT_TOKEN").ok();
    if let Some(value) = &management {
        ensure(
            value.len() >= 16 && value.len() <= 512 && *value != token,
            "authentication",
            "Management token must be distinct and 16-512 characters",
        )?;
    }
    let address = format!("{host}:{port}");
    let listener = TcpListener::bind(&address)?;
    let local = listener.local_addr()?;
    if !local.ip().is_loopback() {
        ensure(allow_insecure_network, "network", "Non-loopback HTTP needs --allow-insecure-network and should sit behind a TLS reverse proxy")?;
    }
    eprintln!(
        "{}",
        json!({"status":"listening","address":local.to_string(),"connection":connection_info(&runtime)?})
    );
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let runtime = runtime.clone();
        let token = token.clone();
        let management = management.clone();
        match stream {
            Ok(stream) => {
                if active
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                        (n < MAX_CONNECTIONS).then_some(n + 1)
                    })
                    .is_err()
                {
                    // Never block the accept loop writing to an unresponsive peer.
                    drop(stream);
                    continue;
                }
                let permit = ConnectionPermit(active.clone());
                std::thread::spawn(move || {
                    let _permit = permit;
                    let _ = handle(stream, runtime, &token, management.as_deref());
                });
            }
            Err(e) => return Err(Error::new("network", e.to_string())),
        }
    }
    Ok(())
}

struct ConnectionPermit(Arc<AtomicUsize>);
impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A wall-clock request deadline, including clients that drip-feed bytes.
struct RequestReader<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}
impl Read for RequestReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "Request deadline exceeded")
            })?;
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(bytes)
    }
}

fn bounded_line(reader: &mut impl BufRead, remaining: usize) -> Result<String> {
    let mut line = Vec::new();
    reader
        .take((remaining + 1) as u64)
        .read_until(b'\n', &mut line)?;
    ensure(
        line.len() <= remaining && line.ends_with(b"\n"),
        "http",
        "Incomplete or oversized HTTP header",
    )?;
    String::from_utf8(line).map_err(|_| Error::new("http", "Invalid header encoding"))
}

fn connection_info(runtime: &Runtime) -> Result<Value> {
    Ok(json!({
        "version":env!("CARGO_PKG_VERSION"),
        "collection":crate::collections::scope(&runtime.root)?["collection"],
        "transport":"REST", "native_http_mcp":false,
        "authentication":"Authorization: Bearer <server token>",
        "mcp_bridge":{"command":"rhyven","args":["mcp","--server","http(s)://SERVER:PORT"],"token_environment":"RHYVEN_SERVE_TOKEN"},
        "setup":"rhyven connect --client CLIENT --server URL --expect-collection COLLECTION",
        "routes":{"discover":"GET /categories","describe":"GET /categories/{publisher}/{app}","call":"POST /categories/{publisher}/{app}/functions/{function}"},
        "instructions":["Use this server's collection for all requests; local collection flags cannot override it.","The endpoint is REST. Configure a local stdio MCP bridge for an MCP client, or call REST directly.","Provide the same bearer token in the client's environment. Never put tokens in app arguments.","Remote marketplace changes additionally require RHYVEN_MANAGEMENT_TOKEN and human approval."]
    }))
}

fn handle(
    mut stream: TcpStream,
    runtime: Runtime,
    token: &str,
    management: Option<&str>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let (method, path, headers, body) = match read_request(&mut stream) {
        Ok(v) => v,
        Err(e) => {
            write_response(&mut stream, 400, &e)?;
            return Ok(());
        }
    };
    if headers.get("authorization").map(String::as_str) != Some(&format!("Bearer {token}")) {
        write_response(
            &mut stream,
            401,
            &Error::new("authentication", "Bearer token required"),
        )?;
        return Ok(());
    }
    let actor = headers
        .get("x-rhyven-actor")
        .filter(|s| valid_actor(s))
        .cloned()
        .unwrap_or_else(|| "shared-client".into());
    if actor.starts_with(crate::services::PRINCIPAL) {
        write_response(
            &mut stream,
            403,
            &Error::new(
                "permission",
                "Service principals cannot be supplied by clients",
            ),
        )?;
        return Ok(());
    }
    let scoped = Runtime::new(&runtime.root, &actor)?;
    let privileged = management.is_some()
        && headers.get("x-rhyven-management").map(String::as_str) == management;
    let response = if path.starts_with("/approvals/") {
        if !privileged {
            Err(Error::new("permission", "Management credential required"))
        } else if method != "POST" {
            Err(Error::new("not_found", "Unknown route"))
        } else {
            crate::marketplace::decide(
                &scoped,
                path.trim_start_matches("/approvals/"),
                body["digest"].as_str().unwrap_or(""),
                body["accept"] == true,
            )
        }
    } else if (path.contains("/rhyven/runtime/")
        && ["service_start", "service_stop", "service_restart"]
            .iter()
            .any(|name| path.trim_end_matches('/').ends_with(name))
        && !privileged)
        || (path
            .trim_matches('/')
            .starts_with("apps/rhyven/marketplace/actions/")
            || path
                .trim_matches('/')
                .starts_with("categories/rhyven/marketplace/functions/action_"))
            && !privileged
    {
        Err(Error::new(
            "permission",
            "Marketplace management requires a distinct RHYVEN_MANAGEMENT_TOKEN",
        ))
    } else {
        route(&scoped, &method, &path, body)
    };
    match response {
        Ok(v) => write_response(&mut stream, 200, &v)?,
        Err(e) => write_response(&mut stream, status(&e), &e)?,
    }
    Ok(())
}

fn read_request(
    stream: &mut TcpStream,
) -> Result<(
    String,
    String,
    std::collections::BTreeMap<String, String>,
    Value,
)> {
    parse_request(BufReader::new(RequestReader {
        stream,
        deadline: Instant::now() + Duration::from_secs(30),
    }))
}

fn parse_request(
    mut reader: impl BufRead,
) -> Result<(
    String,
    String,
    std::collections::BTreeMap<String, String>,
    Value,
)> {
    let first = bounded_line(&mut reader, MAX_HEADER)?;
    let mut pieces = first.split_whitespace();
    let method = pieces.next().unwrap_or("").to_owned();
    let target = pieces.next().unwrap_or("");
    ensure(
        matches!(pieces.next(), Some("HTTP/1.1" | "HTTP/1.0"))
            && pieces.next().is_none()
            && matches!(method.as_str(), "GET" | "POST" | "PATCH")
            && target.starts_with('/')
            && !target.contains('?'),
        "http",
        "Invalid HTTP request",
    )?;
    let mut headers: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut total = first.len();
    loop {
        let line = bounded_line(&mut reader, MAX_HEADER - total)?;
        total += line.len();
        ensure(total <= MAX_HEADER, "http", "Request headers too large")?;
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (key, value) = line
            .trim_end()
            .split_once(':')
            .ok_or_else(|| Error::new("http", "Invalid header"))?;
        ensure(
            !key.is_empty()
                && key
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)),
            "http",
            "Invalid header name",
        )?;
        ensure(
            value
                .trim()
                .bytes()
                .all(|c| c == b'\t' || !c.is_ascii_control()),
            "http",
            "Invalid header value",
        )?;
        ensure(
            headers
                .insert(key.to_ascii_lowercase(), value.trim().into())
                .is_none(),
            "http",
            "Duplicate headers are not supported",
        )?;
    }
    ensure(
        !headers.contains_key("transfer-encoding"),
        "http",
        "Transfer encoding is not supported",
    )?;
    let length = headers
        .get("content-length")
        .map(|s| {
            s.parse::<usize>()
                .map_err(|_| Error::new("http", "Invalid content length"))
        })
        .transpose()?
        .unwrap_or(0);
    ensure(length <= MAX_BODY, "http", "Request body exceeds 1 MiB")?;
    if length > 0 {
        ensure(
            headers.get("content-type").is_some_and(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("application/json")
            }),
            "http",
            "Expected application/json",
        )?;
    }
    let mut raw = vec![0; length];
    reader.read_exact(&mut raw)?;
    let body = if length == 0 {
        json!({})
    } else {
        serde_json::from_slice(&raw)?
    };
    ensure(
        body.is_object(),
        "validation",
        "JSON request body must be an object",
    )?;
    Ok((method, target.into(), headers, body))
}

fn route(runtime: &Runtime, method: &str, path: &str, mut body: Value) -> Result<Value> {
    if method == "GET" && matches!(path.trim_matches('/'), "" | "connection") {
        return connection_info(runtime);
    }
    let parts: Vec<_> = path.trim_matches('/').split('/').collect();
    if method == "GET" && parts == ["categories"] {
        return runtime.call("rhyven_categories", json!({}));
    }
    if parts.first() == Some(&"categories") && parts.len() >= 3 {
        let category = format!("{}/{}", segment(parts[1])?, segment(parts[2])?);
        if method == "GET" && parts.len() == 3 {
            return runtime.call("rhyven_describe", json!({"category":category}));
        }
        if method == "POST" && parts.len() == 5 && parts[3] == "functions" {
            return runtime.call(
                "rhyven_call",
                json!({"category":category,"function":segment(parts[4])?,"args":body}),
            );
        }
        return Err(Error::new("not_found", "Unknown category route"));
    }
    if method == "GET" && parts == ["apps"] {
        return runtime.call("list_apps", json!({}));
    }
    ensure(
        parts.len() >= 3 && parts[0] == "apps",
        "not_found",
        "Unknown API route",
    )?;
    let app = format!("{}/{}", segment(parts[1])?, segment(parts[2])?);
    if method == "GET" && parts.len() == 3 {
        return runtime.call("describe_app", json!({"app":app}));
    }
    ensure(parts.len() >= 4, "not_found", "Unknown API route")?;
    match (method, parts[3]) {
        ("POST", "query") if parts.len() == 4 => {
            body["app"] = json!(app);
            runtime.call("query", body)
        }
        ("POST", "objects") if parts.len() == 5 => {
            body["app"] = json!(app);
            body["object"] = json!(segment(parts[4])?);
            runtime.call("create", body)
        }
        ("GET", "objects") if parts.len() == 6 => runtime.call(
            "get",
            json!({"app":app,"object":segment(parts[4])?,"id":decode(parts[5])?}),
        ),
        ("PATCH", "objects") if parts.len() == 6 => {
            body["app"] = json!(app);
            body["object"] = json!(segment(parts[4])?);
            body["id"] = json!(segment(parts[5])?);
            runtime.call("update", body)
        }
        ("POST", "actions") if parts.len() == 5 => {
            body["app"] = json!(app);
            body["action"] = json!(segment(parts[4])?);
            runtime.call("execute", body)
        }
        _ => Err(Error::new("not_found", "Unknown API route")),
    }
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure(i + 2 < bytes.len(), "validation", "Invalid URL encoding")?;
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| Error::new("validation", "Invalid URL encoding"))?;
            out.push(
                u8::from_str_radix(hex, 16)
                    .map_err(|_| Error::new("validation", "Invalid URL encoding"))?,
            );
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::new("validation", "Invalid URL encoding"))
}
fn segment(s: &str) -> Result<&str> {
    ensure(
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "validation",
        "Invalid URL segment",
    )?;
    Ok(s)
}
fn app_parts(app: &str) -> Result<(&str, &str)> {
    let (p, n) = app
        .split_once('/')
        .ok_or_else(|| Error::new("validation", "Expected publisher/app"))?;
    segment(p)?;
    segment(n)?;
    Ok((p, n))
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("validation", format!("Missing {key}")))
}
fn valid_actor(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}
fn status(error: &Error) -> u16 {
    match error.code.as_str() {
        "authentication" => 401,
        "not_found" => 404,
        "permission" => 403,
        "revision_conflict" | "idempotency_conflict" | "already_installed" => 409,
        "network" => 502,
        _ => 400,
    }
}
fn write_response(
    stream: &mut TcpStream,
    status: u16,
    value: &impl serde::Serialize,
) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    let text = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Bad Gateway",
    };
    write!(stream, "HTTP/1.1 {status} {text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn rejects_oversized_and_ambiguous_http_framing() {
        let mut oversized = std::io::Cursor::new(vec![b'x'; 2 * MAX_HEADER]);
        assert!(bounded_line(&mut oversized, MAX_HEADER).is_err());
        assert_eq!(oversized.position(), (MAX_HEADER + 1) as u64);
        for request in [
            "POST /categories HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 0\r\n\r\n{}",
            "POST /categories HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\n\r\n",
            "GET /categories HTTP/1.1\r\nAuthorization: Bearer a\r\nauthorization: Bearer b\r\n\r\n",
            "POST /categories HTTP/1.1\r\nContent-Length: 2\r\nContent-Type: text/plain\r\n\r\n{}",
            "GET /categories HTTP/1.1\r\n Bad: header\r\n\r\n",
        ] { assert!(parse_request(request.as_bytes()).is_err(), "{request}"); }
        assert!(parse_request(b"POST /categories HTTP/1.1\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}".as_slice()).is_ok());
    }
}
