//! On-demand Python and Node actions. host.execute grants unsandboxed host access.
use crate::{catalog, collections, error::ensure, store, Error, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX: usize = 1_048_576;

pub fn enabled(p: &Value) -> bool {
    p["execution"]["driver"] == "script"
}

fn safe_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c))
        && name.split('/').all(|part| {
            !part.is_empty()
                && !matches!(part, "." | ".." | ".venv" | "node_modules" | "__pycache__")
        })
}

fn version(value: &str) -> Result<(u64, u64)> {
    let parts: Vec<_> = value.split('.').collect();
    ensure(
        parts.len() == 2,
        "package",
        "Runtime minimum version must be major.minor",
    )?;
    Ok((
        parts[0]
            .parse()
            .map_err(|_| Error::new("package", "Invalid runtime version"))?,
        parts[1]
            .parse()
            .map_err(|_| Error::new("package", "Invalid runtime version"))?,
    ))
}

/// Execution metadata can be validated without downloading package source.
pub fn validate(p: &Value) -> Result<()> {
    let e = &p["execution"];
    catalog::keys(
        e,
        &[
            "driver",
            "protocol",
            "language",
            "entrypoint",
            "environment",
            "dependencies",
            "python_version",
            "node_version",
            "timeout_seconds",
        ],
    )?;
    ensure(
        e["protocol"] == "rhyven.action/1",
        "package",
        "Scripts require rhyven.action/1",
    )?;
    ensure(
        matches!(e["language"].as_str(), Some("python" | "javascript")),
        "package",
        "Script language must be python or javascript",
    )?;
    ensure(
        safe_path(e["entrypoint"].as_str().unwrap_or("")),
        "package",
        "Script entrypoint must be a relative package file",
    )?;
    if let Some(mode) = e.get("environment") {
        ensure(
            matches!(mode.as_str(), Some("isolated" | "shared")),
            "package",
            "Environment must be isolated or shared",
        )?;
    }
    if let Some(deps) = e.get("dependencies") {
        catalog::keys(deps, &["pip", "npm"])?;
        for (manager, path) in deps.as_object().unwrap() {
            ensure(
                safe_path(path.as_str().unwrap_or("")),
                "package",
                "Dependency lockfile must be a relative package file",
            )?;
            ensure(
                manager != "npm" || path == "package-lock.json",
                "package",
                "npm requires package-lock.json at the package root",
            )?;
        }
    }
    for key in ["python_version", "node_version"] {
        if let Some(v) = e.get(key) {
            version(v.as_str().unwrap_or(""))?;
        }
    }
    if let Some(v) = e.get("timeout_seconds") {
        ensure(
            v.as_u64().is_some_and(|n| (1..=300).contains(&n)),
            "package",
            "timeout_seconds must be 1..300",
        )?;
    }
    Ok(())
}

pub fn validate_files(p: &Value) -> Result<()> {
    let files = p["files"]
        .as_object()
        .ok_or_else(|| Error::new("package", "Script packages require embedded files"))?;
    ensure(
        !files.is_empty() && files.len() <= 128,
        "package",
        "Provide 1..128 script files",
    )?;
    for (name, text) in files {
        ensure(
            safe_path(name) && text.is_string(),
            "package",
            "Script files must map safe relative paths to UTF-8 text",
        )?;
        ensure(
            !files
                .keys()
                .any(|other| other.starts_with(&format!("{name}/"))),
            "package",
            "A package path cannot be both a file and a directory",
        )?;
    }
    ensure(
        p.to_string().len() <= MAX,
        "package",
        "Script package exceeds 1 MiB",
    )?;
    let e = &p["execution"];
    ensure(
        files.contains_key(e["entrypoint"].as_str().unwrap()),
        "package",
        "Missing script entrypoint",
    )?;
    for (manager, lock) in e["dependencies"].as_object().into_iter().flatten() {
        let content = files
            .get(lock.as_str().unwrap())
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("package", "Missing dependency lockfile"))?;
        if manager == "pip" {
            for line in content
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty() && !s.starts_with('#'))
            {
                let words: Vec<_> = line.split_whitespace().collect();
                let pinned = words[0].split_once("==").is_some_and(|(name, ver)| {
                    !name.is_empty()
                        && !ver.is_empty()
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                        && ver
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".+_-".contains(&b))
                });
                ensure(pinned && words.len() >= 2 && words[1..].iter().all(|v| {
                    v.strip_prefix("--hash=sha256:").is_some_and(crate::container::digest)
                }), "package", "pip lock entries must be name==version followed by SHA-256 hashes, one requirement per line")?;
            }
        } else {
            let lock: Value = serde_json::from_str(content)?;
            let manifest: Value = serde_json::from_str(
                files
                    .get("package.json")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::new("package", "npm requires package.json"))?,
            )?;
            ensure(
                manifest.is_object()
                    && lock["lockfileVersion"] == 3
                    && lock["packages"].is_object(),
                "package",
                "npm requires a version 3 lockfile",
            )?;
            for (name, dependency) in lock["packages"].as_object().unwrap() {
                if name.is_empty() {
                    continue;
                }
                ensure(dependency["link"] != true
                    && dependency["resolved"].as_str().is_some_and(|s| {
                        reqwest::Url::parse(s).is_ok_and(|u| u.scheme() == "https" && u.username().is_empty() && u.password().is_none())
                    })
                    && dependency["integrity"].as_str().is_some_and(|s| s.starts_with("sha512-") && s.len() > 7),
                    "package", "npm dependencies require HTTPS artifacts and SHA-512 integrity; local/git dependencies are unsupported")?;
            }
        }
    }
    Ok(())
}

/// Only explicit source directories may resolve filenames; downloaded packages are self-contained.
pub fn bundle(directory: &Path, p: &mut Value) -> Result<()> {
    let Some(names) = p["files"].as_array() else {
        return Ok(());
    };
    ensure(names.len() <= 128, "package", "At most 128 script files")?;
    let mut files = serde_json::Map::new();
    let mut total = p.to_string().len();
    for name in names {
        let name = name.as_str().unwrap_or("");
        ensure(
            safe_path(name) && !files.contains_key(name),
            "package",
            "Invalid or duplicate script filename",
        )?;
        let mut path = directory.to_path_buf();
        for part in name.split('/') {
            path.push(part);
            ensure(
                !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
                "package",
                "Script source cannot include symlinks",
            )?;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX + 1) as u64)
            .read_to_end(&mut bytes)?;
        total += bytes.len();
        ensure(total <= MAX, "package", "Script package exceeds 1 MiB")?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::new("package", "Script files must be UTF-8"))?;
        files.insert(name.into(), json!(text));
    }
    p["files"] = Value::Object(files);
    Ok(())
}

pub(crate) fn executable(name: &str) -> Result<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if directory.is_absolute() && directory.join(name).is_file() {
            return Ok(directory.join(name));
        }
    }
    Err(Error::new(
        "script_unavailable",
        format!(
            "Install {name} and add its directory to PATH; Rhyven does not install system packages"
        ),
    ))
}

fn clean(command: &mut Command) -> &mut Command {
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LANG", "C.UTF-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1")
}

struct Environment {
    path: PathBuf,
    tools: BTreeMap<String, PathBuf>,
    identity: Value,
}

fn environment(home: &Path, p: &Value) -> Result<Environment> {
    ensure(
        cfg!(unix),
        "script_unavailable",
        "Script execution currently requires Linux or another Unix host",
    )?;
    let e = &p["execution"];
    let mut tools = BTreeMap::new();
    let mut versions = serde_json::Map::new();
    for (language, program, field, minimum) in [
        ("python", "python3", "python_version", "3.10"),
        ("javascript", "node", "node_version", "20.0"),
    ] {
        let manager = if language == "python" { "pip" } else { "npm" };
        if e["language"] != language && e["dependencies"].get(manager).is_none() {
            continue;
        }
        let program_path = executable(program)?;
        let output = run(
            clean(Command::new(&program_path).arg("--version")),
            vec![],
            15,
        )?;
        let release = String::from_utf8_lossy(&output)
            .trim()
            .trim_start_matches("Python ")
            .trim_start_matches('v')
            .to_owned();
        let components: Vec<_> = release.split('.').collect();
        ensure(
            components.len() >= 2,
            "script_unavailable",
            "Could not determine interpreter version",
        )?;
        let actual = version(&components[..2].join("."))?;
        let required = version(e[field].as_str().unwrap_or(minimum))?;
        ensure(
            actual >= required && actual >= version(minimum)?,
            "script_unavailable",
            format!(
                "{program} {release} is too old; requires at least {}.{}",
                required.0, required.1
            ),
        )?;
        versions.insert(
            program.into(),
            json!({"path":program_path,"version":release}),
        );
        tools.insert(program.into(), program_path);
    }
    let dependencies: BTreeMap<_, _> = e["dependencies"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(manager, file)| (manager, p["files"][file.as_str().unwrap()].clone()))
        .collect();
    let identity = json!({"format":1,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"runtimes":versions,"dependencies":dependencies,
        "npm_manifest":if e["dependencies"].get("npm").is_some() {p["files"]["package.json"].clone()} else {Value::Null},
        "package":if e["environment"] == "shared" {Value::Null} else {json!(store::hash(p))}});
    Ok(Environment {
        path: home
            .join("script-runtime/environments")
            .join(store::hash(&identity)),
        tools,
        identity,
    })
}

fn home(root: &Path) -> Result<PathBuf> {
    Ok(collections::home_for(root)?.unwrap_or(collections::state_dir(root)?))
}

fn directory(path: &Path) -> Result<()> {
    store::private_dir(path)?;
    ensure(
        !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
        "integrity",
        "Script runtime directories cannot be symlinks",
    )
}

pub fn prepare(root: &Path, p: &Value) -> Result<()> {
    prepare_home(&home(root)?, p)
}

pub(crate) fn prepare_home(home: &Path, p: &Value) -> Result<()> {
    let env = environment(home, p)?;
    directory(&home.join("script-runtime"))?;
    directory(&home.join("script-runtime/environments"))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("script-runtime/prepare.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let marker = env.path.join("ready.json");
    if marker.exists() {
        ensure(
            serde_json::from_slice::<Value>(&std::fs::read(marker)?)? == env.identity,
            "integrity",
            "Script environment identity changed",
        )?;
        return Ok(());
    }
    if env.path.exists() {
        ensure(
            !std::fs::symlink_metadata(&env.path)?
                .file_type()
                .is_symlink(),
            "integrity",
            "Environment cannot be a symlink",
        )?;
        std::fs::remove_dir_all(&env.path)?;
    }
    // venvs embed absolute paths: build at the final location, publish readiness last.
    directory(&env.path)?;
    let deps = &p["execution"]["dependencies"];
    if let Some(python) = env.tools.get("python3") {
        let mut command = Command::new(python);
        clean(&mut command).args(["-m", "venv"]);
        if deps.get("pip").is_none() {
            command.arg("--without-pip");
        }
        command.arg(env.path.join(".venv"));
        run(&mut command, vec![], 120).map_err(|e| {
            Error::new(
                "script_unavailable",
                format!(
                    "Python environment setup failed; install Python venv support: {}",
                    e.message
                ),
            )
        })?;
        if let Some(file) = deps["pip"].as_str() {
            let lockfile = env.path.join("requirements.lock");
            std::fs::write(&lockfile, p["files"][file].as_str().unwrap())?;
            let mut command = Command::new(env.path.join(".venv/bin/python"));
            clean(&mut command)
                .current_dir(&env.path)
                .env("HOME", &env.path)
                .env("PIP_CONFIG_FILE", "/dev/null")
                .args([
                    "-m",
                    "pip",
                    "install",
                    "--disable-pip-version-check",
                    "--no-input",
                    "--require-hashes",
                    "--only-binary=:all:",
                    "--cache-dir",
                ])
                .arg(home.join("script-runtime/cache/pip"))
                .arg("-r")
                .arg(lockfile);
            run(&mut command, vec![], 300)?;
        }
    }
    if deps.get("npm").is_some() {
        for name in ["package.json", "package-lock.json"] {
            std::fs::write(env.path.join(name), p["files"][name].as_str().unwrap())?;
        }
        let mut command = Command::new(executable("npm")?);
        clean(&mut command)
            .current_dir(&env.path)
            .env("HOME", &env.path)
            .env("NPM_CONFIG_USERCONFIG", "/dev/null")
            .args([
                "ci",
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
                "--cache",
            ])
            .arg(home.join("script-runtime/cache/npm"));
        run(&mut command, vec![], 300)?;
    }
    store::write(&marker, &env.identity)
}

pub(crate) struct Invocation {
    pub command: Command,
    pub data_dir: PathBuf,
    pub(crate) _source: tempfile::TempDir,
}

pub(crate) fn launch(root: &Path, p: &Value) -> Result<Invocation> {
    let env = environment(&home(root)?, p)?;
    let marker = env.path.join("ready.json");
    ensure(marker.is_file(), "script_unavailable", "Script environment missing or interpreter changed; reinstall the reviewed package to rebuild it")?;
    ensure(
        serde_json::from_slice::<Value>(&std::fs::read(marker)?)? == env.identity,
        "integrity",
        "Script environment identity changed",
    )?;
    let source = tempfile::tempdir()?;
    for (name, text) in p["files"].as_object().unwrap() {
        let path = source.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, text.as_str().unwrap())?;
    }
    if p["execution"]["dependencies"].get("npm").is_some() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            env.path.join("node_modules"),
            source.path().join("node_modules"),
        )?;
    }
    // Preserve the existing executable-app data layout and backup format.
    let data_dir = crate::execution::instance(root, p["name"].as_str().unwrap())?.join("data");
    directory(&data_dir)?;
    let data_dir = std::fs::canonicalize(data_dir)?;
    let mut command = Command::new(if p["execution"]["language"] == "python" {
        env.path.join(".venv/bin/python")
    } else {
        env.tools["node"].clone()
    });
    clean(&mut command)
        .arg(
            source
                .path()
                .join(p["execution"]["entrypoint"].as_str().unwrap()),
        )
        .current_dir(source.path())
        .env("HOME", source.path())
        .env("RHYVEN_DATA_DIR", &data_dir)
        .env("RHYVEN_SCRIPT_ACTION", "1");
    let mut paths = vec![
        env.path.join(".venv/bin"),
        env.path.join("node_modules/.bin"),
    ];
    paths.extend(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|p| p.is_absolute()),
    );
    command.env(
        "PATH",
        std::env::join_paths(paths)
            .map_err(|_| Error::new("script_unavailable", "Invalid executable search path"))?,
    );
    Ok(Invocation {
        command,
        data_dir,
        _source: source,
    })
}

/// Bounded pipes and a process group avoid shell interpolation and leaked ordinary children.
#[cfg(unix)]
pub(crate) fn run(command: &mut Command, input: Vec<u8>, timeout: u64) -> Result<Vec<u8>> {
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    struct Process(std::process::Child);
    impl Drop for Process {
        fn drop(&mut self) {
            // Every command starts its own group. Deliberately daemonized host code is outside this guarantee.
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.wait();
        }
    }
    fn nonblocking(fd: i32) -> Result<()> {
        // These descriptors belong to the child pipes and remain open throughout the call.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    fn drain(pipe: &mut impl Read, out: &mut Vec<u8>) -> Result<()> {
        let mut bytes = [0; 8192];
        loop {
            match pipe.read(&mut bytes) {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    out.extend_from_slice(&bytes[..n]);
                    ensure(
                        out.len() <= MAX,
                        "script_protocol",
                        "Script output exceeds 1 MiB",
                    )?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
    let child = command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            Error::new(
                "script_unavailable",
                format!("Cannot start script runtime: {e}"),
            )
        })?;
    let mut process = Process(child);
    let mut stdin = process.0.stdin.take();
    let mut stdout = process.0.stdout.take().unwrap();
    let mut stderr = process.0.stderr.take().unwrap();
    nonblocking(stdin.as_ref().unwrap().as_raw_fd())?;
    nonblocking(stdout.as_raw_fd())?;
    nonblocking(stderr.as_raw_fd())?;
    let (mut out, mut err, mut offset) = (Vec::new(), Vec::new(), 0);
    let start = Instant::now();
    loop {
        if let Some(pipe) = stdin.as_mut() {
            match pipe.write(&input[offset..]) {
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                    stdin.take();
                }
                Err(e) => return Err(e.into()),
            }
            if offset == input.len() {
                stdin.take();
            }
        }
        drain(&mut stdout, &mut out)?;
        drain(&mut stderr, &mut err)?;
        if let Some(status) = process.0.try_wait()? {
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            ensure(
                status.success(),
                "script_failed",
                format!(
                    "Script process failed: {}",
                    String::from_utf8_lossy(if err.is_empty() {
                        &out[..out.len().min(4096)]
                    } else {
                        &err[..err.len().min(4096)]
                    })
                ),
            )?;
            return Ok(out);
        }
        ensure(
            start.elapsed() < Duration::from_secs(timeout),
            "script_timeout",
            "Script timed out; side effects may have occurred. Inspect state before retrying",
        )?;
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(unix))]
pub(crate) fn run(_: &mut Command, _: Vec<u8>, _: u64) -> Result<Vec<u8>> {
    Err(Error::new(
        "script_unavailable",
        "Script execution currently requires a Unix host",
    ))
}

/// Probe only host tools; never prepare dependencies or execute package files.
pub fn requirements(p: &Value) -> Result<Value> {
    let environment = environment(Path::new("."), p)?;
    if p["execution"]["language"] == "python" || p["execution"]["dependencies"].get("pip").is_some()
    {
        let python = environment.tools.get("python3").unwrap();
        run(
            clean(Command::new(python).args(["-c", "import venv, ensurepip"])),
            vec![],
            15,
        )?;
    }
    if p["execution"]["dependencies"].get("npm").is_some() {
        run(
            clean(Command::new(executable("npm")?).arg("--version")),
            vec![],
            15,
        )?;
    }
    Ok(environment.identity["runtimes"].clone())
}
