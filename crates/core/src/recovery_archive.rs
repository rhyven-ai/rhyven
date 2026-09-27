//! Streaming archive framing. Payload memory is fixed; extraction metadata lives in SQLite.
use crate::{error::ensure, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
pub(crate) const MAGIC: &[u8; 16] = b"RHYVEN-BACKUP-3\n";
const HEADER_MAX: usize = 65536;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format: u32,
    collection: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    size: u64,
    directory: bool,
    mode: Option<u32>,
}
struct Hashed<T> {
    inner: T,
    hash: Sha256,
}
impl<T> Hashed<T> {
    fn new(inner: T) -> Self {
        Self {
            inner,
            hash: Sha256::new(),
        }
    }
}
impl<T: Write> Write for Hashed<T> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
impl<T: Read> Read for Hashed<T> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
}
fn frame(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure(
        bytes.len() <= HEADER_MAX,
        "backup",
        "Archive entry metadata exceeds 64 KiB",
    )?;
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    Ok(())
}
fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure(
        length <= HEADER_MAX,
        "integrity",
        "Archive entry metadata exceeds 64 KiB",
    )?;
    if length == 0 {
        return Ok(None);
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}
/// Copy exactly one payload with bounded memory and a per-file checksum.
fn payload(reader: &mut impl Read, writer: &mut impl Write, mut size: u64) -> Result<[u8; 32]> {
    let mut buffer = [0u8; 65536];
    let mut hash = Sha256::new();
    while size > 0 {
        let count = size.min(buffer.len() as u64) as usize;
        reader.read_exact(&mut buffer[..count])?;
        writer.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
        size -= count as u64;
    }
    Ok(hash.finalize().into())
}
fn add(
    writer: &mut impl Write,
    base: &Path,
    path: &Path,
    count: &mut u64,
    total: &mut u64,
) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    ensure(
        meta.is_file() || meta.is_dir(),
        "backup",
        "Only regular files and directories can be backed up; symlinks are forbidden",
    )?;
    let relative = path
        .strip_prefix(base)
        .unwrap()
        .to_str()
        .ok_or_else(|| Error::new("backup", "Paths must be UTF-8"))?;
    if relative.ends_with("/instance.lock") && relative.split('/').count() == 3 {
        return Ok(());
    }
    ensure(
        super::recovery::safe_path(relative, meta.is_dir()),
        "backup",
        "Unsupported archive path",
    )?;
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o777)
    };
    #[cfg(not(unix))]
    let mode = None;
    let entry = Entry {
        path: relative.into(),
        size: if meta.is_dir() { 0 } else { meta.len() },
        directory: meta.is_dir(),
        mode,
    };
    frame(writer, &entry)?;
    let digest = if entry.directory {
        Sha256::digest([]).into()
    } else {
        let mut file = File::open(path)?;
        let digest = payload(&mut file, writer, entry.size)?;
        ensure(
            file.read(&mut [0])? == 0,
            "backup",
            "File grew during backup",
        )?;
        digest
    };
    writer.write_all(&digest)?;
    *count = count
        .checked_add(1)
        .ok_or_else(|| Error::new("backup", "Entry count overflow"))?;
    *total = total
        .checked_add(entry.size)
        .ok_or_else(|| Error::new("backup", "Content length overflow"))?;
    if entry.directory {
        for child in std::fs::read_dir(path)? {
            add(writer, base, &child?.path(), count, total)?;
        }
    }
    Ok(())
}
pub(crate) fn write(state: &Path, snapshot: &Path, out: &Path, scope: Value) -> Result<u64> {
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut writer = Hashed::new(std::io::BufWriter::new(temp.as_file_mut()));
    writer.write_all(MAGIC)?;
    frame(
        &mut writer,
        &Header {
            format: 3,
            collection: scope,
        },
    )?;
    let (mut count, mut total) = (0u64, 0u64);
    add(
        &mut writer,
        snapshot.parent().unwrap(),
        snapshot,
        &mut count,
        &mut total,
    )?;
    if state.join("containers").exists() {
        add(
            &mut writer,
            state,
            &state.join("containers"),
            &mut count,
            &mut total,
        )?;
    }
    writer.write_all(&0u32.to_le_bytes())?;
    writer.write_all(&count.to_le_bytes())?;
    writer.write_all(&total.to_le_bytes())?;
    let digest = writer.hash.clone().finalize();
    writer.inner.write_all(&digest)?;
    writer.inner.flush()?;
    drop(writer);
    temp.as_file().sync_all()?;
    temp.persist_noclobber(out)
        .map_err(|e| Error::new("backup", e.to_string()))?;
    Ok(total)
}
/// On-disk path index bounds RAM even for collections with millions of files.
pub(crate) struct Index {
    db: rusqlite::Connection,
    _file: tempfile::NamedTempFile,
}
impl Index {
    pub(crate) fn new(parent: &Path) -> Result<Self> {
        let file = tempfile::NamedTempFile::new_in(parent)?;
        let db = rusqlite::Connection::open(file.path())?;
        db.execute_batch("PRAGMA cache_size=-2048; PRAGMA temp_store=FILE; CREATE TABLE entries(path TEXT PRIMARY KEY, mode INTEGER); BEGIN;")?;
        Ok(Self { db, _file: file })
    }
    pub(crate) fn register(&self, path: &str, mode: Option<u32>) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO entries(path,mode) VALUES(?1,?2)",
                rusqlite::params![path, mode],
            )
            .map_err(|_| Error::new("integrity", "Duplicate or invalid archive path"))?;
        Ok(())
    }
    pub(crate) fn finish(&self, root: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut stmt = self.db.prepare(
                "SELECT path,mode FROM entries WHERE mode IS NOT NULL ORDER BY length(path) DESC",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                let mode: u32 = row.get(1)?;
                std::fs::set_permissions(root.join(path), std::fs::Permissions::from_mode(mode))?;
            }
        }
        #[cfg(not(unix))]
        let _ = root;
        Ok(())
    }
}
pub(crate) fn extract(file: File, staging: &Path, index: &Index) -> Result<()> {
    let mut reader = Hashed::new(std::io::BufReader::new(file));
    let mut magic = [0; 16];
    reader.read_exact(&mut magic)?;
    ensure(&magic == MAGIC, "integrity", "Invalid archive magic")?;
    let header = read_frame(&mut reader)?
        .ok_or_else(|| Error::new("integrity", "Missing archive header"))?;
    let header: Header = serde_json::from_slice(&header)?;
    ensure(header.format == 3, "backup", "Unsupported archive version")?;
    let (mut count, mut total, mut database) = (0u64, 0u64, false);
    while let Some(header) = read_frame(&mut reader)? {
        let entry: Entry = serde_json::from_slice(&header)?;
        ensure(
            super::recovery::safe_path(&entry.path, entry.directory)
                && (!entry.directory || entry.size == 0)
                && entry.mode.is_none_or(|m| m & !0o777 == 0),
            "integrity",
            "Invalid archive path, size or mode",
        )?;
        index.register(&entry.path, entry.mode)?;
        let path = staging.join(&entry.path);
        let digest = if entry.directory {
            std::fs::create_dir_all(&path)?;
            Sha256::digest([]).into()
        } else {
            std::fs::create_dir_all(path.parent().unwrap())?;
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            let digest = payload(&mut reader, &mut out, entry.size)?;
            out.sync_all()?;
            digest
        };
        let mut expected = [0; 32];
        reader.read_exact(&mut expected)?;
        ensure(digest == expected, "integrity", "File checksum mismatch")?;
        database |= entry.path == "state.sqlite3";
        count = count
            .checked_add(1)
            .ok_or_else(|| Error::new("integrity", "Entry count overflow"))?;
        total = total
            .checked_add(entry.size)
            .ok_or_else(|| Error::new("integrity", "Content length overflow"))?;
    }
    let mut expected_count = [0; 8];
    reader.read_exact(&mut expected_count)?;
    let mut expected_total = [0; 8];
    reader.read_exact(&mut expected_total)?;
    ensure(
        database
            && count == u64::from_le_bytes(expected_count)
            && total == u64::from_le_bytes(expected_total),
        "integrity",
        "Incomplete archive",
    )?;
    let digest: [u8; 32] = reader.hash.finalize().into();
    let mut expected = [0; 32];
    reader.inner.read_exact(&mut expected)?;
    ensure(
        digest == expected && reader.inner.read(&mut [0])? == 0,
        "integrity",
        "Archive checksum mismatch or trailing data",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(entries: &[Entry]) -> Vec<u8> {
        let mut out = Hashed::new(Vec::new());
        out.write_all(MAGIC).unwrap();
        frame(
            &mut out,
            &Header {
                format: 3,
                collection: Value::Null,
            },
        )
        .unwrap();
        for entry in entries {
            frame(&mut out, entry).unwrap();
            out.write_all(&Sha256::digest([])).unwrap();
        }
        out.write_all(&0u32.to_le_bytes()).unwrap();
        out.write_all(&(entries.len() as u64).to_le_bytes())
            .unwrap();
        out.write_all(&0u64.to_le_bytes()).unwrap();
        out.inner.extend(out.hash.finalize());
        out.inner
    }
    fn extract_bytes(bytes: &[u8]) -> Result<()> {
        let dir = tempfile::tempdir()?;
        let archive = dir.path().join("archive");
        std::fs::write(&archive, bytes)?;
        let stage = dir.path().join("stage");
        std::fs::create_dir(&stage)?;
        let index = Index::new(dir.path())?;
        extract(File::open(archive)?, &stage, &index)
    }
    fn entry(path: &str) -> Entry {
        Entry {
            path: path.into(),
            size: 0,
            directory: false,
            mode: Some(0o644),
        }
    }
    #[test]
    fn rejects_duplicate_paths_privilege_bits_and_metadata_corruption() {
        assert!(extract_bytes(&archive(&[entry("state.sqlite3")])).is_ok());
        assert!(
            extract_bytes(&archive(&[entry("state.sqlite3"), entry("state.sqlite3")])).is_err()
        );
        let mut invalid = entry("state.sqlite3");
        invalid.mode = Some(0o4755);
        assert!(extract_bytes(&archive(&[invalid])).is_err());
        let mut bytes = archive(&[entry("state.sqlite3")]);
        let offset = bytes
            .windows(10)
            .position(|b| b == b"\"mode\":420")
            .unwrap();
        bytes[offset + 7..offset + 10].copy_from_slice(b"493");
        assert!(extract_bytes(&bytes).is_err());
        let mut oversized = MAGIC.to_vec();
        oversized.extend(u32::MAX.to_le_bytes());
        assert!(extract_bytes(&oversized).is_err());
    }
}
