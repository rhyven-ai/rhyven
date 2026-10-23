use agent_market_core::{error::ensure, Result, Runtime};
use clap::Subcommand;
use serde_json::{json, Value};
use std::{io::Read, path::PathBuf};

#[derive(Subcommand)]
pub enum Command {
    /// Index explicitly selected UTF-8 files as one complete source snapshot.
    IngestFiles {
        #[arg(required=true, num_args=1..=100)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        source: String,
    },
    /// Refresh one installed local app object's record snapshot.
    IngestRecords {
        category: String,
        object: String,
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
    /// Find source passages using local BM25 ranking.
    Query {
        search: String,
        #[arg(long)]
        source: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: u64,
    },
    Status {
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 50)]
        limit: u64,
    },
    /// Remove indexed content, preserving the original source data.
    RemoveSource { source: String },
}

pub fn run(r: &Runtime, command: Command) -> Result<Value> {
    let (action, args) = match command {
        Command::IngestFiles { paths, source } => {
            let mut documents = Vec::new();
            let mut total = 0;
            for path in paths {
                let meta = std::fs::symlink_metadata(&path)?;
                ensure(
                    meta.is_file() && !meta.file_type().is_symlink(),
                    "validation",
                    "Select regular UTF-8 files, not directories or symlinks",
                )?;
                ensure(
                    meta.len() <= 524288,
                    "validation",
                    "Each file is limited to 512 KiB",
                )?;
                let mut text = String::new();
                std::fs::File::open(&path)?
                    .take(524289)
                    .read_to_string(&mut text)?;
                ensure(
                    text.len() <= 524288,
                    "validation",
                    "Each file is limited to 512 KiB",
                )?;
                total += text.len();
                ensure(
                    total <= 700000,
                    "validation",
                    "Selected files exceed the ingestion request budget; use smaller sources",
                )?;
                let id = path.to_str().ok_or_else(|| {
                    agent_market_core::Error::new("validation", "File paths must be UTF-8")
                })?;
                documents.push(json!({"id":id,"title":path.file_name().and_then(|s|s.to_str()).unwrap_or(id),"text":text,"reference":id}));
            }
            (
                "ingest_files",
                json!({"source":source,"documents":documents}),
            )
        }
        Command::IngestRecords {
            category,
            object,
            fields,
        } => {
            let mut args = json!({"category":category,"object":object});
            if !fields.is_empty() {
                args["fields"] = json!(fields);
            }
            ("ingest_records", args)
        }
        Command::Query {
            search,
            source,
            limit,
        } => {
            let mut args = json!({"search":search,"limit":limit});
            if let Some(source) = source {
                args["source"] = json!(source);
            }
            ("query", args)
        }
        Command::Status { offset, limit } => ("status", json!({"offset":offset,"limit":limit})),
        Command::RemoveSource { source } => ("remove_source", json!({"source":source})),
    };
    r.call("rhyven_call",json!({"category":agent_market_core::wobbegong::APP,"function":format!("action_{action}"),"args":args}))
}
