//! MCP client configuration and connection verification.
use agent_market_core::{collections, error::ensure, Error, Result, Runtime};
use clap::Args;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

#[derive(Args)]
pub struct Options {
    /// Client whose configuration to merge. Generic prints a portable MCP entry.
    #[arg(long, default_value = "generic", value_parser = ["generic", "codex", "claude", "cursor", "vscode", "cline", "hermes", "openclaw"])]
    pub client: String,
    /// Print instructions without configuring a client or starting the probe.
    #[arg(long, conflicts_with = "check")]
    pub print: bool,
    /// Check the generated MCP connection without modifying client configuration.
    #[arg(long)]
    pub check: bool,
    /// Explicit client settings file; required for Cline and useful for agent profiles.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Entry name, allowing several collections in the same agent client.
    #[arg(long, default_value = "rhyven")]
    pub name: String,
    /// Replace a different entry with this name. Other settings are preserved.
    #[arg(long)]
    pub replace: bool,
    /// Connect the local stdio bridge to a shared REST server.
    #[arg(long)]
    pub server: Option<String>,
    #[arg(long, default_value = "RHYVEN_SERVE_TOKEN", requires = "server")]
    pub token_env: String,
    /// Fail verification if the remote server reports a different collection.
    #[arg(long, requires = "server")]
    pub expect_collection: Option<String>,
}

impl Options {
    pub fn instructions() -> Self {
        Self {
            client: "generic".into(),
            print: true,
            check: false,
            config: None,
            name: "rhyven".into(),
            replace: false,
            server: None,
            token_env: "RHYVEN_SERVE_TOKEN".into(),
            expect_collection: None,
        }
    }
}

pub fn launch(runtime: &Runtime) -> Result<(PathBuf, Vec<String>)> {
    let mut args = if let Some(home) = collections::home_for(&runtime.root)? {
        vec![
            "--home".into(),
            home.to_string_lossy().into_owned(),
            "--collection".into(),
            runtime
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ]
    } else {
        vec![
            "--workspace".into(),
            runtime.root.to_string_lossy().into_owned(),
        ]
    };
    args.extend(["--actor".into(), runtime.actor.clone(), "mcp".into()]);
    Ok((std::env::current_exe()?, args))
}

pub fn run(runtime: &Runtime, options: Options) -> Result<Value> {
    ensure(
        options.name != "__proto__" && !options.name.is_empty()
            && options.name.len() <= 64
            && options
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "configuration",
        "Connection name must contain 1-64 letters, digits, underscores or hyphens and cannot be __proto__",
    )?;
    ensure(
        options
            .token_env
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
            && !options.token_env.is_empty(),
        "configuration",
        "Invalid token environment variable name",
    )?;
    let (exe, mut args) = launch(runtime)?;
    let local_collection = collections::scope(&runtime.root)?["collection"].clone();
    let mut environment = Vec::<String>::new();
    if let Some(server) = &options.server {
        // Validate the URL even for print-only mode, without needing the real token.
        agent_market_core::http::HttpClient::new(server, "connection-instructions".into())?;
        args.extend([
            "--server".into(),
            server.clone(),
            "--token-env".into(),
            options.token_env.clone(),
        ]);
        environment.push(options.token_env.clone());
        environment.push("RHYVEN_MANAGEMENT_TOKEN".into());
    }
    let entry = json!({"command":exe,"args":args});
    let mut config = json!({"mcpServers":{&options.name:entry}});
    if options.client == "vscode" {
        config = json!({"servers":{&options.name:{"type":"stdio","command":exe,"args":args}}});
    } else if options.client == "codex" {
        config = json!({"mcp_servers":{&options.name:{"command":exe,"args":args,"env_vars":environment}}});
    } else if options.client == "claude" {
        config["mcpServers"][&options.name]["type"] = json!("stdio");
    }
    if options.client == "hermes" {
        config = json!({"mcp_servers":{&options.name:entry}});
    } else if options.client == "openclaw" {
        config = json!({"mcp":{"servers":{&options.name:{"transport":"stdio","command":exe,"args":args}}}});
    }
    let mut argv: Vec<String> = vec![exe.to_string_lossy().into_owned()];
    argv.extend(args.iter().take_while(|s| s.as_str() != "--actor").cloned());
    argv.extend(["connect".into(), "--client".into(), "CLIENT".into()]);
    let mut result = json!({
        "version":env!("CARGO_PKG_VERSION"), "status":"instructions", "client":options.client,
        "collection":if options.server.is_some() {Value::Null} else {local_collection},
        "collection_source":if options.server.is_some() {"remote server; local --collection does not route remote calls"} else {"pinned in MCP launch arguments"},
        "transport":"stdio", "rest_server":options.server, "configuration":config,
        "configuration_format":if options.client == "codex" {"TOML (shown as JSON here)"} else if options.client == "hermes" {"YAML (shown as JSON here)"} else if options.client == "openclaw" {"JSON5"} else {"JSON"},
        "required_environment":if options.server.is_some() {json!([options.token_env])} else {json!([])},
        "optional_environment":if options.server.is_some() {json!(["RHYVEN_MANAGEMENT_TOKEN"])} else {json!([])},
        "configured":false,"server_verified":false,"client_session_verified":false,
        "connect_argv":argv,"supported_clients":["codex","claude","cursor","vscode","cline","hermes","openclaw","generic"],
        "instructions":[
            "Run connect --client CLIENT with the same --home/--collection (or --workspace). It configures the client and verifies MCP discovery. Use --print to review first; --check verifies without writing configuration.",
            "Generic clients: merge configuration into your client's MCP settings. Cline: pass --config with its MCP settings file. Restart/reload the client if needed, then call rhyven_categories and confirm collection.",
            "Use rhyven_categories(), rhyven_describe(category), then rhyven_call(category,function,args). The marketplace is rhyven/marketplace; installed apps appear without restarting MCP.",
            "Show app descriptions, stars and permissions, and request human approval before downloads. Never approve your own installation request.",
            "For HTTP, run serve with a bearer token and use connect --server URL. The URL is REST, not native HTTP MCP. Supply tokens to the client process environment; they are never embedded in this output."
        ],
        "client_next_step":"Reload/restart your agent client, call rhyven_categories(), and verify its collection. A successful setup probe does not prove your running client has reloaded."
    });
    let executable_name = match options.client.as_str() {
        "vscode" => "code",
        "generic" => "",
        client => client,
    };
    let client_executable = find_executable(executable_name);
    result["client_executable"] = json!(client_executable);
    result["client_detection"] = json!(if executable_name.is_empty() {
        "not_applicable"
    } else if client_executable.is_some() {
        "executable_found"
    } else {
        "not_found_on_path"
    });
    if !executable_name.is_empty() && client_executable.is_none() {
        result["client_warning"] = json!("Client executable not found on PATH. Configuration can still be prepared, but an installed client must load it before agents can use Rhyven. An editor extension or remote client may exist outside PATH.");
    }
    if options.client == "hermes" {
        result["client_next_step"] = json!("In Hermes, use /reload-mcp or start a new session. Call rhyven_categories() and confirm the collection. Use --config for a non-default profile's config.yaml.");
    } else if options.client == "openclaw" {
        result["client_next_step"] = json!("Use an OpenClaw version with native mcp.servers support. Run openclaw mcp doctor NAME --probe with your connection name, then reload/restart the Gateway that runs your agent and call rhyven_categories(). Older mcporter-only configurations need a generic MCP entry instead.");
    }
    if ["hermes", "openclaw"].contains(&options.client.as_str()) {
        result["configuration_note"] = json!("Existing configuration is backed up before changes. YAML/JSON5 formatting and comments may be normalized; unrelated settings are retained.");
    }
    if options.print {
        return Ok(result);
    }
    let verified = probe(&exe, &args)?;
    if let Some(expected) = &options.expect_collection {
        ensure(
            verified["collection"] == expected.as_str(),
            "collection",
            "Remote collection differs from --expect-collection; client configuration unchanged",
        )?;
    }
    result["collection"] = verified["collection"].clone();
    result["server_verified"] = json!(true);
    result["verification"] = verified;
    result["status"] = json!("verified");
    if !options.check && options.client != "generic" {
        let path = config_path(&options)?;
        let changed = merge(
            &path,
            &options.client,
            &options.name,
            &config,
            options.replace,
        )?;
        result["configured"] = json!(true);
        result["configuration_changed"] = json!(changed);
        result["config_path"] = json!(path);
        result["status"] = json!("configured");
    }
    Ok(result)
}

fn find_executable(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(name))
        .find(|path| {
            path.metadata().is_ok_and(|metadata| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    metadata.is_file()
                }
            })
        })
}

fn config_path(options: &Options) -> Result<PathBuf> {
    if let Some(path) = &options.config {
        return Ok(std::path::absolute(path)?);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("configuration", "Set HOME or pass --config"))?;
    Ok(match options.client.as_str() {
        "codex" => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".codex"))
            .join("config.toml"),
        "hermes" => std::env::var_os("HERMES_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".hermes"))
            .join("config.yaml"),
        "openclaw" => std::env::var_os("OPENCLAW_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("OPENCLAW_STATE_DIR")
                    .map(PathBuf::from)
                    .unwrap_or(home.join(".openclaw"))
                    .join("openclaw.json")
            }),
        "claude" => home.join(".claude.json"),
        "cursor" => home.join(".cursor/mcp.json"),
        "vscode" => std::env::current_dir()?.join(".vscode/mcp.json"),
        _ => {
            return Err(Error::new(
                "configuration",
                "Pass --config with the active client's MCP settings file. For Cline, open MCP Servers > Configure > Configure MCP Servers to locate it. CLI example: ~/.cline/mcp.json; Linux VS Code example: ~/.config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json. Choose the file used by your installation; Rhyven does not choose between profiles.",
            ))
        }
    })
}

fn merge(path: &Path, client: &str, name: &str, config: &Value, replace: bool) -> Result<bool> {
    let before = match fs::read(path) {
        Ok(bytes) => {
            ensure(
                !fs::symlink_metadata(path)?.file_type().is_symlink(),
                "configuration",
                "Refusing to replace a symlinked client configuration; pass its real path",
            )?;
            ensure(
                bytes.len() <= 1_048_576,
                "configuration",
                "Client configuration exceeds 1 MiB",
            )?;
            Some(bytes)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ensure(
                fs::symlink_metadata(path).is_err(),
                "configuration",
                "Refusing a dangling config symlink",
            )?;
            None
        }
        Err(e) => return Err(e.into()),
    };
    let text = std::str::from_utf8(before.as_deref().unwrap_or(b""))
        .map_err(|_| Error::new("configuration", "Client configuration is not UTF-8"))?;
    let bytes = if client == "codex" {
        let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|_| {
            Error::new(
                "configuration",
                "Invalid existing TOML; configuration unchanged",
            )
        })?;
        let mut table = toml_edit::Table::new();
        let entry = &config["mcp_servers"][name];
        table["command"] = toml_edit::value(entry["command"].as_str().unwrap());
        for key in ["args", "env_vars"] {
            let mut array = toml_edit::Array::new();
            for item in entry[key].as_array().unwrap() {
                array.push(item.as_str().unwrap());
            }
            table[key] = toml_edit::value(array);
        }
        if let Some(existing) = doc.get("mcp_servers").and_then(|v| v.get(name)) {
            let strings = |key: &str| {
                existing
                    .get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(String::from)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            if existing.get("command").and_then(|v| v.as_str()) == entry["command"].as_str()
                && json!(strings("args")) == entry["args"]
                && json!(strings("env_vars")) == entry["env_vars"]
                && existing.get("enabled").and_then(|v| v.as_bool()) != Some(false)
            {
                return Ok(false);
            }
            ensure(replace, "configuration", "A different Rhyven connection exists; use --name for another entry or --replace to switch it")?;
        }
        if !doc.contains_key("mcp_servers") {
            doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        ensure(
            doc["mcp_servers"].is_table(),
            "configuration",
            "mcp_servers must be a TOML table; configuration unchanged",
        )?;
        doc["mcp_servers"][name] = toml_edit::Item::Table(table);
        doc.to_string().into_bytes()
    } else {
        let mut doc: Value = if text.is_empty() {
            json!({})
        } else if client == "hermes" {
            serde_yaml_ng::from_str(text).map_err(|_| Error::new("configuration", "Invalid YAML or unsupported settings; configuration unchanged. Merge --print output manually."))?
        } else if client == "openclaw" {
            json5::from_str(text).map_err(|_| {
                Error::new("configuration", "Invalid JSON5; configuration unchanged")
            })?
        } else {
            serde_json::from_str(text).map_err(|_| Error::new("configuration", "Existing configuration must be valid JSON (comments are not rewritten); merge the --print output manually"))?
        };
        ensure(
            doc.is_object(),
            "configuration",
            "Client configuration must be an object",
        )?;
        let keys: &[&str] = match client {
            "vscode" => &["servers"],
            "hermes" => &["mcp_servers"],
            "openclaw" => &["mcp", "servers"],
            _ => &["mcpServers"],
        };
        let mut target = &mut doc;
        let mut source = config;
        for key in keys {
            if target.get(*key).is_none() {
                target[*key] = json!({});
            }
            ensure(
                target[*key].is_object(),
                "configuration",
                "MCP server settings must be an object; configuration unchanged",
            )?;
            target = &mut target[*key];
            source = &source[*key];
        }
        let entry = &source[name];
        if let Some(existing) = target.get(name) {
            if existing == entry {
                return Ok(false);
            }
            ensure(replace, "configuration", "A different Rhyven connection exists; use --name for another entry or --replace to switch it")?;
        }
        target[name] = entry.clone();
        if client == "hermes" {
            merge_hermes(text, name, &doc["mcp_servers"])?
        } else {
            serde_json::to_vec_pretty(&doc)?
        }
    };
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("configuration", "Invalid configuration path"))?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    if before.is_some() {
        staged
            .as_file()
            .set_permissions(fs::metadata(path)?.permissions())?;
    }
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    ensure(
        fs::read(path).ok() == before,
        "configuration",
        "Client configuration changed during setup; retry",
    )?;
    if ["hermes", "openclaw"].contains(&client) {
        if let Some(original) = &before {
            let mut backup = tempfile::Builder::new()
                .prefix(&format!(
                    "{}.rhyven-backup-",
                    path.file_name().unwrap().to_string_lossy()
                ))
                .tempfile_in(parent)?;
            backup.write_all(original)?;
            backup.as_file().sync_all()?;
            backup.keep().map_err(|e| Error::new("io", e.to_string()))?;
        }
    }
    staged
        .persist(path)
        .map_err(|e| Error::new("io", e.to_string()))?;
    Ok(true)
}

/// Change only the top-level MCP block. Leave the rest of Hermes YAML verbatim,
/// including comments and YAML 1.1 scalars interpreted by Hermes's Python loader.
fn merge_hermes(text: &str, name: &str, servers: &Value) -> Result<Vec<u8>> {
    let manual = || {
        Error::new(
            "configuration",
            "Cannot safely merge Hermes YAML; merge --print output manually",
        )
    };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let original: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_yaml_ng::from_str(text).map_err(|_| manual())?
    };
    let mut expected = original.clone();
    expected["mcp_servers"] = servers.clone();
    let mut output;
    if original.get("mcp_servers").is_some() {
        let starts: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| line.starts_with("mcp_servers:").then_some(i))
            .collect();
        ensure(
            starts.len() == 1,
            "configuration",
            "Hermes needs a plain top-level mcp_servers block; merge --print output manually",
        )?;
        let start = starts[0];
        let end = (start + 1..lines.len())
            .find(|&i| {
                let line = lines[i];
                !line.trim().is_empty() && !line.starts_with([' ', '\t', '#'])
            })
            .unwrap_or(lines.len());
        let tail = lines[start]
            .trim_end()
            .strip_prefix("mcp_servers:")
            .unwrap()
            .trim();
        if !tail.is_empty() && !tail.starts_with('#') {
            // Previously generated JSON is also YAML. Do not reinterpret YAML 1.1 flow scalars.
            let flow: Value = serde_json::from_str(tail).map_err(|_| manual())?;
            ensure(
                flow == original["mcp_servers"],
                "configuration",
                "Complex Hermes YAML requires manual merge",
            )?;
            output = lines[..start].concat();
            output.push_str(&format!(
                "mcp_servers: {}\n",
                serde_json::to_string(servers)?
            ));
            output.push_str(&lines[end..].concat());
        } else {
            let indent = lines[start + 1..end]
                .iter()
                .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .map(|line| line.len() - line.trim_start_matches(' ').len())
                .min()
                .ok_or_else(manual)?;
            ensure(
                indent > 0,
                "configuration",
                "Invalid Hermes MCP indentation",
            )?;
            let prefixes = [
                format!("{name}:"),
                format!("\"{name}\":"),
                format!("'{name}':"),
            ];
            let entry_start = (start + 1..end).find(|&i| {
                let line = lines[i];
                line.len() - line.trim_start_matches(' ').len() == indent
                    && prefixes
                        .iter()
                        .any(|prefix| line.trim_start().starts_with(prefix))
            });
            ensure(
                entry_start.is_some() == original["mcp_servers"].get(name).is_some(),
                "configuration",
                "Complex Hermes MCP key requires manual merge",
            )?;
            let replace_start = entry_start.unwrap_or(end);
            let replace_end = entry_start
                .map(|from| {
                    (from + 1..end)
                        .find(|&i| {
                            let line = lines[i];
                            !line.trim().is_empty()
                                && !line.trim_start().starts_with('#')
                                && line.len() - line.trim_start_matches(' ').len() <= indent
                        })
                        .unwrap_or(end)
                })
                .unwrap_or(end);
            output = lines[..replace_start].concat();
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&format!(
                "{}{}: {}\n",
                " ".repeat(indent),
                serde_json::to_string(name)?,
                serde_json::to_string(&servers[name])?
            ));
            output.push_str(&lines[replace_end..].concat());
        }
    } else {
        ensure(!text.lines().any(|line| matches!(line.trim(), "---" | "...") || line.starts_with('{')), "configuration", "Hermes YAML document markers or flow mappings require a manual merge of --print output")?;
        output = text.to_string();
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&format!(
            "mcp_servers: {}\n",
            serde_json::to_string(servers)?
        ));
    }
    let checked: Value = serde_yaml_ng::from_str(&output).map_err(|_| manual())?;
    ensure(
        checked == expected,
        "configuration",
        "Cannot safely merge Hermes MCP settings",
    )?;
    Ok(output.into_bytes())
}

/// Probe the actual executable/transport, not a second in-process dispatch path.
fn probe(exe: &Path, args: &[String]) -> Result<Value> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut check = || -> Result<Value> {
            fn exchange(
                input: &mut impl Write,
                output: &mut impl BufRead,
                id: u64,
                method: &str,
                params: Value,
            ) -> Result<Value> {
                writeln!(
                    input,
                    "{}",
                    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
                )?;
                input.flush()?;
                let mut line = String::new();
                output.take(1_048_577).read_line(&mut line)?;
                ensure(!line.is_empty() && line.len() <= 1_048_576, "connection", "MCP response missing or too large; check executable, server and token environment")?;
                let value: Value = serde_json::from_str(&line)?;
                ensure(
                    value["id"] == id
                        && value.get("error").is_none()
                        && value["result"]["isError"] != true,
                    "connection",
                    "MCP probe failed; check server availability, token environment and collection",
                )?;
                Ok(value["result"].clone())
            }
            let initialize = exchange(
                &mut input,
                &mut output,
                1,
                "initialize",
                json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"rhyven-connect-check","version":env!("CARGO_PKG_VERSION")}}),
            )?;
            writeln!(
                input,
                "{}",
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )?;
            let tools = exchange(&mut input, &mut output, 2, "tools/list", json!({}))?;
            let names: Vec<&str> = tools["tools"]
                .as_array()
                .ok_or_else(|| Error::new("connection", "Missing MCP tools"))?
                .iter()
                .filter_map(|t| t["name"].as_str())
                .collect();
            ensure(
                names.len() == 3
                    && ["rhyven_categories", "rhyven_describe", "rhyven_call"]
                        .iter()
                        .all(|n| names.contains(n)),
                "connection",
                "Unexpected MCP interface",
            )?;
            let response = exchange(
                &mut input,
                &mut output,
                3,
                "tools/call",
                json!({"name":"rhyven_categories","arguments":{}}),
            )?;
            let discovery = response["structuredContent"].clone();
            ensure(
                discovery.get("collection").is_some() && discovery["apps"].is_array(),
                "connection",
                "Discovery omitted collection or apps",
            )?;
            Ok(
                json!({"server":initialize["serverInfo"],"protocol":initialize["protocolVersion"],"tools":names,"collection":discovery["collection"],"apps":discovery["apps"],"check":"initialize, tools/list, rhyven_categories"}),
            )
        };
        let _ = tx.send(check());
    });
    let result = rx.recv_timeout(Duration::from_secs(35)).map_err(|_| {
        Error::new(
            "connection",
            "MCP verification timed out; client configuration unchanged",
        )
    });
    let _ = child.kill();
    let _ = child.wait();
    result?
}
