//! Unix supervisor. Only the owning user can open the control socket.
use super::*;
use rusqlite::OptionalExtension;
use std::{
    collections::{BTreeMap, VecDeque},
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Instant,
};

fn control_dir(base: &Path) -> Result<PathBuf> {
    let p = base.join("supervisor");
    match std::fs::DirBuilder::new().mode(0o700).create(&p) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    let m = std::fs::symlink_metadata(&p)?;
    ensure(
        m.is_dir() && !m.file_type().is_symlink() && m.permissions().mode() & 0o077 == 0,
        "permission",
        "Supervisor directory must be a private directory (mode 0700)",
    )?;
    Ok(p)
}
fn socket(base: &Path) -> Result<PathBuf> {
    let p = control_dir(base)?.join("control.sock");
    ensure(
        p.as_os_str().len() < 100,
        "configuration",
        "Rhyven home path is too long for the supervisor socket; choose a shorter --home",
    )?;
    Ok(p)
}
fn frame(reader: &mut impl BufRead) -> Result<Value> {
    let mut bytes = vec![];
    reader
        .take((MAX + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    ensure(
        bytes.len() <= MAX && bytes.last() == Some(&b'\n'),
        "service_protocol",
        "Expected bounded newline-terminated JSON frame",
    )?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn write_frame(writer: &mut impl Write, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    ensure(
        bytes.len() < MAX,
        "service_protocol",
        "Service frame exceeds 1 MiB",
    )?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
pub(super) fn request(base: &Path, input: Value) -> Result<Value> {
    let mut s = UnixStream::connect(socket(base)?).map_err(|_| Error::new("service_unavailable", "Supervisor is not available; run rhyven daemon start (with the same --home/--workspace)"))?;
    s.set_read_timeout(Some(Duration::from_secs(if input["op"] == "ping" {
        2
    } else {
        650
    })))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    write_frame(&mut s, &input)?;
    let out = frame(&mut BufReader::new(s))?;
    if let Some(e) = out.get("error") {
        return Err(serde_json::from_value(e.clone())?);
    }
    Ok(out["result"].clone())
}
type Logs = Arc<Mutex<VecDeque<u8>>>;
struct Work {
    args: Value,
    actor: String,
    reply: SyncSender<Result<Value>>,
    deadline: Instant,
}
struct Entry {
    tx: SyncSender<Work>,
    thread: JoinHandle<()>,
    logs: Logs,
    memory: u64,
    cpus: u64,
}
struct Manager {
    base: PathBuf,
    workers: Mutex<BTreeMap<(PathBuf, String), Entry>>,
    roots: Mutex<Vec<PathBuf>>,
    stop: Arc<AtomicBool>,
    clients: AtomicUsize,
    max_services: usize,
    memory: u64,
    cpus: u64,
}
impl Manager {
    fn register(&self, r: &Runtime) -> Result<()> {
        ensure(
            base(&r.root)? == self.base,
            "permission",
            "Collection belongs to a different Rhyven home",
        )?;
        let mut roots = self.roots.lock().unwrap();
        if !roots.contains(&r.root) {
            roots.push(r.root.clone());
            store::write(&control_dir(&self.base)?.join("roots.json"), &json!(*roots))?;
        }
        Ok(())
    }
    fn worker(&self, r: &Runtime, app: &str) -> Result<SyncSender<Work>> {
        ensure(
            !self.stop.load(Ordering::SeqCst),
            "service_unavailable",
            "Supervisor is stopping",
        )?;
        let key = (r.root.clone(), app.into());
        let mut workers = self.workers.lock().unwrap();
        workers.retain(|_, w| !w.thread.is_finished());
        if let Some(w) = workers.get(&key) {
            return Ok(w.tx.clone());
        }
        let _gate = crate::maintenance::try_lock(&r.root, Duration::from_millis(500))?;
        let p = package(r, app)?;
        let s = state(&r.root, app)?;
        ensure(
            s["desired"] == "running" && s["suspended"] != true && s["state"] != "failed",
            "service_unavailable",
            "Service is stopped, suspended, or has exhausted retries; use service start",
        )?;
        ensure(
            s["host_retry_at"].as_u64().unwrap_or(0) <= crate::marketplace::now(),
            "service_unavailable",
            "Waiting to retry the container host; inspect service status and doctor",
        )?;
        let memory = p["execution"]["memory_mb"].as_u64().unwrap_or(512);
        let cpus = p["execution"]["cpus"].as_u64().unwrap_or(1);
        ensure(
            workers.len() < self.max_services
                && workers.values().map(|w| w.memory).sum::<u64>() + memory <= self.memory
                && workers.values().map(|w| w.cpus).sum::<u64>() + cpus <= self.cpus,
            "service_unavailable",
            "Supervisor resource budget exceeded; stop another service or adjust daemon limits",
        )?;
        let (tx, rx) = mpsc::sync_channel(8);
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let worker_logs = logs.clone();
        let runtime = r.clone();
        let app_name = app.to_string();
        let thread = thread::spawn(move || worker_loop(runtime, app_name, rx, worker_logs));
        workers.insert(
            key,
            Entry {
                tx: tx.clone(),
                thread,
                logs,
                memory,
                cpus,
            },
        );
        Ok(tx)
    }
    fn handle(&self, input: Value) -> Result<Value> {
        let op = input["op"].as_str().unwrap_or("");
        if op == "ping" {
            return Ok(
                json!({"running":true,"pid":std::process::id(),"home":self.base,"limits":{"services":self.max_services,"memory_mb":self.memory,"cpus":self.cpus}}),
            );
        }
        if op == "shutdown" {
            self.stop.store(true, Ordering::SeqCst);
            return Ok(json!({"stopping":true}));
        }
        ensure(
            !self.stop.load(Ordering::SeqCst),
            "service_unavailable",
            "Supervisor is stopping",
        )?;
        let root = PathBuf::from(
            input["root"]
                .as_str()
                .ok_or_else(|| Error::new("validation", "root required"))?,
        );
        ensure(
            root.is_absolute() && std::fs::canonicalize(&root)? == root,
            "permission",
            "Canonical collection root required",
        )?;
        let r = Runtime::new(&root, input["actor"].as_str().unwrap_or("agent"))?;
        self.register(&r)?;
        let app = input["app"].as_str().unwrap_or("");
        ensure(catalog::app_name(app), "validation", "Invalid app")?;
        if op == "logs" {
            let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(1))?;
            package(&r, app)?;
            let workers = self.workers.lock().unwrap();
            let data = workers
                .get(&(root, app.into()))
                .map(|w| w.logs.lock().unwrap().iter().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            return Ok(
                json!({"app":app,"text":String::from_utf8_lossy(&data),"limit_bytes":32768,"scope":"Current instance stderr tail; not a durable log archive"}),
            );
        }
        ensure(
            matches!(op, "start" | "restart" | "call"),
            "validation",
            "Unknown supervisor operation",
        )?;
        if op == "restart" {
            {
                let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(5))?;
                stop_locked(&r, app, false, None)?;
            }
            let until = Instant::now() + Duration::from_secs(5);
            while self
                .workers
                .lock()
                .unwrap()
                .get(&(r.root.clone(), app.into()))
                .is_some_and(|w| !w.thread.is_finished())
            {
                ensure(
                    Instant::now() < until,
                    "service_unavailable",
                    "Previous service is still shutting down; retry restart",
                )?;
                thread::sleep(Duration::from_millis(50));
            }
        }
        let p;
        {
            let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(5))?;
            p = package(&r, app)?;
            if op == "call" {
                ensure(
                    input["package_sha256"] == store::hash(&p),
                    "version_conflict",
                    "App changed; discover again",
                )?;
                if let Some(saved) = receipt(&r, &p, &input["arguments"])? {
                    return saved;
                }
            }
            let mut s = state(&r.root, app)?;
            // Holding the collection gate proves maintenance is no longer active.
            // An explicit start can recover a suspension left by an interrupted
            // maintenance process; ordinary calls must not clear it implicitly.
            if op != "call" && s["suspended"] == true {
                s["suspended"] = json!(false);
                s["suspension_reason"] = Value::Null;
                s["state"] = json!("stopped");
            }
            ensure(
                s["suspended"] != true,
                "service_unavailable",
                "Collection is under maintenance",
            )?;
            if op != "call"
                || (s["desired"] != "running"
                    && s["explicitly_stopped"] != true
                    && p["execution"]["start_policy"] == "on-demand")
            {
                s["desired"] = json!("running");
                s["explicitly_stopped"] = json!(false);
                if s["state"] != "ready" && s["state"] != "starting" {
                    s["state"] = json!("stopped");
                    s["restart_count"] = json!(0);
                    s["host_retry_at"] = Value::Null;
                    s["error"] = Value::Null;
                }
                save(&r.root, app, &s)?;
                audit(&r, app, "service_start")?;
            }
        }
        self.worker(&r, app)?;
        let until = Instant::now()
            + Duration::from_secs(
                p["execution"]["startup_timeout_seconds"]
                    .as_u64()
                    .unwrap_or(30)
                    + 65,
            );
        loop {
            let s = state(&r.root, app)?;
            if s["state"] == "ready" {
                break;
            }
            ensure(
                s["desired"] == "running"
                    && s["suspended"] != true
                    && !matches!(s["state"].as_str(), Some("failed" | "unavailable")),
                "service_unavailable",
                format!("Service not ready: {s}"),
            )?;
            ensure(
                Instant::now() < until,
                "service_timeout",
                "Service readiness timed out",
            )?;
            thread::sleep(Duration::from_millis(50));
        }
        if op != "call" {
            return state(&r.root, app);
        }
        let tx = self.worker(&r, app)?;
        let (reply, rx) = mpsc::sync_channel(1);
        tx.try_send(Work {
            args: input["arguments"].clone(),
            actor: r.actor,
            reply,
            deadline: Instant::now()
                + Duration::from_secs(p["execution"]["timeout_seconds"].as_u64().unwrap_or(30)),
        })
        .map_err(|_| {
            Error::new(
                "service_unavailable",
                "Service request queue is full or worker stopped",
            )
        })?;
        rx.recv_timeout(Duration::from_secs(330)).map_err(|_| Error::new("service_incomplete", "Service connection lost; side effects may have occurred. Retry only with the same request_id"))?
    }
    fn scan(&self) {
        let roots = self.roots.lock().unwrap().clone();
        for root in roots {
            if let Ok(r) = Runtime::new(root, "supervisor") {
                // Only shutdown suspensions resume automatically. Interrupted or
                // failed maintenance still requires an explicit operator start.
                if let Ok(_gate) = crate::maintenance::try_lock(&r.root, Duration::from_millis(100))
                {
                    if let Ok(list) = states(&r.root) {
                        for mut s in list {
                            if s["suspension_reason"] == "supervisor_shutdown" {
                                s["suspended"] = json!(false);
                                s["suspension_reason"] = Value::Null;
                                s["state"] = json!(if s["desired"] == "running" {
                                    "unavailable"
                                } else {
                                    "stopped"
                                });
                                let _ = save(&r.root, s["app"].as_str().unwrap(), &s);
                            }
                        }
                    }
                }
                if let Ok(list) = states(&r.root) {
                    for s in list {
                        if s["desired"] == "running"
                            && s["suspended"] != true
                            && s["state"] != "failed"
                        {
                            let _ = self.worker(&r, s["app"].as_str().unwrap());
                        }
                    }
                }
            }
        }
    }
}
pub(super) fn run(base: &Path, max_services: usize, memory: u64, cpus: u64) -> Result<()> {
    ensure(
        max_services > 0 && max_services <= 128 && memory > 0 && cpus > 0,
        "configuration",
        "Positive bounded supervisor limits required",
    )?;
    let dir = control_dir(base)?;
    let owner = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("owner.lock"))?;
    fs2::FileExt::try_lock_exclusive(&owner).map_err(|_| {
        Error::new(
            "service_unavailable",
            "Supervisor already running for this home",
        )
    })?;
    let socket = socket(base)?;
    if socket.exists() {
        std::fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let roots: Vec<PathBuf> = if dir.join("roots.json").exists() {
        serde_json::from_slice(&std::fs::read(dir.join("roots.json"))?)?
    } else {
        vec![]
    };
    for root in &roots {
        ensure(
            super::base(root)? == base,
            "integrity",
            "Supervisor root registry contains another home",
        )?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    // OS service managers send SIGTERM; use the same clean shutdown path as
    // daemon stop. Registration is scoped for embedded conformance supervisors.
    struct Signals(Vec<signal_hook::SigId>);
    impl Drop for Signals {
        fn drop(&mut self) {
            for id in self.0.drain(..) {
                signal_hook::low_level::unregister(id);
            }
        }
    }
    let mut signals = Signals(vec![]);
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signals
            .0
            .push(signal_hook::flag::register(signal, stop.clone())?);
    }
    let manager = Arc::new(Manager {
        base: base.into(),
        workers: Mutex::new(BTreeMap::new()),
        roots: Mutex::new(roots),
        stop,
        clients: AtomicUsize::new(0),
        max_services,
        memory,
        cpus,
    });
    let mut scan = Instant::now() - Duration::from_secs(2);
    while !manager.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if manager.clients.fetch_add(1, Ordering::SeqCst) >= 32 {
                    manager.clients.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let m = manager.clone();
                thread::spawn(move || {
                    let result = (|| -> Result<Value> {
                        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                        m.handle(frame(&mut BufReader::new(stream.try_clone()?))?)
                    })();
                    let result = match result {
                        Ok(v) => json!({"result":v}),
                        Err(e) => json!({"error":e}),
                    };
                    let _ = write_frame(&mut stream, &result);
                    m.clients.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25))
            }
            Err(e) => return Err(e.into()),
        }
        if scan.elapsed() >= Duration::from_secs(1) {
            manager.scan();
            scan = Instant::now();
        }
    }
    // Preserve desired state so a subsequent daemon start resumes enabled services.
    for root in manager.roots.lock().unwrap().clone() {
        let r = Runtime::new(root, "supervisor")?;
        let _gate = crate::maintenance::lock(&r.root)?;
        for s in states(&r.root)? {
            if s.get("generation").is_some()
                && s["suspended"] != true
                && s["state"] != "failed"
                && !(s["desired"] == "stopped" && s["state"] == "stopped")
            {
                // Docker may already be shutting down. Persist resumable intent
                // and continue cleanup even when its endpoint is unavailable.
                let _ = stop_locked(
                    &r,
                    s["app"].as_str().unwrap(),
                    false,
                    Some("supervisor_shutdown"),
                );
            }
        }
    }
    let until = Instant::now() + Duration::from_secs(5);
    while manager
        .workers
        .lock()
        .unwrap()
        .values()
        .any(|w| !w.thread.is_finished())
        && Instant::now() < until
    {
        thread::sleep(Duration::from_millis(50));
    }
    for root in manager.roots.lock().unwrap().clone() {
        let _gate = crate::maintenance::lock(&root)?;
        for mut s in states(&root)? {
            if s["suspension_reason"] == "supervisor_shutdown" {
                s["suspended"] = json!(false);
                s["suspension_reason"] = Value::Null;
                if s["state"] != "unavailable" {
                    s["state"] = json!("stopped");
                }
                save(&root, s["app"].as_str().unwrap(), &s)?;
            }
        }
    }
    drop(listener);
    std::fs::remove_file(socket)?;
    drop(owner);
    Ok(())
}

struct Engine {
    child: Child,
    tx: SyncSender<Value>,
    rx: Receiver<Result<Value>>,
    overflow: Arc<AtomicBool>,
    last_ping: Instant,
    last_pong: Instant,
    health: bool,
}
impl Engine {
    fn launch(r: &Runtime, p: &Value, generation: &str, logs: Logs) -> Result<Self> {
        let app = p["name"].as_str().unwrap();
        let name = name(&r.root, app)?;
        // Creation and actual start are synchronous under admission lock. An attach
        // process can never start a container after backup has observed it stopped.
        {
            let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(5))?;
            let s = state(&r.root, app)?;
            ensure(
                active(&s, generation),
                "service_unavailable",
                "Service start cancelled",
            )?;
            halt(&r.root, app, 2, false)?;
            let mut args = container::launch_command(&r.root, p, &name, true)?;
            args[0] = "create".into();
            container::docker(&args, vec![], 30)?;
            container::docker(&["start".into(), name.clone()], vec![], 30)?;
        }
        let mut child = Command::new("docker")
            .args(["attach", "--sig-proxy=false", &name])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let out = child.stdout.take().unwrap();
        let mut err = child.stderr.take().unwrap();
        let (tx, write_rx) = mpsc::sync_channel(16);
        let (read_tx, rx) = mpsc::sync_channel(64);
        let overflow = Arc::new(AtomicBool::new(false));
        let flag = overflow.clone();
        thread::spawn(move || {
            while let Ok(v) = write_rx.recv() {
                if write_frame(&mut stdin, &v).is_err() {
                    flag.store(true, Ordering::SeqCst);
                    break;
                }
            }
        });
        let flag = overflow.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(out);
            loop {
                let v = frame(&mut reader);
                let end = v.is_err();
                if read_tx.try_send(v).is_err() {
                    flag.store(true, Ordering::SeqCst);
                    break;
                }
                if end {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut log = logs.lock().unwrap();
                log.extend(&buf[..n]);
                while log.len() > 32768 {
                    log.pop_front();
                }
            }
        });
        let mut engine = Self {
            child,
            tx,
            rx,
            overflow,
            last_ping: Instant::now(),
            last_pong: Instant::now(),
            health: false,
        };
        engine.send(json!({"type":"initialize","protocol":"rhyven.service/1","context":{"category":app,"collection":collections::scope(&r.root)?["collection"],"generation":generation,"data_dir":"/data"}}))?;
        let deadline = Instant::now()
            + Duration::from_secs(
                p["execution"]["startup_timeout_seconds"]
                    .as_u64()
                    .unwrap_or(30),
            );
        loop {
            ensure(
                Instant::now() < deadline,
                "service_timeout",
                "Service readiness timed out",
            )?;
            if let Some(v) = engine.poll(r, p, generation)? {
                ensure(
                    v["type"] == "ready" && v["protocol"] == "rhyven.service/1",
                    "service_protocol",
                    "Service must acknowledge the protocol with ready",
                )?;
                break;
            }
        }
        engine.health = true;
        engine.last_pong = Instant::now();
        Ok(engine)
    }
    fn send(&self, v: Value) -> Result<()> {
        ensure(
            v.to_string().len() < MAX,
            "service_protocol",
            "Frame too large",
        )?;
        self.tx
            .try_send(v)
            .map_err(|_| Error::new("service_protocol", "Service input queue blocked or closed"))
    }
    fn poll(&mut self, r: &Runtime, p: &Value, generation: &str) -> Result<Option<Value>> {
        ensure(
            active(&state(&r.root, p["name"].as_str().unwrap())?, generation),
            "service_unavailable",
            "Service stopped or suspended",
        )?;
        ensure(
            !self.overflow.load(Ordering::SeqCst),
            "service_protocol",
            "Service stream overflow or write failure",
        )?;
        ensure(
            self.child.try_wait()?.is_none(),
            "service_unavailable",
            "Service stream/process exited",
        )?;
        if self.health {
            ensure(
                self.last_pong.elapsed() < Duration::from_secs(20),
                "service_timeout",
                "Service heartbeat timed out",
            )?;
            if self.last_ping.elapsed() >= Duration::from_secs(5) {
                self.send(json!({"type":"ping"}))?;
                self.last_ping = Instant::now();
            }
        }
        match self.rx.recv_timeout(Duration::from_millis(50)) {
            Ok(v) => {
                let v = v?;
                match v["type"].as_str() {
                    Some("callback") => {
                        ensure(
                            v["id"]
                                .as_str()
                                .is_some_and(|s| !s.is_empty() && s.len() <= 128),
                            "service_protocol",
                            "Callback id required",
                        )?;
                        let response = match callback(r, p, generation, &v) {
                            Ok(result) => {
                                json!({"type":"callback_result","id":v["id"],"result":result})
                            }
                            Err(e) => json!({"type":"callback_result","id":v["id"],"error":e}),
                        };
                        self.send(response)?;
                        Ok(None)
                    }
                    Some("pong") => {
                        self.last_pong = Instant::now();
                        Ok(None)
                    }
                    _ => Ok(Some(v)),
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(_) => Err(Error::new("service_unavailable", "Service stream closed")),
        }
    }
    fn execute(
        &mut self,
        r: &Runtime,
        p: &Value,
        generation: &str,
        args: &Value,
        until: Instant,
    ) -> Result<Value> {
        let action = args["action"].as_str().unwrap_or("");
        let input = schema::validate(args["args"].clone(), &p["actions"][action]["input"])?;
        {
            let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(1))?;
            ensure(
                active(&state(&r.root, p["name"].as_str().unwrap())?, generation),
                "service_unavailable",
                "Service fenced",
            )?;
            if let Some(saved) = receipt(r, p, args)? {
                return saved;
            }
            if let Some(id) = request_id(args)? {
                store::open(&r.root)?.execute(
                    "INSERT INTO receipts(actor,request,fingerprint,result) VALUES(?1,?2,?3,?4)",
                    params![
                        r.actor,
                        id,
                        fingerprint(p, args),
                        json!({"status":"pending"}).to_string()
                    ],
                )?;
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.send(json!({"type":"call","id":id,"category":p["name"],"function":format!("action_{action}"),"args":input,"context":{"actor":r.actor,"request_id":args.get("request_id"),"generation":generation,"collection":collections::scope(&r.root)?["collection"]}}))?;
        let result = loop {
            ensure(
                Instant::now() < until,
                "service_timeout",
                "Service action timed out; its outcome may be unknown",
            )?;
            if let Some(v) = self.poll(r, p, generation)? {
                catalog::keys(&v, &["type", "id", "result", "error"])?;
                ensure(
                    v["type"] == "response"
                        && v["id"] == id
                        && v.get("result").is_some() != v.get("error").is_some(),
                    "service_protocol",
                    "Unexpected service response",
                )?;
                if let Some(e) = v.get("error") {
                    ensure(
                        e["code"].is_string() && e["message"].is_string(),
                        "service_protocol",
                        "App errors need code and message",
                    )?;
                    break Err(Error::new("app_error", e.to_string()));
                }
                break schema::validate(v["result"].clone(), &p["actions"][action]["output"]);
            }
        };
        if result.as_ref().is_err_and(|e| e.code != "app_error") {
            return result;
        }
        let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(1))?;
        ensure(
            active(&state(&r.root, p["name"].as_str().unwrap())?, generation),
            "service_incomplete",
            "Service stopped before recording result",
        )?;
        let mut db = store::open(&r.root)?;
        let tx = db.transaction()?;
        if let Some(id) = request_id(args)? {
            let saved = match &result {
                Ok(v) => json!({"status":"complete","result":v}),
                Err(e) => json!({"status":"error","error":e}),
            };
            tx.execute(
                "UPDATE receipts SET result=?3 WHERE actor=?1 AND request=?2",
                params![r.actor, id, saved.to_string()],
            )?;
        }
        tx.execute("INSERT INTO events(app,event) VALUES(?1,?2)", params![p["name"].as_str().unwrap(),json!({"operation":"service_action","actor":r.actor,"action":action,"request_id":args.get("request_id"),"generation":generation,"time":crate::marketplace::now()}).to_string()])?;
        tx.commit()?;
        result
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn active(s: &Value, generation: &str) -> bool {
    s["generation"] == generation && s["desired"] == "running" && s["suspended"] != true
}
fn request_id(args: &Value) -> Result<Option<&str>> {
    args.get("request_id")
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or_else(|| Error::new("validation", "Invalid request_id"))
        })
        .transpose()
}
fn fingerprint(p: &Value, args: &Value) -> String {
    store::hash(&json!(["service", args, store::hash(p)]))
}
fn receipt(r: &Runtime, p: &Value, args: &Value) -> Result<Option<Result<Value>>> {
    let Some(id) = request_id(args)? else {
        return Ok(None);
    };
    let saved = store::open(&r.root)?
        .query_row(
            "SELECT fingerprint,result FROM receipts WHERE actor=?1 AND request=?2",
            params![r.actor, id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((f, raw)) = saved {
        ensure(
            f == fingerprint(p, args),
            "idempotency_conflict",
            "request_id already used for different arguments/package",
        )?;
        let value: Value = serde_json::from_str(&raw)?;
        return Ok(Some(match value["status"].as_str() {
            Some("complete") => Ok(value["result"].clone()),
            Some("error") => Err(serde_json::from_value(value["error"].clone())?),
            _ => Err(Error::new("service_incomplete", "Previous call may have caused side effects; reconcile its state before using a new request ID")),
        }));
    }
    Ok(None)
}
fn worker_loop(r: Runtime, app: String, rx: Receiver<Work>, logs: Logs) {
    loop {
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let setup = (|| -> Result<Value> {
            let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(2))?;
            let mut s = state(&r.root, &app)?;
            ensure(
                s["desired"] == "running" && s["suspended"] != true && s["state"] != "failed",
                "service_unavailable",
                "Service disabled",
            )?;
            let p = package(&r, &app)?;
            s["generation"] = json!(generation);
            s["state"] = json!("starting");
            s["shutdown_timeout_seconds"] = p["execution"]
                .get("shutdown_timeout_seconds")
                .cloned()
                .unwrap_or(json!(10));
            s["package_sha256"] = json!(store::hash(&p));
            s["version"] = p["version"].clone();
            save(&r.root, &app, &s)?;
            Ok(p)
        })();
        let Ok(p) = setup else {
            return;
        };
        let result = (|| -> Result<()> {
            let mut engine = Engine::launch(&r, &p, &generation, logs.clone())?;
            {
                let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(2))?;
                let mut s = state(&r.root, &app)?;
                ensure(
                    active(&s, &generation),
                    "service_unavailable",
                    "Service start cancelled",
                )?;
                s["state"] = json!("ready");
                s["host_retry_at"] = Value::Null;
                s["error"] = Value::Null;
                save(&r.root, &app, &s)?;
                audit(&r, &app, "service_ready")?;
            }
            loop {
                match rx.try_recv() {
                    Ok(work) => {
                        if Instant::now() >= work.deadline {
                            let _ = work.reply.send(Err(Error::new(
                                "service_timeout",
                                "Request expired in the queue before execution",
                            )));
                            continue;
                        }
                        let caller = Runtime {
                            root: r.root.clone(),
                            actor: work.actor,
                        };
                        let result =
                            engine.execute(&caller, &p, &generation, &work.args, work.deadline);
                        let fatal = result
                            .as_ref()
                            .err()
                            .filter(|e| {
                                matches!(
                                    e.code.as_str(),
                                    "service_timeout" | "service_protocol" | "service_unavailable"
                                )
                            })
                            .map(|e| Error::new(&e.code, &e.message));
                        let _ = work.reply.send(result);
                        if let Some(e) = fatal {
                            return Err(e);
                        }
                    }
                    Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                    Err(mpsc::TryRecvError::Empty) => (),
                }
                ensure(
                    engine.poll(&r, &p, &generation)?.is_none(),
                    "service_protocol",
                    "Unsolicited service response",
                )?;
            }
        })();
        let count = (|| -> Result<u64> {
            let _gate = crate::maintenance::try_lock(&r.root, Duration::from_secs(3))?;
            let mut s = state(&r.root, &app)?;
            ensure(
                active(&s, &generation),
                "service_unavailable",
                "Service stopped",
            )?;
            // Never create a replacement before the old writer is confirmed stopped.
            if let Err(e) = halt(&r.root, &app, 2, false) {
                s["state"] = json!("unavailable");
                s["host_retry_at"] = json!(crate::marketplace::now() + 5);
                s["error"] = json!(e.to_string());
                save(&r.root, &app, &s)?;
                return Err(e);
            }
            if let Err(e) = &result {
                if e.code == "container_unavailable" {
                    s["state"] = json!("unavailable");
                    s["host_retry_at"] = json!(crate::marketplace::now() + 5);
                    s["error"] = json!(e.to_string());
                    save(&r.root, &app, &s)?;
                    return Err(Error::new(&e.code, &e.message));
                }
            }
            let count = s["restart_count"].as_u64().unwrap_or(0) + 1;
            let exhausted = count > p["execution"]["restart_limit"].as_u64().unwrap_or(3);
            s["restart_count"] = json!(count);
            s["state"] = json!(if exhausted { "failed" } else { "restarting" });
            s["error"] = json!(result
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Service stopped".into()));
            save(&r.root, &app, &s)?;
            audit(&r, &app, "service_failure")?;
            ensure(
                !exhausted,
                "service_unavailable",
                "Restart budget exhausted",
            )?;
            Ok(count)
        })();
        let Ok(count) = count else {
            return;
        };
        let until = Instant::now() + Duration::from_secs((1 << count.min(5)).min(30));
        while Instant::now() < until {
            if !state(&r.root, &app).is_ok_and(|s| active(&s, &generation)) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
