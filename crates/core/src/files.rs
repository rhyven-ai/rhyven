//! Bounded, app-owned file storage. Imports never open caller-supplied host paths.
use crate::{catalog, error::ensure, schema, store, Error, Result, Runtime};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};

const MAX_FILE: usize = 512 * 1024;
const MAX_EXPANDED: u64 = 8 * 1024 * 1024;
const MAX_ROWS: usize = 10_000;
const MAX_CELLS: usize = 100_000;
const MAX_PAGE: usize = 100;
const MAX_OUTPUT: usize = 256 * 1024;
const OPS: &[&str] = &[
    "file_import",
    "file_inspect",
    "file_read",
    "file_extract",
    "file_export",
    "file_delete",
];
fn invalid(e: impl std::fmt::Display) -> Error {
    Error::new("file_invalid", e.to_string())
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .ok_or_else(|| invalid(format!("{k} must be a string")))
}
pub fn is_action(a: &Value) -> bool {
    OPS.contains(&a["operation"].as_str().unwrap_or(""))
}
pub fn validate_action(p: &Value, a: &Value) -> Result<()> {
    catalog::keys(
        a,
        &["description", "keywords", "operation", "input", "output"],
    )?;
    ensure(
        !crate::execution::enabled(p)
            && !crate::connector::enabled(p)
            && p["hosting"]["mode"] == "local",
        "package",
        "File operations require a local declarative app",
    )?;
    schema::check(&a["input"], 0)?;
    ensure(
        a["input"]["type"] == "object",
        "package",
        "File action input must be an object",
    )?;
    if let Some(output) = a.get("output") {
        schema::check(output, 0)?;
    }
    permission(p, a["operation"].as_str().unwrap())
}
fn permission(p: &Value, op: &str) -> Result<()> {
    let write = matches!(op, "file_import" | "file_export" | "file_delete");
    for name in if write {
        ["state.write", "files.write"]
    } else {
        ["state.read", "files.read"]
    } {
        ensure(
            p["permissions"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(name))),
            "permission",
            format!("File action requires {name}"),
        )?;
    }
    Ok(())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn directory(root: &Path, app: &str) -> Result<PathBuf> {
    let mut path = crate::execution::instance(root, app)?;
    for name in ["data", "files"] {
        path.push(name);
        std::fs::create_dir_all(&path)?;
        ensure(
            !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "permission",
            "File storage cannot be a symlink",
        )?;
    }
    Ok(path)
}
fn filename(name: &str) -> Result<&str> {
    ensure(
        !name.is_empty()
            && name.len() <= 128
            && !name.contains(['/', '\\'])
            && !name.chars().any(char::is_control)
            && name != "."
            && name != "..",
        "validation",
        "Use a filename without a directory",
    )?;
    Ok(name)
}
fn format(name: &str) -> Result<&'static str> {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => Ok("csv"),
        "json" => Ok("json"),
        "txt" | "md" => Ok("text"),
        "xlsx" => Ok("xlsx"),
        _ => Err(invalid("Supported files: .csv, .json, .txt, .md and .xlsx")),
    }
}
fn metadata(db: &Connection, app: &str, id: &str) -> Result<Value> {
    let raw: Option<String> = db
        .query_row(
            "SELECT metadata FROM managed_files WHERE app=?1 AND id=?2",
            params![app, id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(serde_json::from_str(&raw.ok_or_else(|| {
        Error::new("not_found", "Unknown file in this app")
    })?)?)
}
fn load(root: &Path, app: &str, meta: &Value) -> Result<Vec<u8>> {
    let hash = text(meta, "sha256")?;
    ensure(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "file_invalid",
        "Invalid file hash",
    )?;
    let path = directory(root, app)?.join(hash);
    let info = std::fs::symlink_metadata(&path)?;
    ensure(
        info.is_file() && !info.file_type().is_symlink() && info.len() <= MAX_FILE as u64,
        "file_invalid",
        "Unsafe or oversized stored file",
    )?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure(
        bytes.len() <= MAX_FILE && digest(&bytes) == hash,
        "file_invalid",
        "Stored file changed; restore it from a trusted backup",
    )?;
    Ok(bytes)
}
fn save(r: &Runtime, db: &Connection, app: &str, name: &str, bytes: &[u8]) -> Result<Value> {
    filename(name)?;
    ensure(
        bytes.len() <= MAX_FILE,
        "file_limit",
        "Files must be at most 512 KiB",
    )?;
    let kind = format(name)?;
    // Parse once before accepting bytes; unsupported content is not silently stored.
    rows(bytes, kind, &json!({}))?;
    let hash = digest(bytes);
    let dir = directory(&r.root, app)?;
    let path = dir.join(&hash);
    if !path.exists() {
        let mut used = 0u64;
        for entry in std::fs::read_dir(&dir)? {
            used = used.saturating_add(entry?.metadata()?.len());
        }
        ensure(
            used + bytes.len() as u64 <= 64 * 1024 * 1024,
            "file_limit",
            "Managed files are limited to 64 MiB per app; delete unused files",
        )?;
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist_noclobber(&path).map_err(invalid)?;
    } else {
        load(&r.root, app, &json!({"sha256":hash}))?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let meta = json!({"id":id,"filename":name,"format":kind,"size":bytes.len(),"sha256":hash,"created_at":crate::marketplace::now()});
    let count: u64 = db.query_row(
        "SELECT COUNT(*) FROM managed_files WHERE app=?1",
        [app],
        |r| r.get(0),
    )?;
    ensure(
        count < 1000,
        "file_limit",
        "This app already has 1,000 managed files; delete unused files",
    )?;
    db.execute(
        "INSERT INTO managed_files(app,id,metadata) VALUES(?1,?2,?3)",
        params![app, id, meta.to_string()],
    )?;
    Ok(meta)
}
fn collect(root: &Path, app: &str, db: &Connection) -> Result<()> {
    let mut statement = db.prepare("SELECT metadata FROM managed_files WHERE app=?1")?;
    let rows = statement.query_map([app], |r| r.get::<_, String>(0))?;
    let mut keep = std::collections::BTreeSet::new();
    for row in rows {
        let meta: Value = serde_json::from_str(&row?)?;
        keep.insert(text(&meta, "sha256")?.to_owned());
    }
    for entry in std::fs::read_dir(directory(root, app)?)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit()) && !keep.contains(&name)
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
pub fn call(r: &Runtime, db: &Connection, p: &Value, request: &Value) -> Result<Value> {
    let action = &p["actions"][text(request, "action")?];
    let op = text(action, "operation")?;
    permission(p, op)?;
    let input = schema::validate(
        request.get("args").cloned().unwrap_or(json!({})),
        &action["input"],
    )?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS managed_files(app TEXT NOT NULL,id TEXT NOT NULL,metadata TEXT NOT NULL,PRIMARY KEY(app,id));")?;
    let app = text(p, "name")?;
    collect(&r.root, app, db)?;
    let fingerprint = store::hash(&json!([app, request]));
    let receipt = request
        .get("request_id")
        .map(|x| {
            x.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or_else(|| invalid("Invalid request_id"))
        })
        .transpose()?;
    if let Some(id) = receipt {
        if let Some((old, result)) = db
            .query_row(
                "SELECT fingerprint,result FROM receipts WHERE actor=?1 AND request=?2",
                params![r.actor, id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            ensure(
                old == fingerprint,
                "conflict",
                "request_id was used with different arguments",
            )?;
            return Ok(serde_json::from_str(&result)?);
        }
    }
    let result = match op {
        "file_import" => {
            catalog::keys(&input, &["filename", "content_base64"])?;
            let encoded = text(&input, "content_base64")?;
            ensure(
                encoded.len() <= MAX_FILE.div_ceil(3) * 4,
                "file_limit",
                "File exceeds 512 KiB",
            )?;
            save(
                r,
                db,
                app,
                text(&input, "filename")?,
                &STANDARD.decode(encoded).map_err(invalid)?,
            )?
        }
        "file_export" => {
            catalog::keys(&input, &["filename", "text", "rows"])?;
            let name = text(&input, "filename")?;
            let bytes = match format(name)? {
                "text" => text(&input, "text")?.as_bytes().to_vec(),
                "json" => {
                    serde_json::to_vec(input.get("rows").ok_or_else(|| invalid("rows required"))?)?
                }
                "csv" => {
                    let values = input["rows"]
                        .as_array()
                        .ok_or_else(|| invalid("rows must be an array of string arrays"))?;
                    ensure(values.len() <= MAX_ROWS, "file_limit", "Too many rows")?;
                    let mut writer = csv::Writer::from_writer(Vec::new());
                    for row in values {
                        let cells = row
                            .as_array()
                            .ok_or_else(|| invalid("CSV rows must be arrays"))?;
                        ensure(cells.len() <= 100, "file_limit", "At most 100 CSV columns")?;
                        let cells = cells
                            .iter()
                            .map(|x| {
                                x.as_str()
                                    .ok_or_else(|| invalid("CSV cells must be strings"))
                            })
                            .collect::<Result<Vec<_>>>()?;
                        // Escape formulas when spreadsheet programs open the exported file.
                        writer
                            .write_record(cells.into_iter().map(|s| {
                                if s.trim_start().starts_with(['=', '+', '-', '@'])
                                    || s.starts_with(['\t', '\r'])
                                {
                                    format!("'{s}")
                                } else {
                                    s.to_owned()
                                }
                            }))
                            .map_err(invalid)?;
                    }
                    writer.into_inner().map_err(invalid)?
                }
                _ => return Err(invalid("Export supports CSV, JSON and text")),
            };
            save(r, db, app, name, &bytes)?
        }
        "file_inspect" | "file_read" | "file_extract" | "file_delete" => {
            catalog::keys(
                &input,
                if op == "file_extract" {
                    &["file_id", "offset", "limit", "sheet", "pointer"][..]
                } else {
                    &["file_id"][..]
                },
            )?;
            let meta = metadata(db, app, text(&input, "file_id")?)?;
            if op == "file_delete" {
                // Remove metadata transactionally; the next file operation collects unused bytes.
                db.execute(
                    "DELETE FROM managed_files WHERE app=?1 AND id=?2",
                    params![app, meta["id"].as_str().unwrap()],
                )?;
                json!({"deleted":true,"file_id":meta["id"],"cleanup_on_next_file_operation":true})
            } else {
                let bytes = load(&r.root, app, &meta)?;
                if op == "file_read" {
                    json!({"file":meta,"content_base64":STANDARD.encode(bytes)})
                } else if op == "file_inspect" {
                    meta
                } else {
                    let offset = input
                        .get("offset")
                        .map(|v| {
                            v.as_u64()
                                .ok_or_else(|| invalid("offset must be nonnegative"))
                        })
                        .transpose()?
                        .unwrap_or(0) as usize;
                    let limit = input
                        .get("limit")
                        .map(|v| v.as_u64().ok_or_else(|| invalid("limit must be positive")))
                        .transpose()?
                        .unwrap_or(20) as usize;
                    ensure(
                        (1..=MAX_PAGE).contains(&limit) && offset <= MAX_ROWS,
                        "file_limit",
                        "Use limit 1..100 and offset 0..10000",
                    )?;
                    let all = rows(&bytes, text(&meta, "format")?, &input)?;
                    let total = all.len();
                    let mut page = Vec::new();
                    let mut size = 0;
                    for row in all.into_iter().skip(offset).take(limit) {
                        let n = serde_json::to_vec(&row)?.len();
                        ensure(
                            n <= MAX_OUTPUT,
                            "file_limit",
                            "A single row exceeds the extraction output limit",
                        )?;
                        if size + n > MAX_OUTPUT {
                            break;
                        }
                        size += n;
                        page.push(row);
                    }
                    let next = offset + page.len();
                    json!({"file":meta,"rows":page,"total":total,"next_offset":if next<total {json!(next)}else{Value::Null},"content_is_untrusted":true})
                }
            }
        }
        _ => return Err(invalid("Unknown file operation")),
    };
    if let Some(output) = action.get("output") {
        schema::validate(result.clone(), output)?;
    }
    db.execute("INSERT INTO events(app,event) VALUES(?1,?2)",params![app,json!({"operation":op,"actor":r.actor,"time":crate::marketplace::now(),"file_id":result.get("id").or_else(||input.get("file_id")),"request_id":receipt}).to_string()])?;
    if let Some(id) = receipt {
        db.execute(
            "INSERT INTO receipts(actor,request,fingerprint,result) VALUES(?1,?2,?3,?4)",
            params![r.actor, id, fingerprint, result.to_string()],
        )?;
    }
    Ok(result)
}
fn rows(bytes: &[u8], kind: &str, options: &Value) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    match kind {
        "text" => {
            for (i, line) in std::str::from_utf8(bytes)
                .map_err(invalid)?
                .lines()
                .enumerate()
            {
                ensure(i < MAX_ROWS, "file_limit", "At most 10,000 lines")?;
                out.push(json!({"line":i+1,"text":line}));
            }
        }
        "json" => {
            let value: Value = serde_json::from_slice(bytes)?;
            let pointer = options
                .get("pointer")
                .map(|_| text(options, "pointer"))
                .transpose()?
                .unwrap_or("");
            let selected = value
                .pointer(pointer)
                .ok_or_else(|| invalid("JSON pointer does not exist"))?;
            if let Some(array) = selected.as_array() {
                ensure(
                    array.len() <= MAX_ROWS,
                    "file_limit",
                    "At most 10,000 JSON items",
                )?;
                for (i, v) in array.iter().enumerate() {
                    out.push(json!({"pointer":format!("{pointer}/{i}"),"value":v}));
                }
            } else {
                out.push(json!({"pointer":pointer,"value":selected}));
            }
        }
        "csv" => {
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_reader(bytes);
            for (i, row) in reader.records().enumerate() {
                let row = row.map_err(invalid)?;
                ensure(
                    i < MAX_ROWS && row.len() <= 100 && (i + 1) * row.len() <= MAX_CELLS,
                    "file_limit",
                    "CSV row or cell limit exceeded",
                )?;
                out.push(json!({"row":i+1,"values":row.iter().collect::<Vec<_>>()}));
            }
        }
        "xlsx" => out = xlsx(bytes, options)?,
        _ => return Err(invalid("Unsupported format")),
    }
    Ok(out)
}

// Read only worksheet values. No files are extracted, no relationships are fetched,
// and formulas/macros are never evaluated.
fn xlsx(bytes: &[u8], options: &Value) -> Result<Vec<Value>> {
    use quick_xml::events::Event;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    ensure(
        zip.len() <= 256,
        "file_limit",
        "XLSX has too many archive entries",
    )?;
    let mut expanded = 0u64;
    let mut names = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(invalid)?;
        let name = entry.name();
        ensure(
            names.insert(name.to_owned())
                && !name.contains('\\')
                && Path::new(name)
                    .components()
                    .all(|p| matches!(p, Component::Normal(_))),
            "file_invalid",
            "Unsafe or duplicate XLSX archive path",
        )?;
        expanded = expanded
            .checked_add(entry.size())
            .ok_or_else(|| invalid("Archive overflow"))?;
        ensure(
            expanded <= MAX_EXPANDED,
            "file_limit",
            "XLSX expands beyond 8 MiB",
        )?;
        ensure(
            !name.ends_with("vbaProject.bin") && !name.contains("externalLinks/"),
            "file_invalid",
            "Macros and external workbook links are not supported",
        )?;
    }
    let mut read = |name: &str| -> Result<String> {
        let mut s = String::new();
        zip.by_name(name)
            .map_err(invalid)?
            .take(MAX_EXPANDED + 1)
            .read_to_string(&mut s)?;
        ensure(
            s.len() as u64 <= MAX_EXPANDED && !s.contains("<!DOCTYPE") && !s.contains("<!ENTITY"),
            "file_invalid",
            "Unsafe XML",
        )?;
        validate_xml(&s)?;
        Ok(s)
    };
    let workbook = read("xl/workbook.xml")?;
    let mut reader = quick_xml::Reader::from_str(&workbook);
    let mut sheets = Vec::new();
    loop {
        match reader.read_event().map_err(invalid)? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"sheet" => {
                let mut name = String::new();
                let mut id = String::new();
                for a in e.attributes() {
                    let a = a.map_err(invalid)?;
                    let v = a.unescape_value().map_err(invalid)?.into_owned();
                    match a.key.as_ref() {
                        b"name" => name = v,
                        b"r:id" => id = v,
                        _ => {}
                    }
                }
                sheets.push((name, id));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let chosen = if let Some(name) = options.get("sheet") {
        sheets
            .iter()
            .find(|(s, _)| Some(s.as_str()) == name.as_str())
    } else {
        sheets.first()
    }
    .ok_or_else(|| invalid("Worksheet not found"))?;
    let rels = read("xl/_rels/workbook.xml.rels")?;
    let mut reader = quick_xml::Reader::from_str(&rels);
    let mut target = None;
    loop {
        match reader.read_event().map_err(invalid)? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"Relationship" => {
                let mut id = String::new();
                let mut path = String::new();
                let mut external = false;
                for a in e.attributes() {
                    let a = a.map_err(invalid)?;
                    let v = a.unescape_value().map_err(invalid)?.into_owned();
                    match a.key.as_ref() {
                        b"Id" => id = v,
                        b"Target" => path = v,
                        b"TargetMode" => external = v == "External",
                        _ => {}
                    }
                }
                if id == chosen.1 {
                    ensure(!external, "file_invalid", "External worksheet rejected")?;
                    target = Some(if path.starts_with("/xl/") {
                        path[1..].to_owned()
                    } else {
                        format!("xl/{path}")
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let path = target.ok_or_else(|| invalid("Missing worksheet relationship"))?;
    ensure(
        path.starts_with("xl/worksheets/") && !path.contains(".."),
        "file_invalid",
        "Unsafe worksheet target",
    )?;
    let mut shared = Vec::new();
    if names.contains("xl/sharedStrings.xml") {
        let xml = read("xl/sharedStrings.xml")?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().expand_empty_elements = true;
        let mut current = String::new();
        let mut in_text = false;
        loop {
            match reader.read_event().map_err(invalid)? {
                Event::Start(e) if e.local_name().as_ref() == b"si" => current.clear(),
                Event::Start(e) if e.local_name().as_ref() == b"t" => in_text = true,
                Event::GeneralRef(e) if in_text => current.push_str(
                    &quick_xml::escape::unescape(&format!("&{};", e.decode().map_err(invalid)?))
                        .map_err(invalid)?,
                ),
                Event::Text(e) if in_text => current.push_str(
                    &quick_xml::escape::unescape(&e.decode().map_err(invalid)?).map_err(invalid)?,
                ),
                Event::End(e) if e.local_name().as_ref() == b"t" => in_text = false,
                Event::End(e) if e.local_name().as_ref() == b"si" => {
                    ensure(
                        shared.len() < MAX_CELLS,
                        "file_limit",
                        "Too many shared strings",
                    )?;
                    shared.push(current.clone());
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    let xml = read(&path)?;
    let mut reader = quick_xml::Reader::from_str(&xml);
    reader.config_mut().expand_empty_elements = true;
    let mut out = Vec::new();
    let mut cells = Vec::new();
    let mut row = 0;
    let mut total = 0;
    let mut decoded_bytes = 0usize;
    let (mut addr, mut typ, mut value, mut formula) =
        (String::new(), String::new(), String::new(), String::new());
    let mut field = 0;
    let mut has_formula = false;
    loop {
        match reader.read_event().map_err(invalid)? {
            Event::Start(e) if e.local_name().as_ref() == b"row" => {
                row += 1;
                cells.clear();
                for a in e.attributes() {
                    let a = a.map_err(invalid)?;
                    if a.key.as_ref() == b"r" {
                        row = a
                            .unescape_value()
                            .map_err(invalid)?
                            .parse::<u64>()
                            .map_err(invalid)?;
                    }
                }
            }
            Event::Start(e) if e.local_name().as_ref() == b"c" => {
                addr.clear();
                typ.clear();
                value.clear();
                formula.clear();
                has_formula = false;
                field = 0;
                for a in e.attributes() {
                    let a = a.map_err(invalid)?;
                    match a.key.as_ref() {
                        b"r" => addr = a.unescape_value().map_err(invalid)?.into_owned(),
                        b"t" => typ = a.unescape_value().map_err(invalid)?.into_owned(),
                        _ => {}
                    }
                }
            }
            Event::Start(e)
                if e.local_name().as_ref() == b"v" || e.local_name().as_ref() == b"t" =>
            {
                field = 1
            }
            Event::Start(e) if e.local_name().as_ref() == b"f" => {
                field = 2;
                has_formula = true;
            }
            Event::GeneralRef(e) if field > 0 => {
                let entity = format!("&{};", e.decode().map_err(invalid)?);
                let value_ref = quick_xml::escape::unescape(&entity).map_err(invalid)?;
                if field == 1 {
                    value.push_str(&value_ref)
                } else {
                    formula.push_str(&value_ref)
                }
            }
            Event::Text(e) if field > 0 => {
                let decoded = e.decode().map_err(invalid)?;
                let s = quick_xml::escape::unescape(&decoded).map_err(invalid)?;
                if field == 1 {
                    value.push_str(&s)
                } else {
                    formula.push_str(&s)
                }
            }
            Event::End(e)
                if e.local_name().as_ref() == b"v"
                    || e.local_name().as_ref() == b"t"
                    || e.local_name().as_ref() == b"f" =>
            {
                field = 0
            }
            Event::End(e) if e.local_name().as_ref() == b"c" => {
                total += 1;
                ensure(
                    total <= MAX_CELLS && cells.len() < 100,
                    "file_limit",
                    "XLSX cell limit exceeded",
                )?;
                if typ == "s" {
                    let entry = shared
                        .get(value.parse::<usize>().map_err(invalid)?)
                        .ok_or_else(|| invalid("Invalid shared string index"))?;
                    ensure(
                        entry.len() <= 65_536,
                        "file_limit",
                        "XLSX cell exceeds 64 KiB",
                    )?;
                    value = entry.clone();
                }
                decoded_bytes = decoded_bytes.saturating_add(value.len() + formula.len());
                ensure(
                    value.len() <= 65_536
                        && formula.len() <= 65_536
                        && decoded_bytes <= MAX_EXPANDED as usize,
                    "file_limit",
                    "XLSX decoded cell size limit exceeded",
                )?;
                cells.push(json!({"cell":addr,"raw":value,"type":typ,"formula":if formula.is_empty(){Value::Null}else{json!(formula)},"formula_cache_unverified":has_formula}));
            }
            Event::End(e) if e.local_name().as_ref() == b"row" => {
                ensure(out.len() < MAX_ROWS, "file_limit", "Too many XLSX rows")?;
                out.push(json!({"sheet":chosen.0,"row":row,"cells":cells}));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn validate_xml(xml: &str) -> Result<()> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut depth = 0usize;
    let mut roots = 0;
    loop {
        match reader.read_event().map_err(invalid)? {
            Event::Start(_) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                ensure(
                    depth <= 64 && roots == 1,
                    "file_invalid",
                    "Invalid XML structure or nesting",
                )?;
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("Unexpected XML end"))?;
            }
            Event::DocType(_) => return Err(invalid("XML document types are not supported")),
            Event::Eof => {
                ensure(depth == 0 && roots == 1, "file_invalid", "Incomplete XML")?;
                break;
            }
            _ => {}
        }
    }
    Ok(())
}
