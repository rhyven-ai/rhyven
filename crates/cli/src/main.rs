mod connect;
mod setup;
use agent_market_core::{
    catalog, conformance, error::ensure, http, registry, store, tools::AgentSession, Result,
    Runtime,
};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    name = "rhyven",
    version,
    about = "Headless apps. One standard. One agent interface."
)]
struct Cli {
    /// Legacy isolated workspace; mutually exclusive with named collections.
    #[arg(long, global = true, conflicts_with_all = ["home", "collection"])]
    workspace: Option<PathBuf>,
    /// Shared package home (default: RHYVEN_HOME or ~/.rhyven).
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// App state collection; created automatically (default: global).
    #[arg(long, global = true)]
    collection: Option<String>,
    #[arg(long, global = true, default_value = "agent")]
    actor: String,
    /// Print machine-readable connection instructions instead of opening the TUI.
    #[arg(long)]
    agent: bool,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum AppCommand {
    /// Create a complete app template with a behavior test.
    Init {
        name: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value="declarative", value_parser=["declarative", "container", "service"])]
        runtime: String,
    },
    /// Validate the app contract.
    Validate { path: PathBuf },
    /// Run isolated app behavior tests.
    Test {
        path: PathBuf,
        #[arg(long)]
        allow_container: bool,
    },
    /// Validate, test and package an app.
    Package {
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Override the container image with an immutable reference.
        #[arg(long)]
        image: Option<String>,
    },
    /// Publish an immutable version to the local registry; GitHub submission uses a registry PR.
    Publish { path: PathBuf },
    /// Export an installed app as a standalone MCP bundle.
    ExportMcp {
        app: String,
        #[arg(long)]
        out: PathBuf,
    },
}
impl From<AppCommand> for Command {
    fn from(command: AppCommand) -> Self {
        match command {
            AppCommand::Init { name, dir, runtime } => Self::New { name, dir, runtime },
            AppCommand::Validate { path } => Self::Validate { path },
            AppCommand::Test {
                path,
                allow_container,
            } => Self::Test {
                path,
                allow_container,
            },
            AppCommand::Package { path, out, image } => Self::Package { path, out, image },
            AppCommand::Publish { path } => Self::Publish { path },
            AppCommand::ExportMcp { app, out } => Self::ExportMcp { app, out },
        }
    }
}
#[derive(Subcommand)]
enum CollectionCommand {
    Current,
    List,
    Use { name: String },
}
#[derive(Subcommand)]
enum ServiceCommand {
    List,
    Start { app: String },
    Stop { app: String },
    Restart { app: String },
    Status { app: String },
    Logs { app: String },
}
#[derive(Subcommand)]
enum DaemonCommand {
    /// Generate an OS startup unit (systemd on Linux, launchd on macOS) for review.
    Unit {
        #[arg(long)]
        out: PathBuf,
    },
    /// Run the local supervisor in the foreground (suitable for an OS service manager).
    Run {
        #[arg(long, default_value_t = 8)]
        max_services: usize,
        #[arg(long, default_value_t = 4096)]
        memory_budget_mb: u64,
        #[arg(long, default_value_t = 4)]
        cpu_budget: u64,
    },
    /// Start a detached supervisor with default limits, preserving existing services.
    Start,
    /// Stop the supervisor and its containers; retain enabled service intent.
    Stop,
    Status,
}
#[derive(Subcommand)]
enum Command {
    /// Show the Apache-2.0 license and attribution notices without opening app state.
    License {
        /// Include dependency license texts bundled in this executable.
        #[arg(long)]
        third_party: bool,
    },
    /// Configure an agent client and verify MCP discovery and collection routing.
    Connect(connect::Options),
    /// Manage persistent container apps in the selected collection.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Manage the user-owned local container supervisor.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Initialize Rhyven and optionally install/start container prerequisites. Safe to rerun.
    Setup {
        #[arg(long)]
        containers: bool,
        /// Authorize the described dependency setup without an interactive confirmation.
        #[arg(long)]
        yes: bool,
        /// Show diagnosis and setup routes without changing anything.
        #[arg(long)]
        plan: bool,
    },
    /// Diagnose local execution capabilities without downloading or running apps.
    Doctor,
    /// Inspect or select the CLI default collection.
    Collection {
        #[command(subcommand)]
        command: CollectionCommand,
    },
    /// Back up a collection, including container data.
    Backup {
        collection_name: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Restore to a new collection (pass --collection NAME).
    Restore {
        path: PathBuf,
        #[arg(long)]
        accept_permissions: bool,
    },
    /// Build, validate, test, package, publish and export apps.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Open the desktop terminal marketplace (also the default command).
    Market,
    /// Initialize an empty local runtime; no built-in domain apps.
    Init,
    /// Search the bundled and locally published registry.
    Search {
        #[arg(default_value = "")]
        query: String,
    },
    List,
    /// Inspect a package and its disclosures before installation.
    Inspect {
        package: String,
    },
    Install {
        package: String,
        #[arg(long)]
        accept_permissions: bool,
    },
    /// Compatible additive upgrades only; permission approval is required again.
    Update {
        package: String,
        #[arg(long)]
        accept_permissions: bool,
    },
    /// Remove the app from discovery; retain its data for reinstall/export.
    #[command(name = "remove", visible_alias = "uninstall")]
    Uninstall {
        app: String,
    },
    /// Invoke a universal interface or compatibility runtime operation with JSON, @file or stdin.
    Call {
        operation: String,
        #[arg(default_value = "{}")]
        arguments: String,
    },
    Tools {
        #[arg(long)]
        app: Option<String>,
    },
    /// Start stdio MCP; default is the three-tool category/function interface.
    Mcp {
        #[arg(long)]
        app: Option<String>,
        #[arg(long, requires = "bundle_sha256")]
        bundle: Option<PathBuf>,
        #[arg(long, requires = "bundle")]
        bundle_sha256: Option<String>,
        /// Use an authenticated shared runtime rather than this local workspace.
        #[arg(long, conflicts_with = "bundle")]
        server: Option<String>,
        /// Environment variable containing the shared server's bearer token.
        #[arg(long, requires = "server", default_value = "RHYVEN_SERVE_TOKEN")]
        token_env: String,
    },
    /// Run an authenticated HTTP API for one shared local workspace.
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 7421)]
        port: u16,
        /// Environment variable containing a required bearer token; it is never persisted.
        #[arg(long, default_value = "RHYVEN_SERVE_TOKEN")]
        token_env: String,
        /// Bind plain HTTP beyond loopback. Put it behind a TLS reverse proxy.
        #[arg(long)]
        allow_insecure_network: bool,
    },
    /// Print a harness config using this executable and absolute workspace.
    Config {
        #[arg(default_value="claude",value_parser=["claude","cline","cursor","vscode","codex"])]
        client: String,
    },
    /// Export an installed app with a runtime binary and generated app-specific MCP.
    #[command(hide = true)]
    ExportMcp {
        app: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Create a complete app package template with an executable contract test.
    #[command(hide = true)]
    New {
        name: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value="declarative", value_parser=["declarative", "container", "service"])]
        runtime: String,
    },
    #[command(hide = true)]
    Validate {
        path: PathBuf,
    },
    #[command(hide = true)]
    Test {
        path: PathBuf,
        #[arg(long)]
        allow_container: bool,
    },
    #[command(hide = true)]
    Package {
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        image: Option<String>,
    },
    /// Publish to the local registry (a Git checkout can carry these immutable files).
    #[command(hide = true)]
    Publish {
        path: PathBuf,
    },
    /// Import package JSON files from a registry directory or Git checkout.
    RegistryImport {
        path: PathBuf,
    },
    /// Fetch registry metadata and GitHub stars without downloading app packages.
    RegistryRefresh {
        repository: String,
        #[arg(long, default_value = "main")]
        git_ref: String,
        #[arg(long)]
        anonymous: bool,
    },
    /// Review and approve a pending marketplace request in a human terminal.
    Approve {
        request_id: String,
    },
    /// Fetch a GitHub index and verified release packages into the offline catalog.
    RegistrySync {
        repository: String,
        #[arg(long, default_value = "main")]
        git_ref: String,
        /// Do not use environment or GitHub CLI credentials (public repositories only).
        #[arg(long)]
        anonymous: bool,
    },
    /// Generate a submission entry with the exact release asset byte hash.
    RegistryEntry {
        path: PathBuf,
        #[arg(long)]
        repository: String,
        #[arg(long)]
        asset_id: u64,
    },
    /// Validate an index, download/hash-check packages and test local contracts.
    RegistryValidate {
        path: PathBuf,
        #[arg(long)]
        base: Option<PathBuf>,
        #[arg(long)]
        anonymous: bool,
    },
    /// Consistent JSON export of local packages, records and audit events.
    Snapshot {
        #[arg(long)]
        out: PathBuf,
    },
    /// Run all five packages' isolated contract tests; no network calls.
    Demo,
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", serde_json::to_string(&e).unwrap());
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    ensure(
        !cli.agent || cli.command.is_none(),
        "configuration",
        "Use --agent without a subcommand",
    )?;
    if let Some(Command::License { third_party }) = &cli.command {
        let mut terms = json!({
            "spdx": "Apache-2.0",
            "license": include_str!("../../../LICENSE"),
            "notice": include_str!("../../../NOTICE")
        });
        if *third_party {
            terms["third_party_notices"] = json!(include_str!(concat!(
                env!("OUT_DIR"),
                "/THIRD_PARTY_NOTICES.txt"
            )));
        }
        println!("{}", serde_json::to_string_pretty(&terms)?);
        return Ok(());
    }
    let home = cli
        .home
        .clone()
        .or_else(|| std::env::var_os("RHYVEN_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".rhyven")))
        .ok_or_else(|| {
            agent_market_core::Error::new("configuration", "Set RHYVEN_HOME or pass --home")
        })?;
    // Restore must run before the normal collection auto-creation.
    let early = match &cli.command {
        Some(Command::Setup {
            containers,
            yes,
            plan,
        }) => {
            ensure(cli.workspace.is_none() && cli.collection.is_none(), "configuration", "Setup uses --home and initializes global; it does not change the selected collection")?;
            Some(setup::run(&home, *containers, *yes, *plan)?)
        }
        Some(Command::Doctor) => Some(agent_market_core::container::doctor()),
        Some(Command::Restore {
            path,
            accept_permissions,
        }) => {
            ensure(
                cli.workspace.is_none(),
                "configuration",
                "Restore requires a named collection",
            )?;
            let name = cli.collection.as_deref().ok_or_else(|| {
                agent_market_core::Error::new(
                    "configuration",
                    "Restore requires --collection NEW_NAME",
                )
            })?;
            Some(agent_market_core::recovery::restore(
                &home,
                name,
                path,
                *accept_permissions,
            )?)
        }
        Some(Command::Collection { command }) => {
            ensure(
                cli.workspace.is_none(),
                "configuration",
                "Collection commands require a Rhyven home",
            )?;
            Some(match command {
                CollectionCommand::Current => {
                    json!({"collection":cli.collection.clone().unwrap_or(agent_market_core::collections::current(&home)?)})
                }
                CollectionCommand::List => agent_market_core::collections::list(&home)?,
                CollectionCommand::Use { name } => {
                    agent_market_core::collections::select(&home, name)?
                }
            })
        }
        Some(Command::Backup {
            collection_name,
            out,
        }) => {
            ensure(
                cli.workspace.is_none(),
                "configuration",
                "Backup requires a named collection",
            )?;
            let root = home.join("collections").join(collection_name);
            ensure(
                agent_market_core::collections::valid_name(collection_name)
                    && root.join("collection.json").exists(),
                "collection",
                "Collection does not exist",
            )?;
            Some(agent_market_core::recovery::backup(
                &Runtime::new(root, &cli.actor)?,
                out,
            )?)
        }
        _ => None,
    };
    if let Some(value) = early {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let runtime = if let Some(workspace) = cli.workspace {
        Runtime::new(workspace, &cli.actor)?
    } else {
        let name = cli
            .collection
            .unwrap_or(agent_market_core::collections::current(&home)?);
        Runtime::collection(home, &name, &cli.actor)?
    };
    use std::io::IsTerminal;
    let default =
        if cli.agent || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            Command::Connect(connect::Options::instructions())
        } else {
            Command::Market
        };
    let command = match cli.command.unwrap_or(default) {
        Command::App { command } => command.into(),
        command => command,
    };
    let value = match command {
        Command::Connect(options) => connect::run(&runtime, options)?,
        Command::Service { command } => {
            let (op, app) = match &command {
                ServiceCommand::List => ("list", None),
                ServiceCommand::Start { app } => ("start", Some(app.as_str())),
                ServiceCommand::Stop { app } => ("stop", Some(app.as_str())),
                ServiceCommand::Restart { app } => ("restart", Some(app.as_str())),
                ServiceCommand::Status { app } => ("status", Some(app.as_str())),
                ServiceCommand::Logs { app } => ("logs", Some(app.as_str())),
            };
            agent_market_core::services::control(&runtime, op, app)?
        }
        Command::Daemon { command } => match command {
            DaemonCommand::Unit { out } => supervisor_unit(&runtime, out)?,
            DaemonCommand::Run {
                max_services,
                memory_budget_mb,
                cpu_budget,
            } => {
                return agent_market_core::services::run(
                    &runtime,
                    max_services,
                    memory_budget_mb,
                    cpu_budget,
                )
            }
            DaemonCommand::Start => start_daemon(&runtime)?,
            DaemonCommand::Stop => {
                agent_market_core::services::daemon_control(&runtime, "shutdown")?
            }
            DaemonCommand::Status => agent_market_core::services::daemon_control(&runtime, "ping")?,
        },
        Command::Setup { .. }
        | Command::Doctor
        | Command::License { .. }
        | Command::App { .. }
        | Command::Collection { .. }
        | Command::Backup { .. }
        | Command::Restore { .. } => unreachable!("command already handled"),
        Command::Market => return agent_market_tui::run(runtime),
        Command::Init => runtime.init()?,
        Command::List => runtime.apps()?,
        Command::Search { query } => json!(runtime
            .catalog()?
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| format!("{} {}", p["name"], p["description"])
                .to_lowercase()
                .contains(&query.to_lowercase()))
            .cloned()
            .collect::<Vec<_>>()),
        Command::Inspect { package } => {
            let p = catalog::resolve(&runtime.root, &package)?;
            json!({"package":p,"sha256":store::hash(&p),"trust":"Unverified","verified":false,"certified":false})
        }
        Command::Install {
            package,
            accept_permissions,
        } => {
            let p = registry::resolve_install(&runtime.root, &package, accept_permissions)?;
            runtime.install(&p, accept_permissions, false)?
        }
        Command::Update {
            package,
            accept_permissions,
        } => {
            let p = registry::resolve_install(&runtime.root, &package, accept_permissions)?;
            runtime.install(&p, accept_permissions, true)?
        }
        Command::Uninstall { app } => runtime.uninstall(&app)?,
        Command::Call {
            operation,
            arguments,
        } => runtime.call(&operation, parse(&arguments)?)?,
        Command::Tools { app } => {
            json!(AgentSession::new(runtime, &app.into_iter().collect::<Vec<_>>())?.definitions())
        }
        Command::Mcp {
            app,
            bundle,
            bundle_sha256,
            server,
            token_env,
        } => {
            let mut selected = app;
            if let Some(path) = bundle {
                let p = catalog::read(&path)?;
                ensure(
                    Some(store::hash(&p)) == bundle_sha256,
                    "integrity",
                    "Exported package changed; regenerate export and review permissions",
                )?;
                if let Some(ref name) = selected {
                    ensure(
                        p["name"] == name.as_str(),
                        "validation",
                        "Bundle/app mismatch",
                    )?;
                }
                // Export was generated only from an already reviewed installed package.
                runtime.install(&p, true, false)?;
                if agent_market_core::services::enabled(&p) {
                    start_daemon(&runtime)?;
                    agent_market_core::services::control(&runtime, "start", p["name"].as_str())?;
                }
                selected = Some(p["name"].as_str().unwrap().into());
            }
            let session = if let Some(server) = server {
                let token = token(&token_env)?;
                AgentSession::remote(&server, token, &selected.into_iter().collect::<Vec<_>>())?
            } else {
                AgentSession::new(runtime, &selected.into_iter().collect::<Vec<_>>())?
            };
            return agent_market_mcp::serve(
                std::io::stdin().lock(),
                std::io::stdout().lock(),
                session,
            );
        }
        Command::Serve {
            host,
            port,
            token_env,
            allow_insecure_network,
        } => {
            return http::serve(
                runtime,
                &host,
                port,
                token(&token_env)?,
                allow_insecure_network,
            );
        }
        Command::Config { client } => {
            let exe = std::env::current_exe()?;
            let args = if let Some(home) = agent_market_core::collections::home_for(&runtime.root)?
            {
                json!([
                    "--home",
                    home,
                    "--collection",
                    runtime.root.file_name().unwrap().to_string_lossy(),
                    "--actor",
                    runtime.actor,
                    "mcp"
                ])
            } else {
                json!(["--workspace", runtime.root, "--actor", runtime.actor, "mcp"])
            };
            if client == "codex" {
                println!(
                    "[mcp_servers.rhyven]\ncommand = {}\nargs = {}",
                    json!(exe.to_string_lossy()),
                    args
                );
                return Ok(());
            }
            if client == "vscode" {
                json!({"servers":{"rhyven":{"type":"stdio","command":exe,"args":args}}})
            } else {
                json!({"mcpServers":{"rhyven":{"command":exe,"args":args}}})
            }
        }
        Command::ExportMcp { app, out } => export(&runtime, &app, out)?,
        Command::New {
            name,
            dir,
            runtime: driver,
        } => {
            ensure(
                catalog::app_name(&name),
                "package",
                "Use publisher/app-name",
            )?;
            let mut p = if driver == "service" {
                serde_json::from_str(include_str!(
                    "../../../examples/container-service-python/app.json"
                ))?
            } else if driver == "container" {
                serde_json::from_str(include_str!("../../../examples/container-python/app.json"))?
            } else {
                catalog::bundled()
                    .into_iter()
                    .find(|p| p["name"] == "rhyven/inventory")
                    .unwrap()
            };
            p["name"] = json!(name);
            p["publisher"] = json!(name.split('/').next().unwrap());
            p["version"] = json!("0.1.0");
            p["description"] = json!(
                "An agent-native asset app; customize objects, actions, rules, guide and tests."
            );
            std::fs::create_dir(&dir)?;
            store::write(&dir.join("app.json"), &p)?;
            std::fs::write(
                dir.join("LICENSE"),
                include_str!("../../../catalog/LICENSE"),
            )?;
            std::fs::write(dir.join("NOTICE"), include_str!("../../../catalog/NOTICE"))?;
            if driver == "service" {
                std::fs::write(
                    dir.join("main.py"),
                    include_str!("../../../examples/container-service-python/main.py"),
                )?;
                std::fs::write(
                    dir.join("rhyven_service.py"),
                    include_str!("../../../examples/container-service-python/rhyven_service.py"),
                )?;
                std::fs::write(
                    dir.join("Dockerfile"),
                    include_str!("../../../examples/container-service-python/Dockerfile"),
                )?;
                std::fs::write(dir.join("README.md"), "# Persistent service\n\nBuild the Docker image, set execution.image to its immutable ID, then validate/test/package. Use `rhyven daemon start`, install with reviewed permissions, and `rhyven service start PUBLISHER/APP`. The remember action requires Project Knowledge 0.4.0; other actions work independently. Stop with `rhyven service stop PUBLISHER/APP`. Retain the included Apache-2.0 LICENSE and NOTICE when distributing derived app code.\n")?;
            } else if driver == "container" {
                std::fs::write(
                    dir.join("main.py"),
                    include_str!("../../../examples/container-python/main.py"),
                )?;
                std::fs::write(
                    dir.join("Dockerfile"),
                    include_str!("../../../examples/container-python/Dockerfile"),
                )?;
                std::fs::write(
                    dir.join("README.md"),
                    include_str!("../../../examples/container-python/README.md"),
                )?;
            } else {
                std::fs::write(dir.join("README.md"),"# Your headless app\n\nEdit app.json, then run `rhyven app validate .` and `rhyven app test .`. No custom MCP server required.\n")?;
            }
            json!({"created":dir,"name":name})
        }
        Command::Validate { path } => {
            let p = catalog::read(&path)?;
            json!({"valid":true,"name":p["name"],"sha256":store::hash(&p),"certified":false})
        }
        Command::Test {
            path,
            allow_container,
        } => conformance::run_with_execution(&catalog::read(&path)?, allow_container)?,
        Command::Package { path, out, image } => {
            let mut p = catalog::read(&path)?;
            if let Some(image) = image {
                ensure(
                    agent_market_core::container::enabled(&p),
                    "package",
                    "--image requires a container app",
                )?;
                p["execution"]["image"] = json!(image);
                catalog::validate(&p)?;
            }
            let tested =
                p["hosting"]["mode"] == "local" && !agent_market_core::container::enabled(&p);
            if tested {
                conformance::run(&p)?;
            }
            write_new(&out, &serde_json::to_vec_pretty(&p)?)?;
            json!({"package":out,"sha256":store::hash(&p),"trust":"Unverified","behavior_tests_run":tested})
        }
        Command::Publish { path } => catalog::publish(&runtime.root, &catalog::read(&path)?)?,
        Command::RegistryImport { path } => {
            let mut results = vec![];
            for entry in std::fs::read_dir(path)? {
                let p = entry?.path();
                if p.extension().is_some_and(|s| s == "json") {
                    results.push(catalog::publish(&runtime.root, &catalog::read(&p)?)?);
                }
            }
            json!(results)
        }
        Command::RegistryRefresh {
            repository,
            git_ref,
            anonymous,
        } => registry::refresh(&runtime.root, &repository, &git_ref, anonymous)?,
        Command::Approve { request_id } => {
            use std::io::IsTerminal;
            ensure(
                std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
                "approval",
                "Approval requires a human terminal or MCP host elicitation",
            )?;
            let review = agent_market_core::marketplace::review(&runtime, &request_id)?;
            println!("{}", serde_json::to_string_pretty(&review)?);
            print!("Approve this exact request? Type yes: ");
            std::io::stdout().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            agent_market_core::marketplace::decide(
                &runtime,
                &request_id,
                review["review_digest"].as_str().unwrap(),
                answer.trim() == "yes",
            )?
        }
        Command::RegistrySync {
            repository,
            git_ref,
            anonymous,
        } => registry::sync(&runtime.root, &repository, &git_ref, anonymous)?,
        Command::RegistryEntry {
            path,
            repository,
            asset_id,
        } => registry::entry(&path, &repository, asset_id)?,
        Command::RegistryValidate {
            path,
            base,
            anonymous,
        } => registry::check(&path, base.as_deref(), anonymous)?,
        Command::Snapshot { out } => {
            let snapshot = runtime.snapshot()?;
            write_new(&out, &serde_json::to_vec_pretty(&snapshot)?)?;
            json!({"snapshot":out,"sha256":store::hash(&snapshot)})
        }
        Command::Demo => json!(catalog::bundled()
            .iter()
            .map(conformance::run)
            .collect::<Result<Vec<_>>>()?),
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn parse(s: &str) -> Result<Value> {
    let raw = if s == "-" {
        let mut v = String::new();
        std::io::stdin().take(1_048_577).read_to_string(&mut v)?;
        v
    } else if let Some(file) = s.strip_prefix('@') {
        std::fs::read_to_string(file)?
    } else {
        s.into()
    };
    ensure(raw.len() <= 1_048_576, "validation", "JSON exceeds 1 MiB")?;
    Ok(serde_json::from_str(&raw)?)
}
fn token(name: &str) -> Result<String> {
    ensure(
        name.starts_with("RHYVEN_")
            && name.len() <= 128
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
        "authentication",
        "Token environment variable must use an uppercase RHYVEN_ name",
    )?;
    let value = std::env::var(name).map_err(|_| {
        agent_market_core::Error::new(
            "authentication",
            format!("Set {name} before starting or connecting to a shared server"),
        )
    })?;
    ensure(
        value.len() >= 16 && value.len() <= 512,
        "authentication",
        "Shared server token must be 16-512 characters",
    )?;
    Ok(value)
}
fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn start_daemon(runtime: &Runtime) -> Result<Value> {
    use std::process::{Command as Process, Stdio};
    if let Ok(status) = agent_market_core::services::daemon_control(runtime, "ping") {
        return Ok(status);
    }
    let base = agent_market_core::services::base(&runtime.root)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(base.join("supervisor/daemon.log"))?;
    let mut child = Process::new(std::env::current_exe()?);
    if let Some(home) = agent_market_core::collections::home_for(&runtime.root)? {
        child.arg("--home").arg(home);
    } else {
        child.arg("--workspace").arg(&runtime.root);
    }
    child
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        child.process_group(0);
    }
    let mut child = child.spawn()?;
    for _ in 0..100 {
        if let Ok(status) = agent_market_core::services::daemon_control(runtime, "ping") {
            return Ok(status);
        }
        if child.try_wait()?.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(agent_market_core::Error::new(
        "service_unavailable",
        format!(
            "Supervisor did not start; inspect {}",
            base.join("supervisor/daemon.log").display()
        ),
    ))
}
fn supervisor_unit(runtime: &Runtime, out: PathBuf) -> Result<Value> {
    let exe = std::env::current_exe()?;
    let base = agent_market_core::services::base(&runtime.root)?;
    let (flag, root) = if let Some(home) = agent_market_core::collections::home_for(&runtime.root)?
    {
        ("--home", home)
    } else {
        ("--workspace", runtime.root.clone())
    };
    let argv = [
        exe.to_string_lossy().into_owned(),
        flag.into(),
        root.to_string_lossy().into_owned(),
        "daemon".into(),
        "run".into(),
    ];
    ensure(
        argv.iter().all(|s| !s.chars().any(char::is_control)),
        "configuration",
        "Startup paths cannot contain control characters",
    )?;
    let label = format!("rhyven-{}", &store::hash(&json!(base))[..12]);
    let (body, kind) = if cfg!(target_os = "linux") {
        let command = argv
            .iter()
            .map(|s| {
                format!(
                    "\"{}\"",
                    s.replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('%', "%%")
                        .replace('$', "$$")
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        (format!("[Unit]\nDescription=Rhyven persistent app supervisor\n\n[Service]\nType=simple\nExecStart={command}\nRestart=on-failure\nRestartSec=5\nTimeoutStopSec=45\n\n[Install]\nWantedBy=default.target\n"), "systemd-user")
    } else if cfg!(target_os = "macos") {
        let escape = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;")
        };
        let args = argv
            .iter()
            .map(|s| format!("<string>{}</string>", escape(s)))
            .collect::<String>();
        (format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array>{args}</array><key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict></dict></plist>\n"), "launchd-user")
    } else {
        return Err(agent_market_core::Error::new(
            "service_unavailable",
            "Startup unit generation supports Linux and macOS",
        ));
    };
    write_new(&out, body.as_bytes())?;
    Ok(
        json!({"generated":out,"kind":kind,"label":label,"enabled":false,"instructions":"Review the generated unit, configure Docker PATH/context and secrets in your OS service manager, then enable it. This command does not modify OS startup settings."}),
    )
}
fn export(runtime: &Runtime, app: &str, out: PathBuf) -> Result<Value> {
    ensure(
        app != agent_market_core::marketplace::APP && app != agent_market_core::services::APP,
        "permission",
        "Platform marketplace cannot be exported",
    )?;
    let mut p = runtime.describe(app)?;
    p.as_object_mut().unwrap().remove("trust");
    p.as_object_mut().unwrap().remove("trust_evidence");
    catalog::validate(&p)?;
    std::fs::create_dir(&out)?;
    let out = std::fs::canonicalize(out)?;
    std::fs::copy(std::env::current_exe()?, out.join("rhyven"))?;
    std::fs::write(out.join("LICENSE"), include_str!("../../../LICENSE"))?;
    std::fs::write(out.join("NOTICE"), include_str!("../../../NOTICE"))?;
    std::fs::write(
        out.join("THIRD_PARTY_NOTICES.txt"),
        include_str!(concat!(env!("OUT_DIR"), "/THIRD_PARTY_NOTICES.txt")),
    )?;
    store::write(&out.join("app.json"), &p)?;
    let digest = store::hash(&p);
    let launch=format!("#!/bin/sh\nset -eu\nDIR=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$DIR/rhyven\" --workspace \"$DIR/state\" mcp --bundle \"$DIR/app.json\" --bundle-sha256 {digest}\n");
    std::fs::write(out.join("launch.sh"), launch)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            out.join("launch.sh"),
            std::fs::Permissions::from_mode(0o755),
        )?;
    }
    let args = json!([
        "--workspace",
        out.join("state"),
        "mcp",
        "--bundle",
        out.join("app.json"),
        "--bundle-sha256",
        digest
    ]);
    store::write(
        &out.join("mcp.json"),
        &json!({"mcpServers":{"rhyven-app":{"command":out.join("rhyven"),"args":args}}}),
    )?;
    std::fs::write(out.join("README.md"),format!("# Standalone {} MCP\n\nRun ./launch.sh. This bundle includes its own runtime and app contract; no Rhyven installation is needed. Data is created under state/. Your original app data is not copied. Same operating system/architecture as the exported executable.\n\nConfigure your harness to launch the absolute path to launch.sh, or use mcp.json. launch.sh remains relocatable; regenerate absolute paths in mcp.json if you move the folder. Tools are generated from this exact reviewed package at startup. Changing the package requires a fresh export. Container exports require Docker and access to their pinned image (the image is not copied into the export). Remote apps retain their disclosed endpoint/auth requirements; no secrets are copied.\n\nService bundles start a dedicated supervisor and persist after the MCP client closes. Stop it from this directory with `./rhyven --workspace ./state daemon stop`. Peer apps are not bundled; install any required peers in the same workspace.\n",app))?;
    Ok(
        json!({"exported":app,"directory":out,"launch":out.join("launch.sh"),"package_sha256":digest,"data_copied":false}),
    )
}
