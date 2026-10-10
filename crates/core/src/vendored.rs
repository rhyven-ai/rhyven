//! Compatibility checks for source already embedded in complete apps.
use crate::{catalog, error::ensure, store, Error, Result};
use serde_json::Value;
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
