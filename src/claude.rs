//! The one module that reads Claude Code itself: `claude agents --json` and the files under
//! `~/.claude/projects` (transcripts, subagents, workflows). Their formats may change between versions.

use crate::json::{Json, obj};
use crate::risk::Approval;
use crate::sys;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// `claude agents --json`: every live session, its status and what it waits for. None when the reading
/// failed, so that a hiccup does not empty the board.
pub fn agents() -> Option<Vec<Json>> {
    let out = sys::output(Command::new("claude").args(["agents", "--json"]), Duration::from_secs(15))?;
    Some(Json::parse(&out).ok()?.items().to_vec())
}

/// Where the transcripts live: `$CLAUDE_CONFIG_DIR/projects`, else `~/.claude/projects`.
pub fn projects() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(|| sys::home().join(".claude"), PathBuf::from).join("projects")
}

fn list(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir).map(|d| d.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect()).unwrap_or_default();
    names.sort();
    names
}

/// `<projects>/<cwd, every character but letters and digits turned into "-">/<session id>.jsonl`, else
/// any project folder holding that session.
pub fn transcript_of(projects: &Path, id: &str, cwd: Option<&str>) -> Option<PathBuf> {
    if !(8..=64).contains(&id.len()) || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    let file = format!("{id}.jsonl");
    if let Some(cwd) = cwd {
        let guess = projects.join(cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>()).join(&file);
        if guess.exists() {
            return Some(guess);
        }
    }
    list(projects).into_iter().map(|d| projects.join(d).join(&file)).find(|f| f.exists())
}

// ---- caches: a live transcript only grows, so a result holds as long as the file size does ----

type Cache<T> = OnceLock<Mutex<HashMap<PathBuf, (u64, Arc<T>)>>>;

fn cached<T>(cache: &'static Cache<T>, file: &Path, compute: impl FnOnce(u64, Option<Arc<T>>) -> T) -> Option<Arc<T>> {
    let size = fs::metadata(file).ok()?.len();
    let map = cache.get_or_init(Default::default);
    let previous = {
        let map = map.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(file) {
            Some((s, v)) if *s == size => return Some(v.clone()),
            other => other.map(|(_, v)| v.clone()),
        }
    };
    let value = Arc::new(compute(size, previous));
    map.lock().unwrap_or_else(|e| e.into_inner()).insert(file.to_path_buf(), (size, value.clone()));
    Some(value)
}

/// The last `len` bytes of a file, as text (a cut character at the start becomes a replacement mark).
fn tail(file: &Path, size: u64, len: u64) -> String {
    let mut buf = Vec::new();
    let read = fs::File::open(file).and_then(|mut f| {
        f.seek(SeekFrom::Start(size.saturating_sub(len)))?;
        f.take(len).read_to_end(&mut buf)
    });
    if read.is_ok() { String::from_utf8_lossy(&buf).into_owned() } else { String::new() }
}

/// The model a session talks to now, from the end of its transcript.
/// ponytail: a text search in the last 256 KB; a full parse is for the detail view.
pub fn model_of(file: &Path) -> Option<String> {
    static CACHE: Cache<Option<String>> = OnceLock::new();
    let found = cached(&CACHE, file, |size, previous| {
        let text = tail(file, size, 256 * 1024);
        let at = text.rfind("\"model\":\"claude-").map(|i| i + "\"model\":\"".len());
        at.and_then(|i| text[i..].find('"').map(|j| text[i..i + j].to_string())).or_else(|| previous.and_then(|p| (*p).clone()))
    })?;
    (*found).clone()
}

/// The last thing the assistant said, from the end of the transcript: the text of a "done" card.
pub fn last_text_of(file: &Path) -> Option<String> {
    static CACHE: Cache<Option<String>> = OnceLock::new();
    let found = cached(&CACHE, file, |size, previous| {
        let text = tail(file, size, 256 * 1024);
        let last = text.lines().rev().filter(|l| l.contains("\"assistant\"")).filter_map(|l| Json::parse(l).ok()).find_map(|e| {
            let said: Vec<&str> = e.get("message").get("content").items().iter().filter(|c| c.get("type").str() == Some("text")).filter_map(|c| c.get("text").str()).collect();
            (e.get("type").str() == Some("assistant") && !said.is_empty()).then(|| said.join("\n")).filter(|t| !t.is_empty())
        });
        last.or_else(|| previous.and_then(|p| (*p).clone()))
    })?;
    (*found).clone()
}

// ---- transcripts ----

#[derive(Clone, Debug, Default)]
pub struct Step {
    pub at: String,
    pub tool: String,
    pub target: String,
    pub error: Option<String>,
}

impl Step {
    fn to_json(&self) -> Json {
        obj([("at", self.at.as_str().into()), ("tool", self.tool.as_str().into()), ("target", self.target.as_str().into()), ("error", self.error.as_deref().into())])
    }
}

#[derive(Clone, Debug, Default)]
pub struct Edit {
    pub at: String,
    pub file: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub agent: Option<String>,
    lines: (usize, usize), // lines added, removed
}

impl Edit {
    fn to_json(&self) -> Json {
        obj([
            ("at", self.at.as_str().into()),
            ("file", self.file.as_str().into()),
            ("before", self.before.as_deref().into()),
            ("after", self.after.as_deref().into()),
            ("agent", self.agent.as_deref().into()),
        ])
    }
}

#[derive(Clone, Debug, Default)]
struct Launch {
    at: String,
    name: Option<String>,
    script: Option<String>,
    script_path: Option<String>,
    run_id: Option<String>,
}

/// One entry of a session's thread: what the user typed, what the assistant said, a tool call and how
/// it ended.
#[derive(Clone, Debug)]
enum Item {
    User { at: String, text: String },
    Said { at: String, text: String },
    Tool { at: String, tool: String, target: String, out: Option<String>, error: bool },
}

impl Item {
    fn to_json(&self) -> Json {
        match self {
            Item::User { at, text } => obj([("kind", "user".into()), ("at", at.as_str().into()), ("text", text.as_str().into())]),
            Item::Said { at, text } => obj([("kind", "text".into()), ("at", at.as_str().into()), ("text", text.as_str().into())]),
            Item::Tool { at, tool, target, out, error } => obj([
                ("kind", "tool".into()),
                ("at", at.as_str().into()),
                ("tool", tool.as_str().into()),
                ("target", target.as_str().into()),
                ("out", out.as_deref().into()),
                ("error", error.then_some(true).into()),
            ]),
        }
    }
}

#[derive(Debug, Default)]
pub struct Transcript {
    pub title: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub last_text: Option<String>,
    pub prompts: Vec<(String, String)>,
    pub steps: Vec<Step>,
    pub edits: Vec<Edit>,
    launches: Vec<Launch>,
    thread: Vec<Item>,
    todos: Json, // the last TodoWrite list: the session's plan and where it is
}

const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// At most `n` characters, `…` when cut.
fn cut(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn cut_opt(v: &Json, n: usize) -> Option<String> {
    v.str().map(|s| cut(s, n))
}

/// The text of a message content: a string, or the `text` of each part.
fn text_of(content: &Json) -> String {
    match content {
        Json::Str(s) => s.clone(),
        Json::Arr(parts) => parts.iter().map(|c| c.get("text").str().unwrap_or("")).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn opt(v: &Json) -> Option<String> {
    v.str().map(String::from)
}

/// A quoted value after `key:` in a script, on the same line: `name: 'audit'`. The key must start a word.
fn quoted_after(script: &str, key: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (i, _) in script.match_indices(key) {
        if script[..i].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let rest = script[i + key.len()..].trim_start();
        let Some(rest) = rest.strip_prefix(':') else { continue };
        let rest = rest.trim_start();
        let Some(q) = rest.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) else { continue };
        let body = &rest[1..];
        if let Some(end) = body.find(q).filter(|&e| !body[..e].contains('\n')) {
            found.push(body[..end].to_string());
        }
    }
    found
}

/// The name a workflow script declares in `meta.name`.
fn meta_name(script: &str) -> Option<String> {
    quoted_after(script, "name").into_iter().next()
}

/// The phase titles a workflow script declares in `meta.phases`.
/// ponytail: a text search, not a JavaScript parser; titles must be plain string literals.
pub fn phases_of(script: &str) -> Vec<String> {
    let Some(start) = script.match_indices("phases").find_map(|(i, _)| script[i + 6..].trim_start().strip_prefix(':').map(|r| r.trim_start())).and_then(|r| r.strip_prefix('['))
    else {
        return Vec::new();
    };
    // The block ends at the first `]` followed by a comma, a brace or a new line.
    let end = start.match_indices(']').find(|(i, _)| start[i + 1..].trim_start_matches([' ', '\t', '\r']).starts_with([',', '}', '\n'])).map_or(start.len(), |(i, _)| i);
    quoted_after(&start[..end], "title")
}

/// What a tool call is about, in one line.
pub fn target(input: &Json) -> String {
    const KEYS: [&str; 11] = ["command", "file_path", "notebook_path", "url", "pattern", "query", "description", "prompt", "name", "skill", "scriptPath"];
    let t = KEYS
        .iter()
        .map(|k| input.get(k))
        .find(|v| !v.is_null())
        .map(|v| v.str().map_or_else(|| v.to_string(), String::from))
        .or_else(|| meta_name(input.get("script").str().unwrap_or("")))
        .unwrap_or_default();
    cut(&t.split_whitespace().collect::<Vec<_>>().join(" "), 200)
}

fn lines(file: &Path) -> Vec<Json> {
    fs::read_to_string(file).unwrap_or_default().lines().filter(|l| !l.is_empty()).filter_map(|l| Json::parse(l).ok()).collect()
}

fn line_count(v: &Json) -> usize {
    v.str().map_or(0, |s| s.lines().count())
}

pub fn read_transcript(file: &Path) -> Arc<Transcript> {
    static CACHE: Cache<Transcript> = OnceLock::new();
    cached(&CACHE, file, |_, _| parse(lines(file))).unwrap_or_default()
}

/// The end of a transcript (its last 512 KB), for what the board needs on every reading: the last turn
/// and the last steps, without parsing a long transcript in full every two seconds.
pub fn read_tail(file: &Path) -> Arc<Transcript> {
    static CACHE: Cache<Transcript> = OnceLock::new();
    cached(&CACHE, file, |size, _| {
        let text = tail(file, size, 512 * 1024);
        // The first line is cut unless the whole file fits.
        let skip = usize::from(size > 512 * 1024);
        parse(text.lines().skip(skip).filter(|l| !l.is_empty()).filter_map(|l| Json::parse(l).ok()).collect())
    })
    .unwrap_or_default()
}

fn parse(records: Vec<Json>) -> Transcript {
    let mut t = Transcript::default();
    let mut calls: HashMap<String, (usize, usize)> = HashMap::new(); // tool_use id -> step, thread item
    let mut launches: HashMap<String, usize> = HashMap::new();
    for e in records {
        let at = e.get("timestamp").str().unwrap_or("").to_string();
        if !at.is_empty() {
            t.start.get_or_insert_with(|| at.clone());
            t.end = Some(at.clone());
        }
        if let Some(b) = e.get("gitBranch").str() {
            t.branch = Some(b.into());
        }
        match e.get("type").str() {
            Some("ai-title") => t.title = opt(e.get("aiTitle")),
            Some("permission-mode") => t.mode = opt(e.get("permissionMode")),
            Some("assistant") => {
                let message = e.get("message");
                if let Some(m) = message.get("model").str().filter(|m| m.starts_with("claude-")) {
                    t.model = Some(m.into()); // not "<synthetic>" replays
                }
                for c in message.get("content").items() {
                    if c.get("type").str() == Some("text") {
                        if let Some(text) = c.get("text").str().filter(|s| !s.is_empty()) {
                            t.last_text = Some(text.into());
                            t.thread.push(Item::Said { at: at.clone(), text: cut(text, 4000) });
                        }
                    }
                    if c.get("type").str() != Some("tool_use") {
                        continue;
                    }
                    let (name, input) = (c.get("name").str().unwrap_or(""), c.get("input"));
                    let step = Step { at: at.clone(), tool: name.into(), target: target(input), error: None };
                    t.thread.push(Item::Tool { at: at.clone(), tool: name.into(), target: step.target.clone(), out: None, error: false });
                    calls.insert(c.get("id").str().unwrap_or("").into(), (t.steps.len(), t.thread.len() - 1));
                    t.steps.push(step);
                    if EDIT_TOOLS.contains(&name) {
                        let file = input.get("file_path").str().or(input.get("notebook_path").str()).unwrap_or("").to_string();
                        let parts = if name == "MultiEdit" { input.get("edits").items().to_vec() } else { vec![input.clone()] };
                        for p in parts {
                            let after =
                                if p.get("new_string").is_null() { if p.get("content").is_null() { p.get("new_source") } else { p.get("content") } } else { p.get("new_string") };
                            t.edits.push(Edit {
                                at: at.clone(),
                                file: file.clone(),
                                before: cut_opt(p.get("old_string"), 2000),
                                after: cut_opt(after, 2000),
                                agent: None,
                                lines: (line_count(after), line_count(p.get("old_string"))),
                            });
                        }
                    }
                    if name == "TodoWrite" {
                        t.todos = input.get("todos").clone();
                    }
                    if name == "Workflow" {
                        launches.insert(c.get("id").str().unwrap_or("").into(), t.launches.len());
                        t.launches.push(Launch {
                            at: at.clone(),
                            name: opt(input.get("name")),
                            script: opt(input.get("script")),
                            script_path: opt(input.get("scriptPath")),
                            run_id: None,
                        });
                    }
                }
            }
            Some("user") => {
                let content = e.get("message").get("content");
                let results: Vec<&Json> = content.items().iter().filter(|c| c.get("type").str() == Some("tool_result")).collect();
                for r in &results {
                    let id = r.get("tool_use_id").str().unwrap_or("");
                    let error = r.get("is_error").bool() == Some(true);
                    let out = text_of(r.get("content"));
                    if let Some(&(step, item)) = calls.get(id) {
                        if error {
                            t.steps[step].error = Some(cut(&out, 400));
                        }
                        if let Item::Tool { out: o, error: err, .. } = &mut t.thread[item] {
                            *o = Some(cut(out.trim(), 300)).filter(|s| !s.is_empty());
                            *err = error;
                        }
                    }
                    if let (Some(&l), Some(run)) = (launches.get(id), e.get("toolUseResult").get("runId").str()) {
                        t.launches[l].run_id = Some(run.into());
                    }
                }
                // What the user typed; injected context (reminders, command output) starts with a tag.
                let typed = if results.is_empty() { text_of(content).trim().to_string() } else { String::new() };
                if !typed.is_empty() && e.get("isMeta").bool() != Some(true) && !typed.starts_with('<') {
                    let text = cut(&typed, 2000);
                    t.prompts.push((at.clone(), text.clone()));
                    t.thread.push(Item::User { at: at.clone(), text });
                }
            }
            _ => {}
        }
    }
    t
}

// ---- subagents and workflows ----

#[derive(Clone, Debug, Default)]
pub struct Agent {
    pub id: String,
    pub label: String,
    pub kind: Option<String>,
    pub phase: Option<String>,
    pub model: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub tools: usize,
    pub errors: usize,
    pub running: bool,
    pub result: Option<String>,
}

impl Agent {
    fn to_json(&self) -> Json {
        obj([
            ("id", self.id.as_str().into()),
            ("label", self.label.as_str().into()),
            ("type", self.kind.as_deref().into()),
            ("phase", self.phase.as_deref().into()),
            ("model", self.model.as_deref().into()),
            ("start", self.start.as_deref().into()),
            ("end", self.end.as_deref().into()),
            ("tools", self.tools.into()),
            ("errors", self.errors.into()),
            ("running", self.running.into()),
            ("result", self.result.as_deref().into()),
        ])
    }
}

fn read_json(file: &Path) -> Json {
    fs::read_to_string(file).ok().and_then(|t| Json::parse(&t).ok()).unwrap_or_default()
}

// ponytail: "running" means no result yet and a transcript written in the last 10 minutes; an agent
// stuck longer in one tool call reads as stopped. Upgrade path: a SubagentStop seen by the hooks.
const RUNNING_MS: u64 = 10 * 60_000;

/// `result`: None when the agent has not returned, Some(Json::Null) when it returned nothing.
fn agent_of(dir: &Path, id: &str, label: Option<&str>, phase: Option<&str>, result: Option<&Json>) -> (Agent, Option<Arc<Transcript>>) {
    let meta = read_json(&dir.join(format!("agent-{id}.meta.json")));
    let file = dir.join(format!("agent-{id}.jsonl"));
    let t = file.exists().then(|| read_transcript(&file));
    let recent = t.as_ref().and_then(|t| t.end.as_deref()).and_then(sys::parse_iso).is_some_and(|end| sys::now_ms().saturating_sub(end) < RUNNING_MS);
    let agent = Agent {
        id: id.into(),
        label: label.map(String::from).or_else(|| opt(meta.get("description"))).or_else(|| opt(meta.get("agentType"))).unwrap_or_else(|| id.into()),
        kind: opt(meta.get("agentType")),
        phase: phase.map(String::from).or_else(|| opt(meta.get("workflowPhase"))),
        model: t.as_ref().and_then(|t| t.model.clone()),
        start: t.as_ref().and_then(|t| t.start.clone()),
        end: t.as_ref().and_then(|t| t.end.clone()),
        tools: t.as_ref().map_or(0, |t| t.steps.len()),
        errors: t.as_ref().map_or(0, |t| t.steps.iter().filter(|s| s.error.is_some()).count()),
        running: result.is_none() && meta.get("finished").bool() != Some(true) && recent,
        result: result.map(|r| cut(&r.str().map_or_else(|| r.to_string(), String::from), 300)),
    };
    (agent, t)
}

fn script_of(launch: Option<&Launch>) -> String {
    let Some(launch) = launch else { return String::new() };
    if let Some(s) = &launch.script {
        return s.clone();
    }
    let Some(path) = launch.script_path.as_deref() else { return String::new() };
    let ok = [".js", ".mjs", ".ts"].iter().any(|e| path.ends_with(e)) && fs::metadata(path).is_ok_and(|m| m.len() <= 1_000_000);
    if ok { fs::read_to_string(path).unwrap_or_default() } else { String::new() }
}

fn run_of(session_dir: &Path, run_id: &str, launch: Option<&Launch>, edits: &mut Vec<Edit>) -> Json {
    let record = read_json(&session_dir.join("workflows").join(format!("{run_id}.json")));
    let dir = session_dir.join("subagents").join("workflows").join(run_id);
    let journal = lines(&dir.join("journal.jsonl"));
    let results: HashMap<&str, &Json> = journal.iter().filter(|e| e.get("type").str() == Some("result")).filter_map(|e| Some((e.get("agentId").str()?, e.get("result")))).collect();
    let agents: Vec<Agent> = journal
        .iter()
        .filter(|e| e.get("type").str() == Some("started"))
        .filter_map(|e| {
            let id = e.get("agentId").str()?;
            let (agent, t) = agent_of(&dir, id, e.get("label").str(), e.get("phase").str(), results.get(id).copied());
            if let Some(t) = t {
                edits.extend(t.edits.iter().map(|x| Edit { agent: Some(agent.label.clone()), ..x.clone() }));
            }
            Some(agent)
        })
        .collect();
    // No record yet: still running, or stopped with agents that never returned.
    let status = record.get("status").str().map(String::from).unwrap_or_else(|| {
        if agents.iter().any(|a| a.running) {
            "running".into()
        } else if agents.iter().any(|a| a.result.is_none()) {
            "interrupted".into()
        } else {
            "ended".into()
        }
    });
    let declared: Vec<String> = record.get("phases").items().iter().filter_map(|p| p.get("title").str().filter(|t| !t.is_empty()).map(String::from)).collect();
    let name = opt(record.get("workflowName"))
        .or_else(|| launch.and_then(|l| l.name.clone()))
        .or_else(|| launch.and_then(|l| l.script.as_deref()).and_then(meta_name))
        .or_else(|| launch.and_then(|l| l.script_path.as_deref()).and_then(|p| Path::new(p).file_stem()?.to_str().map(String::from)))
        .unwrap_or_else(|| run_id.into());
    let start = record.get("startTime").num().map(|ms| sys::iso(ms as u64)).or_else(|| launch.map(|l| l.at.clone())).or_else(|| agents.first().and_then(|a| a.start.clone()));
    let phases = if declared.is_empty() { phases_of(&script_of(launch)) } else { declared };
    obj([
        ("runId", run_id.into()),
        ("name", name.into()),
        ("status", status.into()),
        ("start", start.into()),
        ("durationMs", record.get("durationMs").clone()),
        ("phases", Json::Arr(phases.into_iter().map(Json::from).collect())),
        ("current", agents.last().and_then(|a| a.phase.as_deref()).into()),
        ("agents", Json::Arr(agents.iter().map(Agent::to_json).collect())),
    ])
}

/// The same call failing at least three times in a row: (tool, target, count).
pub fn loops_of(steps: &[Step]) -> Vec<(String, String, usize)> {
    let mut loops = Vec::new();
    let mut run: Vec<&Step> = Vec::new();
    let end = Step::default();
    for s in steps.iter().chain(std::iter::once(&end)) {
        if s.error.is_some() && run.first().is_some_and(|f| f.tool == s.tool && f.target == s.target) {
            run.push(s);
            continue;
        }
        if run.len() >= 3 {
            loops.push((run[0].tool.clone(), run[0].target.clone(), run.len()));
        }
        run = if s.error.is_some() { vec![s] } else { Vec::new() };
    }
    loops
}

/// A loop going on now: the last steps end with the same call failing three times or more, and the
/// transcript moved in the last 15 minutes.
pub fn recent_loop(t: &Transcript, now: u64) -> Option<(String, String, usize)> {
    let fresh = t.end.as_deref().and_then(sys::parse_iso).is_some_and(|end| now.saturating_sub(end) < 15 * 60_000);
    let steps = last(&t.steps, 10);
    if !fresh || steps.last()?.error.is_none() {
        return None;
    }
    loops_of(steps).pop()
}

const TEST_COMMANDS: [&str; 13] =
    ["npm test", "npm run test", "pnpm test", "yarn test", "cargo test", "pytest", "go test", "node --test", "mvn test", "gradle test", "dotnet test", "vitest", "jest"];

/// What the last turn leaves to check: its last test run, commits, a merge request link, files changed.
/// ponytail: words in the commands and their output; a project with another test runner reads as none.
pub fn facts(t: &Transcript) -> Json {
    let start = t.thread.iter().rposition(|i| matches!(i, Item::User { .. })).map_or(0, |i| i + 1);
    let (mut tests, mut commits, mut mr, mut files) = (Json::Null, 0usize, None, Vec::new());
    for item in &t.thread[start..] {
        let Item::Tool { tool, target, out, error, .. } = item else { continue };
        let out = out.as_deref().unwrap_or("");
        if TEST_COMMANDS.iter().any(|c| target.contains(c)) {
            let line = out
                .lines()
                .find(|l| {
                    let l = l.to_lowercase();
                    l.contains("pass") || l.contains("fail")
                })
                .or(out.lines().next())
                .unwrap_or("");
            tests = obj([("ok", (!error).into()), ("summary", cut(line.trim(), 80).into())]);
        }
        if target.contains("git commit") && !error {
            commits += 1;
        }
        if let Some(url) = out
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/'))
            .find(|w| w.starts_with("http") && (w.contains("/merge_requests/") || w.contains("/pull/")))
        {
            mr = Some(url.to_string());
        }
        if EDIT_TOOLS.contains(&tool.as_str()) && !error && !files.contains(target) {
            files.push(target.clone());
        }
    }
    obj([("tests", tests), ("commits", commits.into()), ("mr", mr.into()), ("files", files.len().into())])
}

fn last<T>(v: &[T], n: usize) -> &[T] {
    &v[v.len().saturating_sub(n)..]
}

pub fn detail(projects: &Path, id: &str, cwd: Option<&str>) -> Option<Json> {
    let file = transcript_of(projects, id, cwd)?;
    let t = read_transcript(&file);
    let session_dir = file.with_extension("");
    let mut edits = t.edits.clone();
    let by_run: HashMap<&str, &Launch> = t.launches.iter().filter_map(|l| Some((l.run_id.as_deref()?, l))).collect();
    let mut run_ids: Vec<String> = t.launches.iter().filter_map(|l| l.run_id.clone()).collect();
    for r in list(&session_dir.join("subagents").join("workflows")) {
        if !run_ids.contains(&r) {
            run_ids.push(r);
        }
    }
    let mut runs: Vec<Json> = run_ids.iter().map(|r| run_of(&session_dir, r, by_run.get(r.as_str()).copied(), &mut edits)).collect();
    runs.sort_by_key(|r| r.get("start").to_string());
    let sub = session_dir.join("subagents");
    let agents: Vec<Json> = list(&sub)
        .iter()
        .filter_map(|f| f.strip_prefix("agent-")?.strip_suffix(".meta.json"))
        .filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(|id| {
            let (agent, transcript) = agent_of(&sub, id, None, None, None);
            if let Some(t) = transcript {
                edits.extend(t.edits.iter().map(|x| Edit { agent: Some(agent.label.clone()), ..x.clone() }));
            }
            agent.to_json()
        })
        .collect();
    edits.sort_by(|a, b| a.at.cmp(&b.at));
    // Lines added and removed per file, for the inspector.
    let mut files: Vec<(String, usize, usize)> = Vec::new();
    for e in &edits {
        match files.iter_mut().find(|f| f.0 == e.file) {
            Some(f) => (f.1, f.2) = (f.1 + e.lines.0, f.2 + e.lines.1),
            None => files.push((e.file.clone(), e.lines.0, e.lines.1)),
        }
    }
    let errors: Vec<&Step> = t.steps.iter().filter(|s| s.error.is_some()).collect();
    Some(obj([
        ("title", t.title.as_deref().into()),
        ("branch", t.branch.as_deref().into()),
        ("model", t.model.as_deref().into()),
        ("mode", t.mode.as_deref().into()),
        ("start", t.start.as_deref().into()),
        ("end", t.end.as_deref().into()),
        ("lastText", t.last_text.as_deref().into()),
        ("prompts", Json::Arr(last(&t.prompts, 100).iter().map(|(at, text)| obj([("at", at.as_str().into()), ("text", text.as_str().into())])).collect())),
        ("steps", Json::Arr(last(&t.steps, 300).iter().map(Step::to_json).collect())),
        ("edits", Json::Arr(last(&edits, 100).iter().map(Edit::to_json).collect())),
        ("files", Json::Arr(files.iter().map(|(f, plus, minus)| obj([("file", f.as_str().into()), ("plus", (*plus).into()), ("minus", (*minus).into())])).collect())),
        ("errors", Json::Arr(last(&errors, 50).iter().map(|s| s.to_json()).collect())),
        (
            "loops",
            Json::Arr(loops_of(&t.steps).into_iter().map(|(tool, target, count)| obj([("tool", tool.into()), ("target", target.into()), ("count", count.into())])).collect()),
        ),
        ("thread", Json::Arr(last(&t.thread, 200).iter().map(Item::to_json).collect())),
        ("todos", t.todos.clone()),
        ("runs", Json::Arr(runs)),
        ("agents", Json::Arr(agents)),
    ]))
}

// ---- approvals given by hand, for suggested rules ----

// Each tool result records how its call was allowed; `user_temporary` is a "yes, this once" in the
// terminal dialog. Only those lines and the tool calls are parsed, once per file size.
fn approvals_in(file: &Path) -> Arc<Vec<Approval>> {
    static CACHE: Cache<Vec<Approval>> = OnceLock::new();
    cached(&CACHE, file, |_, _| {
        let session = file.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let mut calls: HashMap<String, (String, Json)> = HashMap::new();
        let mut found = Vec::new();
        for line in fs::read_to_string(file).unwrap_or_default().lines() {
            if !line.contains("\"tool_use\"") && !line.contains("user_temporary") {
                continue;
            }
            let Ok(e) = Json::parse(line) else { continue };
            if e.get("type").str() == Some("assistant") {
                for c in e.get("message").get("content").items().iter().filter(|c| c.get("type").str() == Some("tool_use")) {
                    let input = obj([("command", c.get("input").get("command").clone()), ("url", c.get("input").get("url").clone())]);
                    calls.insert(c.get("id").str().unwrap_or("").into(), (c.get("name").str().unwrap_or("").into(), input));
                }
            }
            let d = e.get("permissionDecision");
            if e.get("type").str() != Some("user") || d.get("decision").str() != Some("accept") || d.get("source").str() != Some("user_temporary") {
                continue;
            }
            for r in e.get("message").get("content").items().iter().filter(|r| r.get("type").str() == Some("tool_result")) {
                if let Some((tool, input)) = calls.get(r.get("tool_use_id").str().unwrap_or("")) {
                    found.push(Approval { tool: tool.clone(), input: input.clone(), at: e.get("timestamp").str().unwrap_or("").into(), session: session.clone() });
                }
            }
        }
        found
    })
    .unwrap_or_default()
}

/// Every main transcript of every project.
/// ponytail: subagent transcripts are left out; approvals mostly happen in the main thread.
pub fn approvals(projects: &Path) -> Vec<Approval> {
    let mut all = Vec::new();
    for dir in list(projects) {
        for f in list(&projects.join(&dir)).iter().filter(|f| f.ends_with(".jsonl")) {
            all.extend(approvals_in(&projects.join(&dir).join(f)).iter().map(|a| Approval {
                tool: a.tool.clone(),
                input: a.input.clone(),
                at: a.at.clone(),
                session: a.session.clone(),
            }));
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0a1b2c3d-0000-4000-8000-000000000001";

    fn at(min: u64) -> String {
        sys::iso(sys::now_ms() - (60 - min) * 60_000)
    }

    fn jsonl(file: &Path, records: &[Json]) {
        fs::write(file, records.iter().map(|r| r.to_string() + "\n").collect::<String>()).unwrap();
    }

    fn j(text: &str) -> Json {
        Json::parse(text).unwrap()
    }

    fn use_(id: &str, name: &str, input: Json, min: u64, model: &str) -> Json {
        obj([
            ("type", "assistant".into()),
            ("timestamp", at(min).into()),
            ("gitBranch", "feature/tide".into()),
            (
                "message",
                obj([("model", model.into()), ("content", Json::Arr(vec![obj([("type", "tool_use".into()), ("id", id.into()), ("name", name.into()), ("input", input)])]))]),
            ),
        ])
    }

    fn result(id: &str, min: u64, is_error: bool) -> Json {
        obj([
            ("type", "user".into()),
            ("timestamp", at(min).into()),
            (
                "message",
                obj([(
                    "content",
                    Json::Arr(vec![obj([
                        ("type", "tool_result".into()),
                        ("tool_use_id", id.into()),
                        ("is_error", is_error.into()),
                        ("content", (if is_error { "Exit code 1" } else { "ok" }).into()),
                    ])]),
                )]),
            ),
        ])
    }

    // An invented Claude Code home: one project, one session that ran a two-phase workflow.
    fn home() -> PathBuf {
        let home = std::env::temp_dir().join(format!("clearance-test-{}", sys::random_hex(6).unwrap()));
        let project = home.join("projects").join("-work-orchard");
        let session = project.join(ID);
        let run = session.join("subagents").join("workflows").join("wf_test-001");
        fs::create_dir_all(&run).unwrap();
        fs::create_dir_all(session.join("workflows")).unwrap();
        let sonnet = "claude-sonnet-5-5";
        let mut workflow_result = result("t5", 12, false);
        workflow_result.set("toolUseResult", j(r#"{"status":"async_launched","runId":"wf_test-001"}"#));
        jsonl(
            &project.join(format!("{ID}.jsonl")),
            &[
                j(r#"{"type":"permission-mode","permissionMode":"auto"}"#),
                obj([("type", "user".into()), ("timestamp", at(0).into()), ("message", obj([("content", "<system-reminder>context</system-reminder>".into())]))]),
                obj([("type", "user".into()), ("timestamp", at(1).into()), ("isMeta", true.into()), ("message", obj([("content", "injected".into())]))]),
                obj([("type", "user".into()), ("timestamp", at(2).into()), ("message", obj([("content", "Ship the tide tables.".into())]))]),
                j(r#"{"type":"ai-title","aiTitle":"Tide tables"}"#),
                use_("t1", "Bash", j(r#"{"command":"npm test"}"#), 3, sonnet),
                result("t1", 4, true),
                use_("t2", "Bash", j(r#"{"command":"npm test"}"#), 5, sonnet),
                result("t2", 6, true),
                use_("t3", "Bash", j(r#"{"command":"npm test"}"#), 7, sonnet),
                result("t3", 8, true),
                use_("t4", "MultiEdit", j(r#"{"file_path":"src/tide.ts","edits":[{"old_string":"a","new_string":"b"},{"old_string":"c","new_string":"d"}]}"#), 9, sonnet),
                result("t4", 10, false),
                use_("t5", "Workflow", j(r#"{"scriptPath":"/nowhere/ship.js","args":{}}"#), 11, sonnet),
                workflow_result,
                use_("t6", "TodoWrite", j(r#"{"todos":[{"content":"Read the tables","status":"completed"},{"content":"Ship them","status":"in_progress"}]}"#), 13, sonnet),
            ],
        );
        let start = sys::now_ms() - 50 * 60_000;
        fs::write(
            session.join("workflows").join("wf_test-001.json"),
            format!(r#"{{"workflowName":"ship","status":"completed","startTime":{start},"durationMs":600000,"phases":[{{"title":"Build"}},{{"title":"Check"}}]}}"#),
        )
        .unwrap();
        jsonl(
            &run.join("journal.jsonl"),
            &[
                j(r#"{"type":"launched"}"#),
                j(r#"{"type":"started","agentId":"a1","label":"build","phase":"Build"}"#),
                j(r#"{"type":"result","agentId":"a1","result":{"ok":true}}"#),
                j(r#"{"type":"started","agentId":"a2","label":"check","phase":"Check"}"#),
            ],
        );
        fs::write(run.join("agent-a1.meta.json"), r#"{"agentType":"builder","workflowPhase":"Build"}"#).unwrap();
        jsonl(&run.join("agent-a1.jsonl"), &[use_("b1", "Write", j(r#"{"file_path":"dist/tide.json","content":"{}"}"#), 20, "claude-haiku-5-5"), result("b1", 21, false)]);
        fs::write(run.join("agent-a2.meta.json"), r#"{"agentType":"checker","workflowPhase":"Check"}"#).unwrap();
        jsonl(&run.join("agent-a2.jsonl"), &[use_("c1", "Read", j(r#"{"file_path":"dist/tide.json"}"#), 59, "claude-opus-5-5")]);
        fs::write(session.join("subagents").join("agent-x1.meta.json"), r#"{"agentType":"Explore","description":"find tide code","finished":true}"#).unwrap();
        jsonl(&session.join("subagents").join("agent-x1.jsonl"), &[use_("x", "Grep", j(r#"{"pattern":"tide"}"#), 30, "claude-haiku-5-5"), result("x", 31, false)]);
        home
    }

    #[test]
    fn finds_a_session_from_its_folder_and_refuses_odd_ids() {
        let home = home();
        let projects = home.join("projects");
        let file = projects.join("-work-orchard").join(format!("{ID}.jsonl"));
        assert_eq!(transcript_of(&projects, ID, Some("/work/orchard")), Some(file.clone()));
        assert_eq!(transcript_of(&projects, ID, None), Some(file.clone()));
        assert_eq!(transcript_of(&projects, "../etc", None), None);
        assert_eq!(model_of(&file).as_deref(), Some("claude-sonnet-5-5"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn session_header_prompts_errors_loops_edits_thread_and_plan() {
        let home = home();
        let d = detail(&home.join("projects"), ID, Some("/work/orchard")).unwrap();
        assert_eq!(d.get("title").str(), Some("Tide tables"));
        assert_eq!(d.get("branch").str(), Some("feature/tide"));
        assert_eq!(d.get("mode").str(), Some("auto"));
        let prompts: Vec<&str> = d.get("prompts").items().iter().filter_map(|p| p.get("text").str()).collect();
        assert_eq!(prompts, ["Ship the tide tables."]);
        assert_eq!(d.get("errors").items().len(), 3);
        assert_eq!(d.get("loops").to_string(), r#"[{"tool":"Bash","target":"npm test","count":3}]"#);
        let edits: Vec<String> = d
            .get("edits")
            .items()
            .iter()
            .map(|e| format!("{}:{}:{}", e.get("agent").str().unwrap_or("session"), e.get("file").str().unwrap(), e.get("after").str().unwrap()))
            .collect();
        assert_eq!(edits, ["session:src/tide.ts:b", "session:src/tide.ts:d", "build:dist/tide.json:{}"]);
        assert_eq!(d.get("files").to_string(), r#"[{"file":"src/tide.ts","plus":2,"minus":2},{"file":"dist/tide.json","plus":1,"minus":0}]"#);
        let thread: Vec<String> =
            d.get("thread").items().iter().take(3).map(|i| format!("{}:{}", i.get("kind").str().unwrap(), i.get("text").str().or(i.get("out").str()).unwrap_or(""))).collect();
        assert_eq!(thread, ["user:Ship the tide tables.", "tool:Exit code 1", "tool:Exit code 1"]);
        assert_eq!(d.get("thread").items()[1].get("error").bool(), Some(true));
        assert_eq!(d.get("todos").items()[1].get("status").str(), Some("in_progress"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn facts_of_the_last_turn_and_a_loop_going_on() {
        let home = home();
        let file = home.join("projects").join("-work-orchard").join(format!("{ID}.jsonl"));
        let t = read_tail(&file);
        assert_eq!(facts(&t).to_string(), r#"{"tests":{"ok":false,"summary":"Exit code 1"},"commits":0,"files":1}"#);
        assert_eq!(recent_loop(&t, sys::now_ms()), None); // the loop is over: steps went on since
        let steps: Vec<Step> = (0..3).map(|_| Step { at: String::new(), tool: "Bash".into(), target: "npm test".into(), error: Some("x".into()) }).collect();
        let looping = Transcript { steps, end: Some(sys::iso(sys::now_ms())), ..Transcript::default() };
        assert_eq!(recent_loop(&looping, sys::now_ms()), Some(("Bash".into(), "npm test".into(), 3)));
        assert_eq!(recent_loop(&looping, sys::now_ms() + 20 * 60_000), None); // stale
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn workflow_run_declared_phases_agents_current_phase() {
        let home = home();
        let d = detail(&home.join("projects"), ID, Some("/work/orchard")).unwrap();
        let r = &d.get("runs").items()[0];
        assert_eq!(r.get("name").str(), Some("ship"));
        assert_eq!(r.get("status").str(), Some("completed"));
        assert_eq!(r.get("phases").to_string(), r#"["Build","Check"]"#);
        assert_eq!(r.get("current").str(), Some("Check"));
        let agents: Vec<String> = r
            .get("agents")
            .items()
            .iter()
            .map(|a| format!("{}|{}|{}|{}|{}|{}", a.get("label"), a.get("type"), a.get("model"), a.get("tools"), a.get("running"), a.get("result")))
            .collect();
        assert_eq!(agents, [r#""build"|"builder"|"claude-haiku-5-5"|1|false|"{\"ok\":true}""#, r#""check"|"checker"|"claude-opus-5-5"|1|true|null"#]);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn subagents_outside_workflows() {
        let home = home();
        let d = detail(&home.join("projects"), ID, Some("/work/orchard")).unwrap();
        let agents: Vec<String> = d.get("agents").items().iter().map(|a| format!("{}|{}|{}", a.get("label"), a.get("type"), a.get("running"))).collect();
        assert_eq!(agents, [r#""find tide code"|"Explore"|false"#]);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn loops_need_three_failures_of_the_same_call_in_a_row() {
        let s = |target: &str, error: bool| Step { at: String::new(), tool: "Bash".into(), target: target.into(), error: error.then(|| "x".into()) };
        assert!(loops_of(&[s("a", true), s("a", true), s("b", true), s("a", true)]).is_empty());
        assert_eq!(loops_of(&[s("a", true), s("a", true), s("a", true), s("a", false), s("a", true)]), [("Bash".into(), "a".into(), 3)]);
    }

    #[test]
    fn phases_and_name_declared_in_a_workflow_script() {
        let script = "export const meta = {\n  name: 'audit',\n  phases: [{ title: 'Review' }, { title: \"Verify\", detail: 'all' }],\n}\n";
        assert_eq!(phases_of(script), ["Review", "Verify"]);
        assert!(phases_of("no meta").is_empty());
        assert_eq!(target(&obj([("script", script.into())])), "audit");
        assert_eq!(target(&obj([("command", "npm   test\n --watch".into())])), "npm test --watch");
    }

    #[test]
    fn approvals_given_by_hand() {
        let home = home();
        let file = home.join("projects").join("-work-orchard").join("0a1b2c3d-0000-4000-8000-000000000002.jsonl");
        let mut accepted = result("u1", 40, false);
        accepted.set("permissionDecision", j(r#"{"decision":"accept","source":"user_temporary"}"#));
        jsonl(&file, &[use_("u1", "Bash", j(r#"{"command":"npm test"}"#), 39, "claude-sonnet-5-5"), accepted]);
        let found = approvals(&home.join("projects"));
        assert_eq!(found.iter().map(|a| (a.tool.as_str(), a.input.get("command").str().unwrap_or(""))).collect::<Vec<_>>(), [("Bash", "npm test")]);
        let _ = fs::remove_dir_all(home);
    }
}
