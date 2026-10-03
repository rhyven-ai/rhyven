use agent_market_core::{error::ensure, Result};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};

const FILES: &[(&str, &str)] = &[
    (
        "use-rhyven/SKILL.md",
        include_str!("../../../skills/use-rhyven/SKILL.md"),
    ),
    (
        "use-rhyven/RULE.md",
        include_str!("../../../skills/use-rhyven/RULE.md"),
    ),
    (
        "publish-rhyven-app/SKILL.md",
        include_str!("../../../skills/publish-rhyven-app/SKILL.md"),
    ),
    (
        "build-rhyven-declarative-app/SKILL.md",
        include_str!("../../../skills/build-rhyven-declarative-app/SKILL.md"),
    ),
    (
        "build-rhyven-script-app/SKILL.md",
        include_str!("../../../skills/build-rhyven-script-app/SKILL.md"),
    ),
    (
        "build-rhyven-container-app/SKILL.md",
        include_str!("../../../skills/build-rhyven-container-app/SKILL.md"),
    ),
    (
        "build-rhyven-service-app/SKILL.md",
        include_str!("../../../skills/build-rhyven-service-app/SKILL.md"),
    ),
];

fn directory(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => ensure(
            fs::symlink_metadata(path)?.file_type().is_dir(),
            "configuration",
            "Skill directory must be a regular directory",
        ),
        Err(e) => Err(e.into()),
    }
}

/// Versioned copies preserve earlier releases and user edits. No agent settings are changed.
pub fn run(home: &Path, install: bool) -> Result<Value> {
    let base = home.join("skills");
    let root = base.join(env!("CARGO_PKG_VERSION"));
    if install {
        fs::create_dir_all(home)?;
        directory(&base)?;
        directory(&root)?;
    }
    let mut files = Vec::new();
    for (name, contents) in FILES {
        let path = root.join(name);
        if install {
            directory(path.parent().unwrap())?;
            let mut staged = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
            staged.write_all(contents.as_bytes())?;
            staged.as_file().sync_all()?;
            if let Err(e) = staged.persist_noclobber(&path) {
                if e.error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(e.error.into());
                }
            }
        }
        let status = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                if fs::read(&path)? == contents.as_bytes() {
                    "installed"
                } else {
                    "modified_preserved"
                }
            }
            Ok(_) => "existing_path_preserved",
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "not_installed",
            Err(e) => return Err(e.into()),
        };
        files.push(json!({"name":name,"path":path,"status":status}));
    }
    Ok(
        json!({"version":env!("CARGO_PKG_VERSION"),"directory":root,"files":files,
        "install":"rhyven skills --install",
        "usage":"Read use-rhyven/SKILL.md or copy a selected skill into your agent's supported skill directory. RULE.md is optional project guidance; merge it with existing instructions. These files do not configure clients or grant permissions."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_install_is_complete_and_preserves_user_edits() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("new-home");
        assert_eq!(
            run(&home, false).unwrap()["files"]
                .as_array()
                .unwrap()
                .len(),
            7
        );
        assert!(!home.exists());
        let report = run(&home, true).unwrap();
        let root = std::path::PathBuf::from(report["directory"].as_str().unwrap());
        for (name, content) in FILES {
            assert_eq!(fs::read_to_string(root.join(name)).unwrap(), *content);
            assert!(content.lines().count() <= 500);
        }
        let edited = root.join("use-rhyven/SKILL.md");
        fs::write(&edited, "My local changes").unwrap();
        fs::remove_file(root.join("use-rhyven/RULE.md")).unwrap();
        let next = run(&home, true).unwrap();
        assert_eq!(next["files"][0]["status"], "modified_preserved");
        assert_eq!(fs::read_to_string(&edited).unwrap(), "My local changes");
        assert_eq!(next["files"][1]["status"], "installed");
    }
    #[cfg(unix)]
    #[test]
    fn installation_rejects_redirected_directories() {
        let temp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), temp.path().join("skills")).unwrap();
        assert!(run(temp.path(), true).is_err());
        assert_eq!(fs::read_dir(other.path()).unwrap().count(), 0);
    }
}
