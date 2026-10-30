//! clearance: the local control tower for Claude Code sessions (BRIEF, ADR-0010).
//!   clearance                        serves the page and the hooks on 127.0.0.1, without a window
//!   clearance --foreground           the same, in the terminal it was started from
//!   clearance open                   opens the page, signed in, from the running clearance (ADR-0013)
//!   clearance install [--dry-run] [--remove] [--settings <file>]
//!                                    adds clearance's hooks to the user's settings (ADR-0009)
//! CLEARANCE_PORT (47800) and CLEARANCE_HOME (~/.clearance) move a test server out of the live one's way;
//! CLEARANCE_LOG=1 keeps every hook payload, to study a new event's shape.

mod auth;
mod board;
mod claude;
mod fleet;
mod http;
mod install;
mod json;
#[cfg(test)]
mod page;
mod pty;
mod risk;
mod sys;
mod terminal;
mod terms;

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
// The page's own script and styles only, marked with a nonce drawn for each load (ADR-0014).
const NONCE: &str = "__NONCE__";

// The page's fonts and their licences (ADR-0014), in the binary for a copy run without the repository.
const FONTS: [(&str, &[u8]); 25] = [
    ("archivo-800.woff2", include_bytes!("../public/fonts/archivo-800.woff2")),
    ("archivo-OFL.txt", include_bytes!("../public/fonts/archivo-OFL.txt")),
    ("archivoblack-400.woff2", include_bytes!("../public/fonts/archivoblack-400.woff2")),
    ("archivoblack-OFL.txt", include_bytes!("../public/fonts/archivoblack-OFL.txt")),
    ("atkinsonhyperlegiblemono-400-800.woff2", include_bytes!("../public/fonts/atkinsonhyperlegiblemono-400-800.woff2")),
    ("atkinsonhyperlegiblemono-OFL.txt", include_bytes!("../public/fonts/atkinsonhyperlegiblemono-OFL.txt")),
    ("atkinsonhyperlegiblenext-400-800.woff2", include_bytes!("../public/fonts/atkinsonhyperlegiblenext-400-800.woff2")),
    ("atkinsonhyperlegiblenext-OFL.txt", include_bytes!("../public/fonts/atkinsonhyperlegiblenext-OFL.txt")),
    ("delagothicone-400.woff2", include_bytes!("../public/fonts/delagothicone-400.woff2")),
    ("delagothicone-OFL.txt", include_bytes!("../public/fonts/delagothicone-OFL.txt")),
    ("dmserifdisplay-400.woff2", include_bytes!("../public/fonts/dmserifdisplay-400.woff2")),
    ("dmserifdisplay-OFL.txt", include_bytes!("../public/fonts/dmserifdisplay-OFL.txt")),
    ("fraunces-600-800.woff2", include_bytes!("../public/fonts/fraunces-600-800.woff2")),
    ("fraunces-OFL.txt", include_bytes!("../public/fonts/fraunces-OFL.txt")),
    ("fredoka-600.woff2", include_bytes!("../public/fonts/fredoka-600.woff2")),
    ("fredoka-OFL.txt", include_bytes!("../public/fonts/fredoka-OFL.txt")),
    ("ibmplexmono-400.woff2", include_bytes!("../public/fonts/ibmplexmono-400.woff2")),
    ("ibmplexmono-700.woff2", include_bytes!("../public/fonts/ibmplexmono-700.woff2")),
    ("ibmplexmono-OFL.txt", include_bytes!("../public/fonts/ibmplexmono-OFL.txt")),
    ("silkscreen-400.woff2", include_bytes!("../public/fonts/silkscreen-400.woff2")),
    ("silkscreen-OFL.txt", include_bytes!("../public/fonts/silkscreen-OFL.txt")),
    ("spacegrotesk-700.woff2", include_bytes!("../public/fonts/spacegrotesk-700.woff2")),
    ("spacegrotesk-OFL.txt", include_bytes!("../public/fonts/spacegrotesk-OFL.txt")),
    ("vt323-400.woff2", include_bytes!("../public/fonts/vt323-400.woff2")),
    ("vt323-OFL.txt", include_bytes!("../public/fonts/vt323-OFL.txt")),
];

fn csp(nonce: &str) -> String {
    format!("default-src 'self'; script-src 'nonce-{nonce}'; style-src 'self' 'nonce-{nonce}'; font-src 'self'; img-src 'self' data:; frame-ancestors 'none'")
}

struct App {
    port: u16,
    token: String, // the hook secret: hooks, companion mod, `clearance open` (ADR-0013)
    home: PathBuf, // ~/.clearance: token, hooks.json, endpoint.json, browsers.json, journal
    projects: PathBuf,
    state: Mutex<State>,
    terms: Arc<terms::Terms>, // sessions running in clearance's own terminals (ADR-0016)
}

#[derive(Default)]
struct State {
    board: Board,
    auth: auth::Auth,
    held: HashMap<String, mpsc::Sender<Json>>, // request id -> the hook call waiting for an answer
    clients: Vec<(String, TcpStream)>,         // open pages (server-sent events), with their browser
    last: String,
    stops: HashMap<String, (&'static str, u64)>, // session -> "interrupt" or "exit", and when it was asked
    // ponytail: never pruned, a few bytes per session clearance has seen since it started.
    mods: HashMap<String, u64>,            // session -> version of its companion mod: 2 and up can stop the session
    fleet: fleet::Fleet,                   // groups, stopped sessions, names (~/.clearance/fleet.json, ADR-0015)
    memory: HashMap<String, u64>,          // session -> bytes of its processes, read every 10 s
    first_groups: HashMap<String, String>, // session name -> group picked when it was started from the page
    lang: String,                          // the page's language, for the line under the prompt
    terms: Vec<Json>,                      // clearance's terminals for the page: id, name, pid, session
    terms_saved: String,                   // terminals.json as last written
    restart: Option<(u64, bool)>,          // a restart asked from the page: since when, forced
    prompts: Json,                         // the user's shortcuts, ~/.clearance/prompts.json: [{label, text}]
    started: u64,                          // when this clearance started, for the Settings
    claude: Option<String>,                // `claude --version`, read every 10 minutes (ADR-0017)
    poll: (u64, u64, u64),                 // last good `claude agents`: when, how long it took (ms); last failure
    pinged: u64,                           // last heartbeat written to the page streams
}

impl State {
    /// What the page gets: the board, the user's groups and language.
    fn snapshot(&self) -> Json {
        let mut snap = self.board.snapshot();
        snap.set("fleet", self.fleet.to_json());
        snap.set("memory", Json::Obj(self.memory.iter().map(|(id, b)| (id.clone(), (*b).into())).collect()));
        snap.set("lang", self.lang.as_str().into());
        snap.set("mods", Json::Obj(self.mods.iter().map(|(id, v)| (id.clone(), (*v).into())).collect()));
        snap.set("terms", Json::Arr(self.terms.clone()));
        snap.set("restart", self.restart.map(|(since, _)| since).into());
        snap.set("prompts", self.prompts.clone());
        snap.set("claude", self.claude.as_deref().map_or(Json::Null, |v| obj([("version", v.into()), ("validated", claude::validated(v).into())])));
        snap.set("poll", obj([("at", self.poll.0.into()), ("ms", self.poll.1.into()), ("failedAt", self.poll.2.into())]));
        snap.set("version", obj([("version", env!("CARGO_PKG_VERSION").into()), ("commit", env!("CLEARANCE_COMMIT").into()), ("started", self.started.into())]));
        snap
    }

    fn publish(&mut self) {
        let data = self.snapshot().to_string();
        if data == self.last {
            return;
        }
        let msg = format!("data: {data}\n\n");
        self.clients.retain(|(_, c)| {
            let mut w: &TcpStream = c;
            w.write_all(msg.as_bytes()).is_ok()
        });
        self.last = data;
    }

    /// A comment line on every page stream every 15 s: a dead one is dropped even when nothing changes,
    /// and the poll slows down once no page is left (ADR-0017).
    fn ping(&mut self, now: u64) {
        if now.saturating_sub(self.pinged) < 15_000 {
            return;
        }
        self.pinged = now;
        self.clients.retain(|(_, c)| {
            let mut w: &TcpStream = c;
            w.write_all(b": ping\n\n").is_ok()
        });
    }

    /// Whether Claude Code's version, when known, is outside the validated range: nothing is allowed then.
    fn unvalidated(&self) -> Option<&str> {
        self.claude.as_deref().filter(|v| !claude::validated(v))
    }

    /// Answers a held hook call and takes the request off the board. `{}` means no decision: Claude
    /// Code carries on with its own dialog.
    fn finish(&mut self, id: &str, out: Json) {
        self.board.end(id);
        if let Some(tx) = self.held.remove(id) {
            let _ = tx.send(out);
            self.say_held();
        }
    }

    /// Said in clearance's own tab whenever it changes: stopping clearance drops what it holds, and a
    /// dropped request is left to its terminal for good.
    fn say_held(&self) {
        println!("{}", held_line(&self.lang, self.held.len()));
    }
}

fn held_line(lang: &str, held: usize) -> String {
    match (lang, held) {
        ("fr", 0) => "rien en attente : clearance peut être arrêté".into(),
        ("fr", n) => format!("{n} demande(s) en attente : arrêter clearance maintenant les renvoie à leur terminal"),
        (_, 0) => "nothing held: clearance can be stopped".into(),
        (_, n) => format!("{n} request(s) held: stopping clearance now sends them back to their terminals"),
    }
}

impl App {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Ends every page stream before clearance exits: on Windows a stream could outlive the process,
    /// leaving the page on a clearance gone (seen on a restart, cause not found).
    fn hang_up(&self) {
        for (_, c) in self.lock().clients.drain(..) {
            let _ = c.shutdown(std::net::Shutdown::Both);
        }
        self.terms.forget_pages(|_| true);
    }

    fn base(&self) -> String {
        format!("http://{HOST}:{}", self.port)
    }

    /// The sessions seen sending hooks, kept across restarts: after one, a session is still known to be
    /// connected before its next hook.
    fn keep_connected(&self, st: &State) {
        let ids = Json::Obj(st.board.connected.iter().map(|id| (id.clone(), Json::Bool(true))).collect());
        let _ = fs::write(self.home.join("connected.json"), ids.to_string());
    }

    fn keep_fleet(&self, st: &State) {
        let _ = fs::write(self.home.join("fleet.json"), st.fleet.to_json().pretty());
    }

    fn keep_browsers(&self, st: &State) {
        let _ = write_private(&self.home.join("browsers.json"), &st.auth.to_json().pretty());
    }

    /// The browser behind a request: its session's digest, when its cookie is a live session.
    fn browser(&self, req: &http::Request) -> Option<String> {
        let id = req.cookie("clearance")?;
        let mut st = self.lock();
        let (digest, changed) = st.auth.check(id, sys::now_ms());
        if changed {
            self.keep_browsers(&st);
        }
        digest
    }

    /// Opens the page in the default browser on a fresh single-use link, which never leaves clearance
    /// for anything but the browser.
    fn open_page(&self) -> Result<(), String> {
        let link = self.lock().auth.link(sys::now_ms()).map_err(|e| e.to_string())?;
        sys::open_url(&format!("{}/?login={link}", self.base())).map_err(|e| e.to_string())
    }

    /// Append-only journal of what was decided from the page (BRIEF section 6).
    fn journal(&self, entry: Json) {
        let mut line = obj([("at", sys::iso(sys::now_ms()).into())]);
        for (k, v) in entry.entries() {
            line.set(k, v.clone());
        }
        let file = self.home.join("journal.jsonl");
        rotate(&file, JOURNAL_MAX);
        append(&file, &line);
    }
}

/// A JSON file of ours; an empty object when missing or broken.
fn read_json(file: &Path) -> Json {
    fs::read_to_string(file).ok().and_then(|t| Json::parse(&t).ok()).filter(|j| matches!(j, Json::Obj(_))).unwrap_or(Json::Obj(Vec::new()))
}

// About 10 MB of journal in all: the current file and the one before (ADR-0017).
const JOURNAL_MAX: u64 = 5 * 1024 * 1024;

/// Past `max` bytes, `x.jsonl` becomes `x.1.jsonl`, replacing the one before.
fn rotate(file: &Path, max: u64) {
    if fs::metadata(file).is_ok_and(|m| m.len() > max) {
        let _ = fs::rename(file, file.with_extension("1.jsonl"));
    }
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
    let app = App { port, token, home, projects: claude::projects(), state: Mutex::default(), terms: Arc::default() };
    match args.first().map(String::as_str) {
        // In place: what the windowless clearance runs, and a way to watch it while working on it.
        Some("--foreground") => serve(Arc::new(app)),
        // No window to keep open (ADR-0016): the same clearance, detached; the page opens signed in. A second
        // one finds the first on its port and only opens the page.
        None => {
            let exe = std::env::current_exe().unwrap_or_else(|e| fail(&e.to_string()));
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("--foreground");
            sys::hidden(&mut cmd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            // Detached on purpose: this process ends now, the background clearance goes on.
            #[allow(clippy::zombie_processes)]
            let _background = cmd.spawn().unwrap_or_else(|e| fail(&e.to_string()));
            println!("clearance runs in the background on {}; `clearance open` opens its page.", app.base());
        }
        Some("open") => {
            if !ask_open(&app) {
                fail(&format!("clearance does not run on {} (start it with: clearance)", app.base()));
            }
        }
        Some("install") => install::run(&args[1..], &app.base(), &app.token, &app.home).unwrap_or_else(|e| fail(&e)),
        Some("-h" | "--help" | "help") => {
            println!(
                "clearance                      serve the page and the hooks, without a window\nclearance --foreground         the same, in this terminal\nclearance open                 open the page, signed in\nclearance install [--dry-run] [--remove] [--settings <file>]"
            )
        }
        Some(other) => fail(&format!("unknown command: {other} (clearance --help)")),
    }
}

/// Asks the clearance running on this port to open its page; whether it answered.
fn ask_open(app: &App) -> bool {
    let Ok(mut s) = TcpStream::connect((HOST, app.port)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let head = format!("POST /open HTTP/1.1\r\nHost: {HOST}:{}\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", app.port, app.token);
    let mut answer = String::new();
    s.write_all(head.as_bytes()).is_ok() && io::Read::read_to_string(&mut s, &mut answer).is_ok() && answer.starts_with("HTTP/1.1 200")
}

fn fail(message: &str) -> ! {
    eprintln!("clearance: {message}");
    std::process::exit(1)
}

fn serve(app: Arc<App>) {
    // Started by a restart: the clearance it replaces still holds the port for a moment.
    let restarted = std::env::var_os(RESTARTED).is_some();
    let mut bound = TcpListener::bind((HOST, app.port));
    for _ in 0..60 {
        match &bound {
            Err(e) if restarted && e.kind() == io::ErrorKind::AddrInUse => {
                thread::sleep(Duration::from_millis(250));
                bound = TcpListener::bind((HOST, app.port));
            }
            _ => break,
        }
    }
    let listener = match bound {
        Ok(l) => l,
        // Already running: a second `clearance` opens its page instead.
        Err(e) if e.kind() == io::ErrorKind::AddrInUse && ask_open(&app) => {
            println!("clearance already runs on {}: its page opens in the browser.", app.base());
            return;
        }
        Err(e) => fail(&format!("{}: {e}", app.base())),
    };
    let hooks = install::session_settings(&app.base(), &app.token, &app.home);
    let endpoint = obj([("url", app.base().into()), ("token", app.token.as_str().into())]);
    // The companion mod reads where clearance is from ~/.clearance/endpoint.json, wherever it is installed.
    for (name, text) in [("hooks.json", hooks.pretty()), ("endpoint.json", endpoint.to_string())] {
        write_private(&app.home.join(name), &text).unwrap_or_else(|e| fail(&format!("{name}: {e}")));
    }
    {
        let mut st = app.lock();
        // Groups were folders until ADR-0015: the old map is set aside, the user makes their own groups.
        let (old, kept) = (app.home.join("groups.json"), app.home.join("groups.json.old"));
        if old.exists() && !kept.exists() {
            let _ = fs::rename(&old, &kept);
        }
        st.fleet = fleet::Fleet::load(&read_json(&app.home.join("fleet.json")));
        st.lang = read_json(&app.home.join("prefs.json")).get("lang").str().unwrap_or("").to_string();
        st.board.connected = read_json(&app.home.join("connected.json")).entries().iter().map(|(id, _)| id.clone()).collect();
        st.auth = auth::Auth::load(&read_json(&app.home.join("browsers.json")));
        st.prompts = prompts(&read_json(&app.home.join("prompts.json"))).unwrap_or(Json::Arr(Vec::new()));
        st.started = sys::now_ms();
    }
    println!("clearance on {}", app.base());
    // The first time, the browser opens by itself, and the link is printed in case it cannot; afterwards
    // browsers stay signed in across restarts, and no link lies around in this output.
    if app.lock().auth.is_empty() {
        let link = app.lock().auth.link(sys::now_ms()).unwrap_or_else(|e| fail(&e.to_string()));
        println!("sign your browser in (once, within 5 minutes): {}/?login={link}", app.base());
        let _ = app.open_page();
    }
    println!("open the page, or sign another browser in: clearance open");
    println!("hooks for a session: claude --settings {}", app.home.join("hooks.json").display());
    reopen_terms(&app);
    let poller = app.clone();
    thread::spawn(move || poll(&poller));
    let weigher = app.clone();
    thread::spawn(move || weigh(&weigher));
    let restarter = app.clone();
    thread::spawn(move || restart_when_idle(&restarter));
    // Claude Code's version, against the range clearance was checked with (ADR-0017).
    let versions = app.clone();
    thread::spawn(move || {
        loop {
            if let Some(v) = claude::version() {
                let mut st = versions.lock();
                st.claude = Some(v);
                st.publish();
            }
            thread::sleep(Duration::from_secs(600));
        }
    });
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

// Set for the clearance a restart starts, which waits for the port instead of opening the page.
const RESTARTED: &str = "CLEARANCE_RESTARTED";
const RESTART_WAIT_MS: u64 = 20 * 60_000;

/// Starts a session in one of clearance's own terminals (ADR-0016); its group is placed once it shows up.
fn start_term(app: &Arc<App>, start: &terminal::Start, extra: &[String], group: Option<String>) -> Result<(), String> {
    start.check()?;
    let args = start.claude(extra);
    let launch = terms::Launch {
        name: start.name.clone(),
        cwd: start.cwd.clone(),
        model: start.model.clone(),
        mode: start.mode.clone(),
        group: group.clone(),
        resume: Some(start.resume.clone()).filter(|r| !r.is_empty()),
    };
    let back = app.clone();
    app.terms
        .start(&args[0], &args[1..], launch, move |_| {
            let mut st = back.lock();
            refresh_terms(&back, &mut st);
            st.publish();
        })
        .map_err(|e| e.to_string())?;
    let mut st = app.lock();
    if let Some(g) = group {
        st.first_groups.insert(start.name.clone(), g);
    }
    refresh_terms(app, &mut st);
    st.publish();
    Ok(())
}

/// The page's list of terminals, each linked to its session by process id, and `terminals.json`, what
/// a next start of clearance reopens.
fn refresh_terms(app: &App, st: &mut State) {
    let rows: Vec<(Arc<terms::Term>, Option<String>)> = app
        .terms
        .all()
        .into_iter()
        .map(|t| {
            let session = st.board.sessions.iter().find(|s| s.get("pid").num() == Some(f64::from(t.pid()))).and_then(|s| s.get("sessionId").str()).map(String::from);
            (t, session)
        })
        .collect();
    st.terms = rows
        .iter()
        .map(|(t, session)| {
            obj([
                ("id", t.id.as_str().into()),
                ("name", t.launch.name.as_str().into()),
                ("cwd", t.launch.cwd.as_str().into()),
                ("pid", u64::from(t.pid()).into()),
                ("session", session.as_deref().into()),
            ])
        })
        .collect();
    let saved = Json::Arr(rows.iter().map(|(t, session)| t.launch.to_json(session.as_deref().or(t.launch.resume.as_deref()))).collect()).pretty();
    if saved != st.terms_saved {
        let _ = fs::write(app.home.join("terminals.json"), &saved);
        st.terms_saved = saved;
    }
}

/// The terminals clearance ran when it stopped, reopened on their sessions (ADR-0016).
fn reopen_terms(app: &Arc<App>) {
    let saved = fs::read_to_string(app.home.join("terminals.json")).ok().and_then(|t| Json::parse(&t).ok()).unwrap_or(Json::Null);
    for t in saved.items() {
        let s = |k: &str| t.get(k).str().unwrap_or("").to_string();
        let start = terminal::Start { cwd: s("cwd"), name: s("name"), model: s("model"), mode: s("mode"), resume: s("session"), prompt: String::new() };
        if start.resume.is_empty() {
            continue;
        }
        let extra = session_extra(app);
        if let Err(e) = start_term(app, &start, &extra, t.get("group").str().map(String::from)) {
            eprintln!("clearance: could not reopen {}: {e}", start.name);
        }
    }
}

/// A restart asked from the page: once clearance's own sessions are idle (or forced, or after 20 minutes),
/// a new clearance starts and this one exits; the new one reopens them.
fn restart_when_idle(app: &App) {
    loop {
        thread::sleep(Duration::from_secs(2));
        let due = {
            let st = app.lock();
            let Some((since, forced)) = st.restart else { continue };
            let mine: Vec<&str> = st.terms.iter().filter_map(|t| t.get("session").str()).collect();
            let busy =
                st.board.sessions.iter().filter(|s| mine.contains(&s.get("sessionId").str().unwrap_or("")) && matches!(s.get("status").str(), Some("busy" | "waiting"))).count();
            forced || busy == 0 || sys::now_ms().saturating_sub(since) > RESTART_WAIT_MS
        };
        if due {
            let mut cmd = std::process::Command::new(std::env::current_exe().unwrap_or_else(|e| fail(&e.to_string())));
            // ponytail: the new clearance runs without a window (ADR-0016); the old one's tab closes with it.
            sys::hidden(&mut cmd).arg("--foreground").env(RESTARTED, "1");
            cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            match cmd.spawn() {
                Ok(_) => {
                    app.hang_up();
                    std::process::exit(0)
                }
                Err(e) => {
                    eprintln!("clearance: restart failed: {e}");
                    app.lock().restart = None;
                }
            }
        }
    }
}

/// What a session clearance starts needs when the user's settings lack clearance's hooks: its hooks file
/// and the companion mod (passing them twice would ask twice).
fn session_extra(app: &App) -> Vec<String> {
    let mut extra = Vec::new();
    if !install::read_settings(&install::user_settings()).is_ok_and(|x| install::installed(&x)) {
        extra.extend(["--settings".to_string(), app.home.join("hooks.json").display().to_string()]);
        let mod_dir = Path::new(REPO).join("mod");
        if mod_dir.is_dir() {
            extra.extend(["--plugin-dir".to_string(), mod_dir.display().to_string()]);
        }
    }
    extra
}

/// `claude agents` every 2 s while a page is open, every 10 s otherwise (ADR-0010).
fn poll(app: &App) {
    loop {
        let asked = sys::now_ms();
        let list = claude::agents().map(|l| l.into_iter().map(|s| with_transcript(&app.projects, s)).collect::<Vec<_>>());
        let open = {
            let mut st = app.lock();
            let now = sys::now_ms();
            if list.is_some() {
                st.poll = (now, now.saturating_sub(asked), st.poll.2);
            } else {
                st.poll.2 = now;
            }
            st.ping(now);
            if let Some(list) = list {
                let known = st.board.connected.len();
                let before = st.board.sessions.clone();
                for id in st.board.sync(list, sys::now_ms()) {
                    st.finish(&id, Json::Obj(Vec::new()));
                }
                let now_list = st.board.sessions.clone();
                let mut changed = st.fleet.observe(&before, &now_list, sys::now_ms());
                // A session started from the page goes in the group picked for it once it shows up.
                for s in &now_list {
                    let (Some(name), Some(id)) = (s.get("name").str(), s.get("sessionId").str()) else { continue };
                    if let Some(g) = st.first_groups.remove(name) {
                        changed |= st.fleet.place(id, Some(&g)).is_ok();
                    }
                }
                refresh_terms(app, &mut st);
                if changed {
                    app.keep_fleet(&st);
                }
                if st.board.connected.len() != known {
                    app.keep_connected(&st);
                }
            }
            st.publish();
            !st.clients.is_empty()
        };
        thread::sleep(Duration::from_secs(if open { 2 } else { 10 }));
    }
}

/// The memory of each session's processes, every 10 s (ADR-0015): apart from the 2 s poll, a process
/// table takes a moment to read.
fn weigh(app: &App) {
    loop {
        let pids: Vec<(String, u32)> = app.lock().board.sessions.iter().filter_map(|s| Some((s.get("sessionId").str()?.to_string(), s.get("pid").num()? as u32))).collect();
        let bytes = sys::tree_memory(&pids.iter().map(|(_, p)| *p).collect::<Vec<_>>());
        {
            let mut st = app.lock();
            st.memory = pids.into_iter().filter_map(|(id, pid)| Some((id, *bytes.get(&pid).filter(|b| **b > 0)?))).collect();
            st.publish();
        }
        thread::sleep(Duration::from_secs(10));
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
        // The main loop waits while its subagents or its workflow work: the session is at work, and
        // its turn is not over until they are done.
        let at_work = claude::agents_at_work(&file, now);
        if at_work > 0 {
            s.set("subagents", at_work.into());
            if s.get("status").str() == Some("idle") {
                s.set("status", "busy".into());
            }
        }
        let quiet = s.get("lastActive").num().map(|ms| now.saturating_sub(ms as u64));
        if let Some((tool, target, count)) = claude::recent_loop(&t, now) {
            s.set("watch", obj([("kind", "loop".into()), ("tool", tool.into()), ("target", target.into()), ("count", count.into())]));
        } else if s.get("status").str() == Some("busy") && at_work == 0 && quiet.is_some_and(|q| q > STALL_MS) {
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

fn route(app: &Arc<App>, s: &TcpStream, req: &http::Request) -> io::Result<()> {
    // Only our own host name: a page on another site cannot reach us through DNS rebinding.
    let host = req.header("host").unwrap_or("");
    if host != format!("{HOST}:{}", app.port) && host != format!("localhost:{}", app.port) {
        return http::respond(s, 421, &[], b"");
    }
    let bearer = req.header("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    let path = req.path.as_str();

    // The hook secret (ADR-0013): hooks, the companion mod, and `clearance open`. Never the page.
    if req.method == "POST" && (path.starts_with("/hook/") || path == "/mod/sync" || path == "/open") {
        if !sys::same(bearer, &app.token) {
            return http::respond(s, 401, &[], b"");
        }
        return match path.strip_prefix("/hook/") {
            Some(event) => hook(app, s, event, &req.json()),
            None if path == "/open" => match app.open_page() {
                Ok(()) => ok(s),
                Err(e) => error(s, 500, &e),
            },
            None => mod_sync(app, s, &req.json()),
        };
    }

    if req.method == "GET" && path.starts_with("/fonts/") {
        return font(s, &path["/fonts/".len()..]);
    }
    if req.method == "GET" && path.starts_with("/vendor/xterm/") {
        return vendor(s, &path["/vendor/xterm/".len()..]);
    }

    if req.method == "GET" && path == "/" {
        if let Some(link) = req.param("login") {
            let mut st = app.lock();
            if !st.auth.redeem(link, sys::now_ms()) {
                return http::respond(s, 401, &[("content-type", "text/plain; charset=utf-8")], SIGNED_OUT.as_bytes());
            }
            let id = st.auth.sign_in(req.header("user-agent").unwrap_or(""), sys::now_ms())?;
            app.keep_browsers(&st);
            // ponytail: 400 days, the most a browser keeps a cookie; clearance ends the session after 30 unused.
            let cookie = format!("clearance={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=34560000");
            return http::respond(s, 303, &[("location", "/"), ("set-cookie", &cookie)], b"");
        }
        if app.browser(req).is_none() {
            return http::respond(s, 401, &[("content-type", "text/plain; charset=utf-8")], SIGNED_OUT.as_bytes());
        }
        let nonce = sys::random_hex(16)?;
        let page = fs::read_to_string(Path::new(REPO).join("public").join("index.html")).unwrap_or_else(|_| PAGE.to_string()).replace(NONCE, &nonce);
        let policy = csp(&nonce);
        return http::respond(s, 200, &[("content-type", "text/html; charset=utf-8"), ("cache-control", "no-store"), ("content-security-policy", &policy)], page.as_bytes());
    }

    let Some(me) = app.browser(req) else { return http::respond(s, 401, &[], b"") };

    if req.method == "GET" {
        if path == "/events" {
            http::events(s)?;
            let mut st = app.lock();
            let mut w = s;
            write!(w, "data: {}\n\n", st.snapshot())?;
            let _ = s.set_write_timeout(Some(Duration::from_secs(2))); // a stuck page must not hold the board
            st.clients.push((me, s.try_clone()?));
            return Ok(());
        }
        if path == "/api/folders" {
            return http::json(s, 200, &folders(req.param("path").unwrap_or("")));
        }
        if path == "/api/browsers" {
            return http::json(s, 200, &app.lock().auth.list(&me));
        }
        // What a terminal draws, from what it kept (ADR-0016).
        if let Some(id) = path.strip_prefix("/api/term/") {
            return if app.terms.attach(id, &me, s)? { Ok(()) } else { error(s, 404, "No such terminal.") };
        }
        if let Some(id) = path.strip_prefix("/api/session/") {
            let cwd = app.lock().board.session(id).and_then(|s| s.get("cwd").str().map(String::from));
            // No transcript yet (nothing sent to a session just started): nothing to show, not an error.
            return http::json(s, 200, &claude::detail(&app.projects, id, cwd.as_deref()).unwrap_or(Json::Null));
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
        // The user's shortcuts, the whole list at once.
        "/api/prompts" => {
            let list = match prompts(body.get("prompts")) {
                Ok(l) => l,
                Err(e) => return error(s, 400, e),
            };
            if let Err(e) = fs::write(app.home.join("prompts.json"), list.pretty()) {
                return error(s, 500, &e.to_string());
            }
            let mut st = app.lock();
            st.prompts = list;
            st.publish();
            ok(s)
        }
        // The user's groups (ADR-0015): `create` (name), `rename` (id, name), `delete` (id).
        "/api/groups" => {
            let (id, name) = (body.get("id").str().unwrap_or(""), body.get("name").str().unwrap_or(""));
            let mut st = app.lock();
            let done = match body.get("action").str() {
                Some("create") => st.fleet.create(name).map(|_| ()),
                Some("rename") => st.fleet.rename(id, name),
                Some("delete") => st.fleet.delete(id).then_some(()).ok_or("No such group."),
                _ => Err("Unknown action."),
            };
            if let Err(e) = done {
                return error(s, 400, e);
            }
            app.keep_fleet(&st);
            st.publish();
            ok(s)
        }
        // A session into a group, or out of every group with no `group`.
        "/api/place" => {
            let session = body.get("session").str().unwrap_or("");
            let mut st = app.lock();
            if session.is_empty() {
                return error(s, 400, "Which session?");
            }
            if let Err(e) = st.fleet.place(session, body.get("group").str()) {
                return error(s, 400, e);
            }
            app.keep_fleet(&st);
            st.publish();
            ok(s)
        }
        // Forgets a stopped session in clearance; Claude Code's transcript is not touched.
        "/api/forget" => {
            let session = body.get("session").str().unwrap_or("");
            let mut st = app.lock();
            if !st.fleet.forget(session) {
                return error(s, 400, "This session is not a stopped one.");
            }
            app.keep_fleet(&st);
            st.publish();
            drop(st);
            app.journal(obj([("kind", "forget".into()), ("session", session.into())]));
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
        // Signs one browser out (`id` as the Settings list it), or every other one.
        "/api/browsers/sign-out" => {
            let mut st = app.lock();
            let gone = st.auth.sign_out(body.get("id").str(), &me);
            st.clients.retain(|(who, c)| {
                let keep = !gone.contains(who);
                if !keep {
                    let _ = c.shutdown(std::net::Shutdown::Both);
                }
                keep
            });
            app.keep_browsers(&st);
            app.terms.forget_pages(|b| gone.iter().any(|g| g == b));
            if gone.contains(&me) {
                return http::respond(
                    s,
                    200,
                    &[("content-type", "application/json"), ("set-cookie", "clearance=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")],
                    b"{\"ok\":true}",
                );
            }
            http::json(s, 200, &obj([("ok", true.into()), ("signedOut", gone.len().into())]))
        }
        // Keys typed in a terminal, and its size.
        p if p.starts_with("/api/term/") && p.ends_with("/input") => {
            let id = &p["/api/term/".len()..p.len() - "/input".len()];
            if app.terms.input(id, body.get("data").str().unwrap_or(""))? { ok(s) } else { error(s, 404, "No such terminal.") }
        }
        p if p.starts_with("/api/term/") && p.ends_with("/resize") => {
            let id = &p["/api/term/".len()..p.len() - "/resize".len()];
            let size = |k: &str| body.get(k).num().map_or(0, |n| n.clamp(2.0, 500.0) as u16);
            if app.terms.resize(id, size("cols"), size("rows"))? { ok(s) } else { error(s, 404, "No such terminal.") }
        }
        // A file pasted or dropped in the page, kept where a session can read it: its path goes in the message.
        "/api/attach" => match attach(req.header("x-name").unwrap_or("file"), req.body()) {
            Ok(path) => http::json(s, 200, &obj([("path", path.into())])),
            Err(e) => error(s, 400, &e),
        },
        // start waits for clearance's own sessions to be idle, orce restarts now, cancel gives up.
        "/api/restart" => {
            let mut st = app.lock();
            st.restart = match body.get("action").str() {
                Some("start") => Some((sys::now_ms(), false)),
                Some("force") => Some((st.restart.map_or(sys::now_ms(), |r| r.0), true)),
                _ => None,
            };
            st.publish();
            ok(s)
        }
        // Stops clearance, which ends the sessions in its terminals; they are reopened at its next start.
        "/api/quit" => {
            ok(s)?;
            app.journal(obj([("kind", "quit".into())]));
            let app = app.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(300));
                app.hang_up();
                std::process::exit(0)
            });
            Ok(())
        }
        "/api/read" => {
            let mut st = app.lock();
            st.board.read(body.get("session").str().unwrap_or(""));
            st.publish();
            ok(s)
        }
        // Stops sessions through their companion mod: the running turn ends, and with "exit" the session quits.
        "/api/stop" => {
            let kind = if body.get("exit").bool() == Some(true) { "exit" } else { "interrupt" };
            let ids: Vec<&str> = body.get("sessions").items().iter().filter_map(Json::str).collect();
            let mut st = app.lock();
            if ids.is_empty() || ids.iter().any(|id| st.board.session(id).is_none()) {
                return error(s, 400, "Stop needs sessions that are running.");
            }
            let now = sys::now_ms();
            for id in &ids {
                st.stops.insert(id.to_string(), (kind, now));
            }
            drop(st);
            app.journal(obj([("kind", "stop".into()), ("how", kind.into()), ("sessions", Json::Arr(ids.iter().map(|&i| i.into()).collect()))]));
            ok(s)
        }
        _ => http::respond(s, 404, &[], b""),
    }
}

const SIGNED_OUT: &str = "This browser is not signed in to clearance, or its link was already used or is more than 5 minutes old.\nRun `clearance open` in a terminal: the page opens here, signed in.\n\nCe navigateur n'est pas connecté à clearance, ou son lien a déjà servi ou date de plus de 5 minutes.\nLancez `clearance open` dans un terminal : la page s'ouvre ici, connectée.\n";

// xterm.js and its fit addon, frozen with their licences (ADR-0016).
const VENDOR: [(&str, &[u8]); 5] = [
    ("xterm.js", include_bytes!("../public/vendor/xterm/xterm.js")),
    ("xterm.css", include_bytes!("../public/vendor/xterm/xterm.css")),
    ("addon-fit.js", include_bytes!("../public/vendor/xterm/addon-fit.js")),
    ("LICENSE-xterm.txt", include_bytes!("../public/vendor/xterm/LICENSE-xterm.txt")),
    ("LICENSE-addon-fit.txt", include_bytes!("../public/vendor/xterm/LICENSE-addon-fit.txt")),
];

fn vendor(s: &TcpStream, name: &str) -> io::Result<()> {
    let Some((_, embedded)) = VENDOR.iter().find(|(n, _)| *n == name) else { return http::respond(s, 404, &[], b"") };
    let bytes = fs::read(Path::new(REPO).join("public").join("vendor").join("xterm").join(name)).unwrap_or_else(|_| embedded.to_vec());
    let kind = if name.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if name.ends_with(".css") {
        "text/css; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    };
    http::respond(s, 200, &[("content-type", kind), ("cache-control", "max-age=86400")], &bytes)
}

/// A folder and its subfolders, for the start form's folder browser; the home folder by default. Names
/// only, hidden folders left out; no file is ever listed.
fn folders(path: &str) -> Json {
    let at = Some(PathBuf::from(path)).filter(|p| !path.is_empty() && p.is_dir()).unwrap_or_else(sys::home);
    let mut names: Vec<String> = fs::read_dir(&at)
        .map(|d| d.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).filter_map(|e| e.file_name().into_string().ok()).filter(|n| !n.starts_with('.')).collect())
        .unwrap_or_default();
    names.sort_by_key(|n| n.to_lowercase());
    obj([
        ("path", at.display().to_string().into()),
        ("parent", at.parent().map(|p| p.display().to_string()).into()),
        ("folders", Json::Arr(names.into_iter().take(500).map(Json::from).collect())),
    ])
}

const MAX_ATTACHMENT: usize = 25 * 1024 * 1024;

/// Saves a pasted or dropped file in its own folder under the system's temporary one, outside
/// clearance's folder, which sessions are denied (ADR-0013); its path.
fn attach(name: &str, bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT {
        return Err("A file of 1 byte to 25 MB.".into());
    }
    let name: String = http::decode(name, false).chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    let name = name.trim_start_matches('.');
    let name = if name.is_empty() { "file" } else { name };
    let dir = std::env::temp_dir().join("clearance-attachments").join(sys::random_hex(8).map_err(|e| e.to_string())?);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// One of the page's fonts (ADR-0014): from the repository while it is there, else from the binary.
fn font(s: &TcpStream, name: &str) -> io::Result<()> {
    let known = FONTS.iter().find(|(n, _)| *n == name);
    let Some((_, embedded)) = known else { return http::respond(s, 404, &[], b"") };
    let bytes = fs::read(Path::new(REPO).join("public").join("fonts").join(name)).unwrap_or_else(|_| embedded.to_vec());
    let kind = if name.ends_with(".woff2") { "font/woff2" } else { "text/plain; charset=utf-8" };
    http::respond(s, 200, &[("content-type", kind), ("cache-control", "max-age=86400")], &bytes)
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
        let known = st.board.connected.len();
        let (request, ended) = st.board.hook(event, body, sys::now_ms());
        if st.board.connected.len() != known {
            app.keep_connected(&st);
        }
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
                st.say_held();
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
                    st.say_held();
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
    let (instruction, stop, waiting, here, lang) = {
        let mut st = app.lock();
        let now = sys::now_ms();
        st.board.report(session, body.get("usage"), now);
        // A mod loaded before it could stop its session says no version: the page must not promise a stop.
        st.mods.insert(session.to_string(), body.get("mod").num().map_or(1, |v| v as u64));
        let instruction = st.board.take(session, now);
        let stop = st.stops.remove(session).and_then(|asked| still_meant(asked, now));
        st.publish();
        (instruction, stop, st.board.requests.len(), st.board.requests.iter().filter(|r| r.session == session).count(), st.lang.clone())
    };
    if instruction.is_some() {
        app.journal(obj([("kind", "instruction-delivered".into()), ("session", session.into())]));
    }
    if let Some(how) = stop {
        app.journal(obj([("kind", "stop-delivered".into()), ("how", how.into()), ("session", session.into())]));
    }
    let line = status_line(&lang, waiting, here);
    http::json(s, 200, &obj([("waiting", waiting.into()), ("here", here.into()), ("instruction", instruction.into()), ("stop", stop.into()), ("line", line.into())]))
}

/// A stop is for the session as it ran when it was asked: one not picked up within a minute (no mod, the
/// session asleep) is dropped, so that it never ends the same session resumed later.
fn still_meant((how, at): (&'static str, u64), now: u64) -> Option<&'static str> {
    (now.saturating_sub(at) < 60_000).then_some(how)
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
    let action = decision.get("action").str().unwrap_or("");
    // Outside the validated versions only what lets nothing run passes (ADR-0017).
    if let Some(v) = st.unvalidated().filter(|_| !matches!(action, "deny" | "dismiss")) {
        return error(s, 409, &format!("Claude Code {v} is not validated with clearance: decide in its terminal."));
    }
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

/// The shortcuts as kept: up to 10 (Ctrl+Shift+1 to 0), each a label and the text it sends.
fn prompts(list: &Json) -> Result<Json, &'static str> {
    let Json::Arr(items) = list else { return Err("Shortcuts come as a list.") };
    if items.len() > 10 {
        return Err("10 shortcuts at most.");
    }
    let mut kept = Vec::new();
    for p in items {
        let (label, text) = (p.get("label").str().unwrap_or("").trim(), p.get("text").str().unwrap_or("").trim());
        if !(1..=30).contains(&label.chars().count()) || !(1..=8000).contains(&text.chars().count()) {
            return Err("A shortcut has a label (30 characters at most) and a text (8000 at most).");
        }
        kept.push(obj([("label", label.into()), ("text", text.into())]));
    }
    Ok(Json::Arr(kept))
}

/// A new session, or a stopped one resumed, in one of clearance's own terminals (ADR-0016).
fn start(app: &Arc<App>, s: &TcpStream, body: &Json) -> io::Result<()> {
    let start = terminal::Start::from_json(body);
    if let Err(e) = start.check() {
        return error(s, 400, e);
    }
    let extra = if body.get("connect").bool() == Some(false) { Vec::new() } else { session_extra(app) };
    let result = start_term(app, &start, &extra, body.get("group").str().map(String::from));
    let err = result.as_ref().err().map(String::as_str);
    app.journal(obj([
        ("kind", "start".into()),
        ("name", start.name.as_str().into()),
        ("cwd", start.cwd.as_str().into()),
        ("model", start.model.as_str().into()),
        ("mode", start.mode.as_str().into()),
        ("resume", Some(start.resume.as_str()).filter(|r| !r.is_empty()).into()),
        ("error", err.into()),
    ]));
    match err {
        Some(e) => error(s, 500, e),
        None => ok(s),
    }
}

/// The last `n` journal lines, newest first.
/// The last `n` lines, the file before the current one included.
fn journal_tail(file: &Path, n: usize) -> Vec<Json> {
    let text = [file.with_extension("1.jsonl"), file.to_path_buf()].iter().filter_map(|f| fs::read_to_string(f).ok()).collect::<String>();
    text.lines().rev().take(n).filter_map(|l| Json::parse(l).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_journal_keeps_two_files() {
        let app = test_app();
        let file = app.home.join("journal.jsonl");
        for n in 0..3u64 {
            append(&file, &obj([("n", n.into())]));
            rotate(&file, 5); // one line is more than 5 bytes
        }
        append(&file, &obj([("n", 3u64.into())]));
        assert_eq!(fs::read_to_string(file.with_extension("1.jsonl")).unwrap(), "{\"n\":2}\n", "the one before is replaced");
        let tail: Vec<String> = journal_tail(&file, 5).iter().map(Json::to_string).collect();
        assert_eq!(tail, [r#"{"n":3}"#, r#"{"n":2}"#], "newest first, across both files");
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn shortcuts_are_checked_and_trimmed() {
        let list = |t: &str| prompts(&Json::parse(t).unwrap());
        assert_eq!(list(r#"[{"label":" Tests ","text":"Run the tests. "}]"#).unwrap().to_string(), r#"[{"label":"Tests","text":"Run the tests."}]"#);
        assert!(list(r#"{"label":"x","text":"y"}"#).is_err());
        assert!(list(r#"[{"label":"","text":"y"}]"#).is_err());
        assert!(list(r#"[{"label":"x","text":"  "}]"#).is_err());
        assert!(list(&format!("[{}]", [r#"{"label":"x","text":"y"}"#; 11].join(","))).is_err());
    }

    #[test]
    fn a_stop_not_picked_up_within_a_minute_is_dropped() {
        assert_eq!(still_meant(("exit", 1_000), 1_000), Some("exit"));
        assert_eq!(still_meant(("interrupt", 1_000), 60_999), Some("interrupt"));
        assert_eq!(still_meant(("exit", 1_000), 61_000), None);
        assert_eq!(still_meant(("exit", 5_000), 1_000), Some("exit"), "a clock gone back does not drop it");
    }

    fn test_app() -> Arc<App> {
        let home = std::env::temp_dir().join(format!("clearance-test-{}", sys::random_hex(6).unwrap()));
        fs::create_dir_all(&home).unwrap();
        Arc::new(App { port: 1, token: "hook-secret".into(), home, projects: PathBuf::new(), state: Mutex::default(), terms: Arc::default() })
    }

    /// One request through `route`, over a real socket; the raw answer.
    fn call(app: &Arc<App>, method: &str, path: &str, headers: &[&str]) -> String {
        call_with(app, method, path, headers, "{}")
    }

    fn call_with(app: &Arc<App>, method: &str, path: &str, headers: &[&str], body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let a = app.clone();
        let server = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let req = http::Request::read(&s).unwrap();
            let _ = route(&a, &s, &req);
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let extra: String = headers.iter().map(|h| format!("{h}\r\n")).collect();
        write!(c, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:1\r\n{extra}Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        server.join().unwrap();
        let mut answer = Vec::new();
        io::Read::read_to_end(&mut c, &mut answer).unwrap();
        String::from_utf8_lossy(&answer).into_owned()
    }

    fn cookie_of(answer: &str) -> String {
        let at = answer.find("set-cookie: clearance=").unwrap() + "set-cookie: clearance=".len();
        answer[at..].split(';').next().unwrap().to_string()
    }

    #[test]
    fn the_hook_secret_opens_no_page_and_no_decision() {
        let app = test_app();
        let forged = ["Authorization: Bearer hook-secret", "Cookie: clearance=hook-secret", "Origin: http://127.0.0.1:1"];
        assert!(call(&app, "POST", "/api/decide", &forged).starts_with("HTTP/1.1 401"), "a session that reads the token cannot approve");
        assert!(call(&app, "GET", "/api/browsers", &forged).starts_with("HTTP/1.1 401"));
        assert!(call(&app, "GET", "/?token=hook-secret", &[]).starts_with("HTTP/1.1 401"), "the token no longer signs in");
        assert!(call(&app, "GET", "/", &["Cookie: clearance=hook-secret"]).starts_with("HTTP/1.1 401"));
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn a_login_link_signs_one_browser_in_once() {
        let app = test_app();
        let link = app.lock().auth.link(sys::now_ms()).unwrap();
        let first = call(&app, "GET", &format!("/?login={link}"), &["User-Agent: Firefox/140"]);
        assert!(first.starts_with("HTTP/1.1 303"), "{first}");
        assert!(first.contains("HttpOnly; SameSite=Strict"));
        assert!(call(&app, "GET", &format!("/?login={link}"), &[]).starts_with("HTTP/1.1 401"), "a link works once");
        let cookie = format!("Cookie: clearance={}", cookie_of(&first));
        let page = call(&app, "GET", "/", &[&cookie]);
        assert!(page.starts_with("HTTP/1.1 200"));
        let policy = page.lines().find(|l| l.starts_with("content-security-policy: ")).unwrap();
        assert!(!policy.contains("unsafe-inline") && policy.contains("script-src 'nonce-"), "{policy}");
        assert!(!page.contains(NONCE), "the nonce is filled in");
        assert!(call(&app, "POST", "/api/decide", &[&cookie]).starts_with("HTTP/1.1 403"), "a write still needs our Origin");
        assert!(call(&app, "POST", "/api/decide", &[&cookie, "Origin: http://127.0.0.1:1"]).starts_with("HTTP/1.1 410"));
        let saved = fs::read_to_string(app.home.join("browsers.json")).unwrap();
        assert!(!saved.contains(&cookie_of(&first)), "only the digest is kept");
        let origin = "Origin: http://127.0.0.1:1";
        assert!(call_with(&app, "POST", "/api/browsers/sign-out", &[&cookie, origin], "{}").contains("\"signedOut\":0"), "no other browser");
        let me = app.lock().auth.list("").items()[0].get("id").str().unwrap().to_string();
        let out = call_with(&app, "POST", "/api/browsers/sign-out", &[&cookie, origin], &format!("{{\"id\":\"{me}\"}}"));
        assert!(out.contains("set-cookie: clearance=; ") && out.contains("Max-Age=0"), "{out}");
        assert!(call(&app, "GET", "/", &[&cookie]).starts_with("HTTP/1.1 401"));
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn fonts_are_served_by_name_only() {
        let app = test_app();
        let font = call(&app, "GET", "/fonts/vt323-400.woff2", &[]);
        assert!(font.starts_with("HTTP/1.1 200") && font.contains("content-type: font/woff2"));
        assert!(call(&app, "GET", "/fonts/..%2Fmain.rs", &[]).starts_with("HTTP/1.1 404"));
        let _ = fs::remove_dir_all(&app.home);
    }

    // ---- end to end (ADR-0017): a fake Claude Code and a fake page over real sockets ----

    /// clearance on a port of its own, as `serve` runs it: each connection in its own thread.
    fn serve_test(app: &Arc<App>) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let a = app.clone();
        thread::spawn(move || {
            for s in listener.incoming().flatten() {
                let a = a.clone();
                thread::spawn(move || {
                    if let Ok(req) = http::Request::read(&s) {
                        let _ = route(&a, &s, &req);
                    }
                });
            }
        });
        addr
    }

    /// A request sent, its answer not read yet.
    fn send(addr: std::net::SocketAddr, method: &str, path: &str, headers: &[&str], body: &str) -> TcpStream {
        let mut c = TcpStream::connect(addr).unwrap();
        let extra: String = headers.iter().map(|h| format!("{h}\r\n")).collect();
        write!(c, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:1\r\n{extra}Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        c
    }

    fn answer(mut c: TcpStream) -> String {
        let mut out = Vec::new();
        let _ = io::Read::read_to_end(&mut c, &mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Claude Code asking for a permission: the hook call, held until someone decides.
    fn permission(addr: std::net::SocketAddr, command: &str) -> TcpStream {
        let body = format!(r#"{{"session_id":"sess-e2e","cwd":"/work/orchard","tool_name":"Bash","tool_input":{{"command":"{command}"}}}}"#);
        send(addr, "POST", "/hook/PermissionRequest", &["Authorization: Bearer hook-secret"], &body)
    }

    fn signed_in(app: &Arc<App>) -> String {
        let link = app.lock().auth.link(sys::now_ms()).unwrap();
        format!("Cookie: clearance={}", cookie_of(&call(app, "GET", &format!("/?login={link}"), &[])))
    }

    /// The id of the request on the board once the hook call is there.
    fn held_id(app: &Arc<App>) -> String {
        for _ in 0..200 {
            if let Some(r) = app.lock().board.requests.first() {
                return r.id.clone();
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("no request reached the board");
    }

    #[test]
    fn e2e_a_permission_held_then_allowed_from_the_page() {
        let app = test_app();
        let (addr, cookie) = (serve_test(&app), signed_in(&app));
        let hook = permission(addr, "cargo test");
        let id = held_id(&app);
        let decided = send(addr, "POST", "/api/decide", &[&cookie, "Origin: http://127.0.0.1:1"], &format!(r#"{{"id":"{id}","action":"allow"}}"#));
        assert!(answer(decided).starts_with("HTTP/1.1 200"));
        let out = answer(hook);
        assert!(out.contains(r#""behavior":"allow""#), "{out}");
        assert!(app.lock().board.requests.is_empty() && app.lock().held.is_empty());
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn e2e_answered_in_the_terminal_first() {
        let app = test_app();
        let (addr, cookie) = (serve_test(&app), signed_in(&app));
        let hook = permission(addr, "rm -rf build");
        let id = held_id(&app);
        // The user answers in the terminal: Claude Code drops its hook call.
        hook.shutdown(std::net::Shutdown::Both).unwrap();
        for _ in 0..300 {
            if app.lock().held.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(app.lock().board.requests.is_empty(), "the request leaves the board");
        assert!(app.lock().board.snapshot().get("settled").items().len() == 1, "and shows as settled in the terminal");
        let late = send(addr, "POST", "/api/decide", &[&cookie, "Origin: http://127.0.0.1:1"], &format!(r#"{{"id":"{id}","action":"allow"}}"#));
        assert!(answer(late).starts_with("HTTP/1.1 410"), "a late answer from the page is refused");
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn e2e_dropped_by_clearance_leaves_the_terminal_dialog() {
        let app = test_app();
        let addr = serve_test(&app);
        let hook = permission(addr, "npm test");
        held_id(&app);
        // A restart: clearance lets go of what it holds; the hook call ends with no decision.
        app.lock().held.clear();
        let out = answer(hook);
        assert!(!out.contains("behavior"), "no decision, Claude Code keeps its own dialog: {out}");
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn e2e_two_pages_see_the_same_request() {
        let app = test_app();
        let (addr, cookie) = (serve_test(&app), signed_in(&app));
        let pages: Vec<TcpStream> = (0..2).map(|_| send(addr, "GET", "/events", &[&cookie], "")).collect();
        thread::sleep(Duration::from_millis(200));
        let _hook = permission(addr, "git push origin main");
        held_id(&app);
        for mut p in pages {
            p.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut seen = String::new();
            let mut buf = [0u8; 65536];
            while !seen.contains("git push origin main") {
                let n = io::Read::read(&mut p, &mut buf).unwrap();
                assert!(n > 0, "the stream ended");
                seen.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            assert!(seen.contains(r#""risk":"infra""#));
        }
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn e2e_an_unvalidated_claude_code_allows_nothing() {
        let app = test_app();
        let (addr, cookie) = (serve_test(&app), signed_in(&app));
        app.lock().claude = Some("9.0.0".into());
        let hook = permission(addr, "cargo build");
        let id = held_id(&app);
        let origin = "Origin: http://127.0.0.1:1";
        let allow = answer(send(addr, "POST", "/api/decide", &[&cookie, origin], &format!(r#"{{"id":"{id}","action":"allow"}}"#)));
        assert!(allow.starts_with("HTTP/1.1 409") && allow.contains("decide in its terminal"), "{allow}");
        let deny = answer(send(addr, "POST", "/api/decide", &[&cookie, origin], &format!(r#"{{"id":"{id}","action":"deny"}}"#)));
        assert!(deny.starts_with("HTTP/1.1 200"));
        assert!(answer(hook).contains(r#""behavior":"deny""#));
        let _ = fs::remove_dir_all(&app.home);
    }

    #[test]
    fn the_tab_says_when_stopping_clearance_drops_requests() {
        assert_eq!(held_line("fr", 0), "rien en attente : clearance peut être arrêté");
        assert!(held_line("en", 2).starts_with("2 request(s) held"));
    }
}
