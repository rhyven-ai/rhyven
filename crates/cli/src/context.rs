use agent_market_core::{enrichment, error::ensure, Result, Runtime};
use clap::Subcommand;
use serde_json::{json, Value};
#[derive(Subcommand)]
pub enum Command {
    /// Export the bundled pre-submit adapter into a new file.
    ExportAdapter { out: std::path::PathBuf },
    /// Show the active setting for a harness connection.
    Status { connection: String },
    /// Enable automatic context. Optionally select an installed filter action.
    Enable {
        connection: String,
        #[arg(long, requires = "action")]
        app: Option<String>,
        #[arg(long, requires = "app")]
        action: Option<String>,
        #[arg(long, conflicts_with = "app")]
        clear_filter: bool,
    },
    /// Skip automatic retrieval on the next prompt. Manual search stays available.
    Disable { connection: String },
    /// Take a packet for a pre-submit adapter, once per actor, connection and turn.
    Take {
        connection: String,
        #[arg(long,default_value="reference",value_parser=["codex","claude","hermes","reference"])]
        client: String,
    },
    /// Preview a packet using JSON input or '-' for stdin.
    Preview {
        #[arg(default_value = "-")]
        input: String,
    },
}
pub fn run(r: &Runtime, cmd: Command) -> Result<Value> {
    match cmd {
        Command::ExportAdapter { out } => {
            use std::io::Write;
            let script = include_bytes!("../../../examples/prompt-enrichment/adapter.py");
            let parent = out
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."));
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(script)?;
            file.persist_noclobber(&out)
                .map_err(|e| agent_market_core::Error::new("io", e.to_string()))?;
            Ok(json!({"adapter":out,"modes":["codex","claude","hermes","reference"]}))
        }
        Command::Status { connection } => enrichment::describe_settings(r, &connection),
        Command::Disable { connection } => enrichment::configure(r, &connection, false, None),
        Command::Enable {
            connection,
            app,
            action,
            clear_filter,
        } => {
            let filter = if clear_filter {
                Some(("", ""))
            } else {
                app.as_deref().zip(action.as_deref())
            };
            enrichment::configure(r, &connection, true, filter)
        }
        Command::Take { connection, client } => {
            if enrichment::settings(r, &connection)?["enabled"] != true {
                return Ok(json!({"addition":null,"reason":"disabled"}));
            }
            let mut args = super::parse("-")?;
            ensure(
                args.is_object(),
                "validation",
                "Expected a context request object",
            )?;
            if args.get("tool").is_none() {
                args["tool"] = json!(enrichment::tool_alias(&client, &connection));
            }
            args["connection"] = json!(connection);
            enrichment::take(r, &args)
        }
        Command::Preview { input } => {
            let args = super::parse(&input)?;
            ensure(
                args.is_object(),
                "validation",
                "Expected a context request object",
            )?;
            r.call("rhyven_call",json!({"category":"rhyven/wobbegong","function":"action_prepare_context","args":args}))
        }
    }
}
