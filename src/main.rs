//! clearance: the local control tower for Claude Code sessions (BRIEF, ADR-0010).
//!   clearance                        serves the page and the hooks on 127.0.0.1
//!   clearance install [--dry-run] [--remove] [--settings <file>]
//!                                    adds clearance's hooks to the user's settings (ADR-0009)
//! CLEARANCE_PORT (47800) and CLEARANCE_HOME (~/.clearance) move a test server out of the live one's way;
//! CLEARANCE_LOG=1 keeps every hook payload, to study a new event's shape.

mod board;
mod claude;
mod http;
mod install;
mod json;
#[cfg(test)]
mod page;
mod risk;
mod sys;
mod terminal;

use board::{Board, Kind};
use json::{Json, obj};
use std::collections::HashMap;
use std::io::{self, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Duration;
use std::{fs, thread};

const HOST: &str = "127.0.0.1";
const PAGE: &str = include_str!("../public/index.html");
// While the repository the binary was built from is there, the page is read from it (no rebuild to see
// an edit) and the companion mod is loaded from it.
const REPO: &str = env!("CARGO_MANIFEST_DIR");
const PAGE_HEADERS: [(&str, &str); 3] = [
    ("content-type", "text/html; charset=utf-8"),
    ("cache-control", "no-store"),
    (
        "content-security-policy",
        "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; frame-ancestors 'none'",
    ),
];

struct App {
    port: u16,
    token: String,
    home: PathBuf, // ~/.clearance: token, hooks.json, endpoint.json, journal
    projects: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    board: Board,
    held: HashMap<String, mpsc::Sender<Json>>, // request id -> the hook call waiting for an answer
    clients: Vec<TcpStream>,                   // open pages (server-sent events)
    last: String,
    first_prompts: HashMap<String, String>, // session name -> instruction for a session started from the page
    groups: Json,                           // folder -> group, where the user moved it (~/.clearance/groups.json)
    lang: String,                           // the page's language, for the line under the prompt
}

impl State {
    /// What the page gets: the board, the user's groups and language.
    fn snapshot(&self) -> Json {
        let mut snap = self.board.snapshot();
        snap.set("groups", self.groups.clone());
        snap.set("lang", self.lang.as_str().into());
        snap
    }

    fn publish(&mut self) {
        let data = self.snapshot().to_string();
        if data == self.last {
            return;
        }
        let msg = format!("data: {data}\n\n");
        self.clients.retain(|c| {
            let mut w: &TcpStream = c;
            w.write_all(msg.as_bytes()).is_ok()
        });
        self.last = data;
    }

    /// Answers a held hook call and takes the request off the board. `{}` means no decision: Claude
    /// Code carries on with its own dialog.
    fn finish(&mut self, id: &str, out: Json) {
        self.board.end(id);
        if let Some(tx) = self.held.remove(id) {
            let _ = tx.send(out);
        }
    }
}

impl App {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn base(&self) -> String {
        format!("http://{HOST}:{}", self.port)
    }

    /// Append-only journal of what was decided from the page (BRIEF section 6).
    fn journal(&self, entry: Json) {
        let mut line = obj([("at", sys::iso(sys::now_ms()).into())]);
        for (k, v) in entry.entries() {
            line.set(k, v.clone());
        }
        append(&self.home.join("journal.jsonl"), &line);
    }
}

/// A JSON file of ours; an empty object when missing or broken.
fn read_json(file: &Path) -> Json {
    fs::read_to_string(file).ok().and_then(|t| Json::parse(&t).ok()).filter(|j| matches!(j, Json::Obj(_))).unwrap_or(Json::Obj(Vec::new()))
}

fn append(file: &Path, line: &Json) {
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{line}");
    }
}

/// A file only its owner reads: the token is in it.
fn write_private(file: &Path, text: &str) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(file)?.write_all(text.as_bytes())
}

/// The token survives restarts: running sessions keep the hooks they started with.
fn token(home: &Path) -> Result<String, String> {
    fs::create_dir_all(home).map_err(|e| format!("{}: {e}", home.display()))?;
    let file = home.join("token");
    if let Ok(t) = fs::read_to_string(&file) {
        return Ok(t.trim().to_string());
    }
    let token = sys::random_hex(24).map_err(|e| e.to_string())?;
    write_private(&file, &token).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(token)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = std::env::var("CLEARANCE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47800);
    let home = std::env::var_os("CLEARANCE_HOME").map_or_else(|| sys::home().join(".clearance"), PathBuf::from);
    let token = token(&home).unwrap_or_else(|e| fail(&e));
    let app = App { port, token, home, projects: claude::projects(), state: Mutex::default() };
    match args.first().map(String::as_str) {
        None => serve(Arc::new(app)),
        Some("install") => install::run(&args[1..], &app.base(), &app.token).unwrap_or_else(|e| fail(&e)),
        Some("-h" | "--help" | "help") => println!("clearance                      serve the page and the hooks\nclearance install [--dry-run] [--remove] [--settings <file>]"),
        Some(other) => fail(&format!("unknown command: {other} (clearance --help)")),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("clearance: {message}");
    std::process::exit(1)
}

fn serve(app: Arc<App>) {
    let hooks = obj([("hooks", install::hooks(&app.base(), &app.token))]);
    let endpoint = obj([("url", app.base().into()), ("token", app.token.as_str().into())]);
    // The companion mod reads where clearance is from ~/.clearance/endpoint.json, wherever it is installed.
    for (name, text) in [("hooks.json", hooks.pretty()), ("endpoint.json", endpoint.to_string())] {
        write_private(&app.home.join(name), &text).unwrap_or_else(|e| fail(&format!("{name}: {e}")));
    }
    {
        let mut st = app.lock();
        st.groups = read_json(&app.home.join("groups.json"));
        st.lang = read_json(&app.home.join("prefs.json")).get("lang").str().unwrap_or("").to_string();
    }
    let listener = TcpListener::bind((HOST, app.port)).unwrap_or_else(|e| fail(&format!("{}: {e}", app.base())));
    println!("clearance on {}/?token={}", app.base(), app.token);
    println!("hooks for a session: claude --settings {}", app.home.join("hooks.json").display());
    let poller = app.clone();
    thread::spawn(move || poll(&poller));
    // ponytail: the first scan of every transcript takes a moment; done at start rather than on the first
    // journal view. Upgrade path: a cache on disk if it grows.
    let warm = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(3));
        claude::approvals(&warm.projects);
    });
    for stream in listener.incoming().flatten() {
        let app = app.clone();
        thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
            if let Ok(req) = http::Request::read(&stream) {
                let _ = stream.set_read_timeout(None);
                let _ = route(&app, &stream, &req);
            }
        });
    }
}

/// `claude agents` every 2 s while a page is open, every 10 s otherwise (ADR-0010).
fn poll(app: &App) {
    loop {
        let list = claude::agents().map(|l| l.into_iter().map(|s| with_transcript(&app.projects, s)).collect::<Vec<_>>());
        let open = {
            let mut st = app.lock();
            if let Some(list) = list {
                for id in st.board.sync(list, sys::now_ms()) {
                    st.finish(&id, Json::Obj(Vec::new()));
                }
                // A session started from the page with an instruction gets it once it shows up.
                let arrived: Vec<(String, String)> = st
                    .board
                    .sessions
                    .iter()
                    .filter_map(|s| Some((s.get("name").str()?.to_string(), s.get("sessionId").str()?.to_string())))
                    .filter(|(name, _)| st.first_prompts.contains_key(name))
                    .collect();
                for (name, id) in arrived {
                    if let Some(text) = st.first_prompts.remove(&name) {
                        st.board.instruct(&id, &text, sys::now_ms());
                    }
                }
            }
            st.publish();
            !st.clients.is_empty()
        };
        thread::sleep(Duration::from_secs(if open { 2 } else { 10 }));
    }
}

/// The model of each session gives its avatar's accessory; its transcript's last write, how long it has
/// been idle; its last message, the text of a done card; the end of its transcript, what the last turn
/// left to check and whether it is looping or stalled.
fn with_transcript(projects: &Path, mut s: Json) -> Json {
    let id = s.get("sessionId").str().unwrap_or("").to_string();
    let cwd = s.get("cwd").str().map(String::from);
    if let Some(file) = claude::transcript_of(projects, &id, cwd.as_deref()) {
        s.set("model", claude::model_of(&file).into());
        if let Ok(modified) = fs::metadata(&file).and_then(|m| m.modified()) {
            let ms = modified.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            s.set("lastActive", ms.into());
        }
        s.set("lastText", claude::last_text_of(&file).into());
        let t = claude::read_tail(&file);
        s.set("title", t.title.as_deref().into());
        s.set("branch", t.branch.as_deref().into());
        s.set("facts", claude::facts(&t));
        let now = sys::now_ms();
        let quiet = s.get("lastActive").num().map(|ms| now.saturating_sub(ms as u64));
        if let Some((tool, target, count)) = claude::recent_loop(&t, now) {
            s.set("watch", obj([("kind", "loop".into()), ("tool", tool.into()), ("target", target.into()), ("count", count.into())]));
        } else if s.get("status").str() == Some("busy") && quiet.is_some_and(|q| q > STALL_MS) {
            s.set("watch", obj([("kind", "stall".into()), ("minutes", quiet.map(|q| q / 60_000).into())]));
        }
    }
    s
}

// A busy session that has not written to its transcript for this long is shown as stalled.
const STALL_MS: u64 = 10 * 60_000;

fn error(s: &TcpStream, status: u16, message: &str) -> io::Result<()> {
    http::json(s, status, &obj([("error", message.into())]))
}

fn ok(s: &TcpStream) -> io::Result<()> {
    http::json(s, 200, &obj([("ok", true.into())]))
}

fn route(app: &App, s: &TcpStream, req: &http::Request) -> io::Result<()> {
    // Only our own host name: a page on another site cannot reach us through DNS rebinding.
    let host = req.header("host").unwrap_or("");
    if host != format!("{HOST}:{}", app.port) && host != format!("localhost:{}", app.port) {
        return http::respond(s, 421, &[], b"");
    }
    let bearer = req.header("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    let path = req.path.as_str();

    if req.method == "POST" && (path.starts_with("/hook/") || path == "/mod/sync") {
        if !sys::same(bearer, &app.token) {
            return http::respond(s, 401, &[], b"");
        }
        return match path.strip_prefix("/hook/") {
            Some(event) => hook(app, s, event, &req.json()),
            None => mod_sync(app, s, &req.json()),
        };
    }

    if req.method == "GET" && path == "/" {
        if let Some(t) = req.param("token") {
            if !sys::same(t, &app.token) {
                return http::respond(s, 401, &[], b"Wrong token.");
            }
            let cookie = format!("clearance={}; HttpOnly; SameSite=Strict; Path=/", app.token);
            return http::respond(s, 303, &[("location", "/"), ("set-cookie", &cookie)], b"");
        }
        if !req.cookie("clearance").is_some_and(|c| sys::same(c, &app.token)) {
            return http::respond(s, 401, &[("content-type", "text/plain")], b"Open the address clearance printed at start.");
        }
        let page = fs::read_to_string(Path::new(REPO).join("public").join("index.html")).unwrap_or_else(|_| PAGE.to_string());
        return http::respond(s, 200, &PAGE_HEADERS, page.as_bytes());
    }

    if !req.cookie("clearance").is_some_and(|c| sys::same(c, &app.token)) {
        return http::respond(s, 401, &[], b"");
    }

    if req.method == "GET" {
        if path == "/events" {
            http::events(s)?;
            let mut st = app.lock();
            let mut w = s;
            write!(w, "data: {}\n\n", st.snapshot())?;
            let _ = s.set_write_timeout(Some(Duration::from_secs(2))); // a stuck page must not hold the board
            st.clients.push(s.try_clone()?);
            return Ok(());
        }
        if let Some(id) = path.strip_prefix("/api/session/") {
            let cwd = app.lock().board.session(id).and_then(|s| s.get("cwd").str().map(String::from));
            return match claude::detail(&app.projects, id, cwd.as_deref()) {
                Some(d) => http::json(s, 200, &d),
                None => error(s, 404, "No transcript for this session."),
            };
        }
        if path == "/api/folders" {
            return folders(s, req.param("path").filter(|p| !p.is_empty()));
        }
        if path == "/api/journal" {
            let rules: Vec<Json> = risk::suggest(&claude::approvals(&app.projects), 3).iter().map(risk::Suggestion::to_json).collect();
            return http::json(s, 200, &obj([("decisions", Json::Arr(journal_tail(&app.home.join("journal.jsonl"), 200))), ("rules", Json::Arr(rules))]));
        }
    }

    // Every write from the page: same origin only.
    if req.method != "POST" || req.header("origin") != Some(format!("http://{host}").as_str()) {
        return http::respond(s, 403, &[], b"");
    }
    let body = req.json();
    match path {
        "/api/decide" => decide(app, s, &body),
        "/api/start" => start(app, s, &body),
        "/api/focus" => {
            let titles: Vec<String> = {
                let st = app.lock();
                let session = st.board.session(body.get("session").str().unwrap_or(""));
                let title = session
                    .and_then(|x| claude::transcript_of(&app.projects, x.get("sessionId").str()?, x.get("cwd").str()))
                    .and_then(|f| claude::read_transcript(&f).title.clone());
                title.into_iter().chain(session.and_then(|x| x.get("name").str().map(String::from))).collect()
            };
            if titles.is_empty() {
                return error(s, 404, "This session is not running.");
            }
            let found = terminal::focus(&titles);
            http::json(s, 200, &obj([("found", found.into()), ("titles", Json::Arr(titles.into_iter().map(Json::from).collect()))]))
        }
        // An instruction for a session, handed to its companion mod on its next sync.
        "/api/instruct" => {
            let (session, text) = (body.get("session").str().unwrap_or(""), body.get("text").str().unwrap_or("").trim());
            if session.is_empty() || text.is_empty() {
                return error(s, 400, "An instruction needs a session and some text.");
            }
            app.lock().board.instruct(session, text, sys::now_ms());
            app.journal(obj([("kind", "instruction".into()), ("session", session.into()), ("text", text.chars().take(500).collect::<String>().into())]));
            app.lock().publish();
            ok(s)
        }
        // Moves a folder's sessions to another group; an empty name puts them back in the folder's own.
        "/api/group" => {
            let (cwd, group) = (body.get("cwd").str().unwrap_or(""), body.get("group").str().unwrap_or("").trim());
            if cwd.is_empty() || group.chars().count() > 40 {
                return error(s, 400, "A group needs a folder and a name of 40 characters at most.");
            }
            let mut st = app.lock();
            if group.is_empty() {
                st.groups.remove(cwd);
            } else {
                st.groups.set(cwd, group.into());
            }
            fs::write(app.home.join("groups.json"), st.groups.pretty())?;
            st.publish();
            ok(s)
        }
        "/api/prefs" => {
            let lang = body.get("lang").str().unwrap_or("");
            if !["en", "fr"].contains(&lang) {
                return error(s, 400, "Unknown language.");
            }
            fs::write(app.home.join("prefs.json"), obj([("lang", lang.into())]).pretty())?;
            let mut st = app.lock();
            st.lang = lang.to_string();
            st.publish();
            ok(s)
        }
        "/api/read" => {
            let mut st = app.lock();
            st.board.read(body.get("session").str().unwrap_or(""));
            st.publish();
            ok(s)
        }
        _ => http::respond(s, 404, &[], b""),
    }
}

/// A hook call: answered at once, or held until the page decides, Claude Code gives up (timeout), or the
/// human answers in the terminal and the call is closed.
fn hook(app: &App, s: &TcpStream, event: &str, body: &Json) -> io::Result<()> {
    if std::env::var_os("CLEARANCE_LOG").is_some() {
        append(&app.home.join("hooks.jsonl"), &obj([("at", sys::iso(sys::now_ms()).into()), ("event", event.into()), ("body", body.clone())]));
    }
    let held = {
        let mut st = app.lock();
        st.board.environments = read_json(&app.home.join("environments.json"));
        let (request, ended) = st.board.hook(event, body, sys::now_ms());
        for id in ended {
            st.finish(&id, Json::Obj(Vec::new()));
        }
        let held = match request {
            // Nobody to decide on a retry: an auto mode denial is not held, the session goes on at once.
            Some(r) if r.kind == Kind::Denied && st.clients.is_empty() => {
                st.board.end(&r.id);
                None
            }
            Some(r) => {
                let (tx, rx) = mpsc::channel();
                st.held.insert(r.id.clone(), tx);
                Some((r.id, rx))
            }
            None => None,
        };
        st.publish();
        held
    };
    let Some((id, rx)) = held else { return http::json(s, 200, &Json::Obj(Vec::new())) };
    // ponytail: the closed call is noticed once a second, so a request answered in the terminal leaves
    // the board up to a second late.
    s.set_nonblocking(true)?;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(out) => {
                s.set_nonblocking(false)?;
                return http::json(s, 200, &out);
            }
            Err(mpsc::RecvTimeoutError::Timeout) if !closed(s) => {}
            Err(_) => {
                let mut st = app.lock();
                if st.held.remove(&id).is_some() {
                    st.board.settle(&id, sys::now_ms());
                    st.publish();
                }
                return Ok(());
            }
        }
    }
}

fn closed(s: &TcpStream) -> bool {
    match s.peek(&mut [0u8; 1]) {
        Ok(n) => n == 0,
        Err(e) => e.kind() != io::ErrorKind::WouldBlock,
    }
}

/// The companion mod, every 5 s: its session's usage in, what waits and an instruction out.
fn mod_sync(app: &App, s: &TcpStream, body: &Json) -> io::Result<()> {
    let session = body.get("session").str().unwrap_or("");
    if session.is_empty() {
        return error(s, 400, "session required");
    }
    let (instruction, waiting, here, lang) = {
        let mut st = app.lock();
        st.board.report(session, body.get("usage"), sys::now_ms());
        let instruction = st.board.take(session, sys::now_ms());
        st.publish();
        (instruction, st.board.requests.len(), st.board.requests.iter().filter(|r| r.session == session).count(), st.lang.clone())
    };
    if instruction.is_some() {
        app.journal(obj([("kind", "instruction-delivered".into()), ("session", session.into())]));
    }
    let line = status_line(&lang, waiting, here);
    http::json(s, 200, &obj([("waiting", waiting.into()), ("here", here.into()), ("instruction", instruction.into()), ("line", line.into())]))
}

/// The line the companion mod shows under the prompt, in the page's language.
fn status_line(lang: &str, waiting: usize, here: usize) -> String {
    match (lang, waiting, here) {
        ("fr", 0, _) => "clearance · rien ne t'attend".into(),
        ("fr", w, 0) => format!("clearance · {w} t'attend{}", if w > 1 { "ent" } else { "" }),
        ("fr", w, h) => format!("clearance · {w} t'attend{}, dont {h} ici", if w > 1 { "ent" } else { "" }),
        (_, 0, _) => "clearance · nothing waiting".into(),
        (_, w, 0) => format!("clearance · {w} waiting for you"),
        (_, w, h) => format!("clearance · {w} waiting for you, {h} here"),
    }
}

fn decide(app: &App, s: &TcpStream, decision: &Json) -> io::Result<()> {
    let id = decision.get("id").str().unwrap_or("");
    let mut st = app.lock();
    let Some(request) = st.board.request(id).cloned() else {
        return error(s, 410, "Already answered, or the session moved on.");
    };
    let out = match board::respond(&request, decision) {
        Ok(out) => out,
        Err(e) => return error(s, 400, &e),
    };
    st.finish(&request.id, out);
    st.publish();
    drop(st);
    app.journal(obj([
        ("kind", "decision".into()),
        ("session", request.session.as_str().into()),
        ("cwd", request.cwd.as_deref().into()),
        ("request", request.kind.as_str().into()),
        ("tool", request.tool.as_str().into()),
        ("target", claude::target(&request.input).into()),
        ("risk", request.risk.as_str().into()),
        ("action", decision.get("action").clone()),
        ("mode", decision.get("mode").clone()),
    ]));
    ok(s)
}

/// A new session in a real terminal tab. It gets clearance's hooks and companion mod from the user's
/// settings when they are installed, from here otherwise (passing them twice would ask twice).
fn start(app: &App, s: &TcpStream, body: &Json) -> io::Result<()> {
    let start = terminal::Start::from_json(body);
    if let Err(e) = start.check() {
        return error(s, 400, e);
    }
    let mut extra = Vec::new();
    let connect = body.get("connect").bool() != Some(false);
    if connect && !install::read_settings(&install::user_settings()).is_ok_and(|x| install::installed(&x)) {
        extra.extend(["--settings".to_string(), app.home.join("hooks.json").display().to_string()]);
        let mod_dir = Path::new(REPO).join("mod");
        if mod_dir.is_dir() {
            extra.extend(["--plugin-dir".to_string(), mod_dir.display().to_string()]);
        }
    }
    let result = terminal::start(&start, &extra);
    let prompt = body.get("prompt").str().map(str::trim).filter(|p| !p.is_empty());
    if let (Ok(()), Some(p)) = (&result, prompt) {
        app.lock().first_prompts.insert(start.name.clone(), p.to_string());
    }
    let err = result.as_ref().err().map(String::as_str);
    app.journal(obj([
        ("kind", "start".into()),
        ("name", start.name.as_str().into()),
        ("cwd", start.cwd.as_str().into()),
        ("model", start.model.as_str().into()),
        ("mode", start.mode.as_str().into()),
        ("error", err.into()),
    ]));
    match err {
        Some(e) => error(s, 500, e),
        None => ok(s),
    }
}

/// The folder picker of the start form: a browser never hands a page a folder's full path.
fn folders(s: &TcpStream, path: Option<&str>) -> io::Result<()> {
    let path = std::path::absolute(path.map_or_else(sys::home, PathBuf::from)).unwrap_or_default();
    let mut names: Vec<String> = match fs::read_dir(&path) {
        Ok(d) => d.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).filter_map(|e| e.file_name().into_string().ok()).filter(|n| !n.starts_with(['.', '$'])).collect(),
        Err(e) => return error(s, 400, &format!("Cannot list {}: {e}", path.display())),
    };
    names.sort_by_key(|n| n.to_lowercase());
    let drives: Vec<Json> = if cfg!(windows) { ('C'..='Z').map(|l| format!("{l}:\\")).filter(|d| Path::new(d).exists()).map(Json::from).collect() } else { vec!["/".into()] };
    http::json(
        s,
        200,
        &obj([
            ("path", path.display().to_string().into()),
            ("parent", path.parent().map(|p| p.display().to_string()).into()),
            ("folders", Json::Arr(names.into_iter().map(Json::from).collect())),
            ("drives", Json::Arr(drives)),
        ]),
    )
}

/// The last `n` journal lines, newest first.
fn journal_tail(file: &Path, n: usize) -> Vec<Json> {
    let text = fs::read_to_string(file).unwrap_or_default();
    text.lines().rev().take(n).filter_map(|l| Json::parse(l).ok()).collect()
}
