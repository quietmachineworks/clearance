//! Board state: what waits for the user, what is done, who is working (BRIEF section 3, ADR-0003).
//! Pure: the server holds the connections and sends what `respond` builds.

use crate::json::{Json, obj};
use crate::risk::{Risk, classify, environment};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Permission,
    Question,
    Plan,
    Denied,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Permission => "permission",
            Kind::Question => "question",
            Kind::Plan => "plan",
            Kind::Denied => "denied",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub id: String,
    pub session: String,
    pub cwd: Option<String>,
    pub kind: Kind,
    pub at: u64,
    pub tool: String,
    pub input: Json,
    pub risk: Risk,
    pub suggestions: Vec<Json>, // Claude Code's own "always allow" proposal
    pub mode: Option<String>,   // permission mode when asked
    pub reason: Option<String>, // auto mode denial reason
    pub seen_waiting: bool,     // the session reported `waiting` since this request arrived
    pub env: Option<String>,    // infrastructure writes: the environment they reach (ADR-0012)
    pub env_pattern: Option<String>,
}

impl Request {
    pub fn to_json(&self) -> Json {
        obj([
            ("id", self.id.as_str().into()),
            ("session", self.session.as_str().into()),
            ("cwd", self.cwd.as_deref().into()),
            ("kind", self.kind.as_str().into()),
            ("at", self.at.into()),
            ("tool", self.tool.as_str().into()),
            ("input", self.input.clone()),
            ("risk", self.risk.as_str().into()),
            ("suggestions", Json::Arr(self.suggestions.clone())),
            ("mode", self.mode.as_deref().into()),
            ("reason", self.reason.as_deref().into()),
            ("seenWaiting", self.seen_waiting.into()),
            ("env", self.env.as_deref().into()),
            ("envPattern", self.env_pattern.as_deref().into()),
        ])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub session: String,
    pub cwd: Option<String>,
    pub at: u64,
    pub message: String,
    pub failed: bool,
}

/// An instruction typed in clearance, delivered once by the session's companion mod.
#[derive(Clone, Debug)]
pub struct Instruction {
    pub text: String,
    pub at: u64,
    pub delivered: Option<u64>,
}

#[derive(Default)]
pub struct Board {
    pub requests: Vec<Request>,
    pub sessions: Vec<Json>, // the entries of `claude agents --json`, plus model, lastActive, lastText
    pub done: HashMap<String, Done>,
    pub agents: HashMap<String, HashSet<String>>, // session -> running subagents
    pub usage: HashMap<String, (u64, Json)>,      // session -> last report of its companion mod
    pub inbox: HashMap<String, Instruction>,      // session -> last instruction typed for it
    pub connected: HashSet<String>,               // sessions that sent a hook event
    pub environments: Json,                       // the user's patterns, ~/.clearance/environments.json
    settled: Vec<(u64, Request)>,                 // requests answered in the terminal, shown for a while
    costs: HashMap<String, Vec<(u64, f64)>>,      // session -> recent (time, cost in USD)
    quota: Vec<(u64, f64)>,                       // recent (time, 5-hour quota used in %)
    seq: u64,
}

// How long a request answered elsewhere stays in view, and the span the rates are measured over.
const SETTLED_MS: u64 = 120_000;
const RATE_SPAN_MS: u64 = 60 * 60_000;
const RATE_MIN_MS: u64 = 5 * 60_000;

/// Keeps the samples of the last hour, adds this one, and gives the change per millisecond once they span
/// five minutes. A fall (a new quota window) starts over.
fn rate(samples: &mut Vec<(u64, f64)>, now: u64, value: f64) -> Option<f64> {
    if samples.last().is_some_and(|&(_, v)| value < v) {
        samples.clear();
    }
    samples.retain(|&(at, _)| now.saturating_sub(at) <= RATE_SPAN_MS);
    samples.push((now, value));
    let (&(t0, v0), &(t1, v1)) = (samples.first()?, samples.last()?);
    (t1 - t0 >= RATE_MIN_MS).then(|| (v1 - v0) / (t1 - t0) as f64)
}

fn id_of(s: &Json) -> &str {
    s.get("sessionId").str().unwrap_or("")
}

fn opt(v: &Json) -> Option<String> {
    v.str().map(String::from)
}

// Plan approvals come as AskUserQuestion with a GO option (BRIEF section 3).
fn is_go(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("go")
}

impl Board {
    pub fn session(&self, id: &str) -> Option<&Json> {
        self.sessions.iter().find(|s| id_of(s) == id)
    }

    pub fn request(&self, id: &str) -> Option<&Request> {
        self.requests.iter().find(|r| r.id == id)
    }

    /// What the companion mod reports: `$.session.usage()` (ADR-0004), stamped on arrival.
    pub fn report(&mut self, session: &str, usage: &Json, now: u64) {
        let mut kept = obj([("context", usage.get("context").clone()), ("rateLimits", usage.get("rateLimits").clone()), ("cost", usage.get("cost").clone())]);
        if let Some(usd) = usage.get("cost").get("usd").num() {
            let per_ms = rate(self.costs.entry(session.to_string()).or_default(), now, usd);
            kept.set("rate", per_ms.map(|r| r * 3_600_000.0).into()); // USD per hour
        }
        if let Some(pct) = usage.get("rateLimits").items().iter().find(|l| l.get("kind").str() == Some("five_hour")).and_then(|l| l.get("percentUsed").num()) {
            rate(&mut self.quota, now, pct);
        }
        self.usage.insert(session.to_string(), (now, kept));
    }

    /// When the 5-hour quota runs dry at the pace of the last hour, if it rises.
    pub fn quota_dry_at(&self) -> Option<u64> {
        let (&(t0, v0), &(t1, v1)) = (self.quota.first()?, self.quota.last()?);
        let per_ms = (v1 - v0) / (t1.checked_sub(t0).filter(|&d| d >= RATE_MIN_MS)? as f64);
        (per_ms > 0.0).then(|| t1 + ((100.0 - v1).max(0.0) / per_ms) as u64)
    }

    pub fn instruct(&mut self, session: &str, text: &str, now: u64) {
        self.inbox.insert(session.to_string(), Instruction { text: text.to_string(), at: now, delivered: None });
    }

    /// The instruction waiting for this session, handed out once.
    pub fn take(&mut self, session: &str, now: u64) -> Option<String> {
        let i = self.inbox.get_mut(session).filter(|i| i.delivered.is_none())?;
        i.delivered = Some(now);
        Some(i.text.clone())
    }

    /// A hook event arrived: the request to hold, if any, and the requests it ends.
    pub fn hook(&mut self, event: &str, body: &Json, now: u64) -> (Option<Request>, Vec<String>) {
        let session = body.get("session_id").str().unwrap_or("").to_string();
        let tool = body.get("tool_name").str().unwrap_or("").to_string();
        let input = if body.get("tool_input").is_null() { Json::Obj(Vec::new()) } else { body.get("tool_input").clone() };
        if !session.is_empty() {
            self.connected.insert(session.clone());
        }
        let new = |board: &mut Board, kind, risk, suggestions, reason| {
            board.seq += 1;
            Request {
                id: board.seq.to_string(),
                session: session.clone(),
                cwd: opt(body.get("cwd")),
                kind,
                at: now,
                tool: tool.clone(),
                input: input.clone(),
                risk,
                suggestions,
                mode: opt(body.get("permission_mode")),
                reason,
                seen_waiting: false,
                env: None,
                env_pattern: None,
            }
        };
        let mut ended = Vec::new();
        let request = match event {
            // Questions come through PermissionRequest too, in parallel with the terminal dialog. A held
            // PreToolUse hook would hide that dialog until it answers (observed in M1, ADR-0005).
            "PermissionRequest" if tool == "AskUserQuestion" => {
                let plan = input.get("questions").items().iter().any(|q| q.get("options").items().iter().any(|o| is_go(o.get("label").str().unwrap_or(""))));
                Some(if plan { new(self, Kind::Plan, Risk::Infra, Vec::new(), None) } else { new(self, Kind::Question, Risk::Read, Vec::new(), None) })
            }
            "PermissionRequest" => Some(new(self, Kind::Permission, classify(&tool, &input), body.get("permission_suggestions").items().to_vec(), None)),
            "PermissionDenied" => Some(new(self, Kind::Denied, classify(&tool, &input), Vec::new(), opt(body.get("reason")))),
            "Stop" | "StopFailure" => {
                ended = self.drop(&session, now);
                let failed = event == "StopFailure";
                let message =
                    opt(body.get("last_assistant_message")).or_else(|| opt(body.get("error"))).unwrap_or_else(|| if failed { "Stopped on error.".into() } else { String::new() });
                self.done.insert(session.clone(), Done { session: session.clone(), cwd: opt(body.get("cwd")), at: now, message, failed });
                None
            }
            "UserPromptSubmit" => {
                ended = self.drop(&session, now);
                self.done.remove(&session);
                None
            }
            // An empty type is an internal agent.
            "SubagentStart" if body.get("agent_type").str().is_some_and(|t| !t.is_empty()) => {
                self.agents.entry(session.clone()).or_default().insert(body.get("agent_id").str().unwrap_or("").to_string());
                None
            }
            "SubagentStop" => {
                if let Some(set) = self.agents.get_mut(&session) {
                    set.remove(body.get("agent_id").str().unwrap_or(""));
                }
                None
            }
            _ => None,
        };
        let request = request.map(|mut r| {
            if r.risk == Risk::Infra && matches!(r.kind, Kind::Permission | Kind::Denied) {
                let (env, pattern) = environment(&r.input, &self.environments);
                (r.env, r.env_pattern) = (Some(env), pattern);
            }
            self.requests.push(r.clone());
            r
        });
        (request, ended)
    }

    /// A new reading of `claude agents --json`: the requests whose session stopped waiting, i.e. answered
    /// in the terminal or gone.
    /// ponytail: per session, not per request. With parallel subagents, a request answered in the terminal
    /// while a sibling still waits stays on the board until the turn ends; answering it is harmless.
    pub fn sync(&mut self, list: Vec<Json>, now: u64) -> Vec<String> {
        let before = std::mem::replace(&mut self.sessions, list);
        let gone = |id: &str, now_list: &[Json]| before.iter().any(|s| id_of(s) == id) && !now_list.iter().any(|s| id_of(s) == id);
        let list = &self.sessions;
        self.done.retain(|id, _| !gone(id, list));
        self.agents.retain(|id, _| !gone(id, list));
        self.usage.retain(|id, _| !gone(id, list));
        self.inbox.retain(|id, _| !gone(id, list));
        self.connected.retain(|id| !gone(id, list));

        // A turn that ended (busy or waiting, now idle) is done until read; a new turn (idle, now busy)
        // means the user typed or an instruction came in. Read off the status rather than from Stop and
        // UserPromptSubmit hooks, whose errors show in every session while clearance is stopped (ADR-0009).
        let mut ended = Vec::new();
        for s in self.sessions.clone() {
            let id = id_of(&s);
            let was = before.iter().find(|b| id_of(b) == id).and_then(|b| b.get("status").str());
            let status = s.get("status").str();
            if status == Some("idle") && matches!(was, Some("busy" | "waiting")) {
                if !self.done.get(id).is_some_and(|d| d.failed) {
                    self.done.insert(
                        id.to_string(),
                        Done { session: id.to_string(), cwd: opt(s.get("cwd")), at: now, message: opt(s.get("lastText")).unwrap_or_default(), failed: false },
                    );
                }
                ended.extend(self.drop(id, now));
            } else if status == Some("busy") && was == Some("idle") {
                self.done.remove(id);
            }
        }
        for r in &mut self.requests {
            if self.sessions.iter().any(|s| id_of(s) == r.session && s.get("status").str() == Some("waiting")) {
                r.seen_waiting = true;
            } else if r.seen_waiting {
                ended.push(r.id.clone());
            }
        }
        let (gone, kept): (Vec<Request>, Vec<Request>) = std::mem::take(&mut self.requests).into_iter().partition(|r| ended.contains(&r.id));
        self.requests = kept;
        self.settled.extend(gone.into_iter().map(|r| (now, r)));
        self.settled.retain(|(at, _)| now.saturating_sub(*at) < SETTLED_MS);
        ended
    }

    /// A held request whose hook call Claude Code closed: answered in the terminal, or timed out.
    pub fn settle(&mut self, id: &str, now: u64) {
        if let Some(i) = self.requests.iter().position(|r| r.id == id) {
            let r = self.requests.remove(i);
            self.settled.push((now, r));
        }
    }

    pub fn end(&mut self, id: &str) {
        self.requests.retain(|r| r.id != id);
    }

    pub fn read(&mut self, session: &str) {
        self.done.remove(session);
    }

    // The turn moved on without the page: the session's requests were settled in the terminal.
    fn drop(&mut self, session: &str, now: u64) -> Vec<String> {
        let (gone, kept): (Vec<Request>, Vec<Request>) = std::mem::take(&mut self.requests).into_iter().partition(|r| r.session == session);
        self.requests = kept;
        let ids = gone.iter().map(|r| r.id.clone()).collect();
        self.settled.extend(gone.into_iter().map(|r| (now, r)));
        ids
    }

    pub fn snapshot(&self) -> Json {
        let mut requests: Vec<&Request> = self.requests.iter().collect();
        requests.sort_by(|a, b| b.risk.cmp(&a.risk).then(a.at.cmp(&b.at)));
        let sessions = self
            .sessions
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s.remove("lastText"); // not sent to the page with every update
                let id = id_of(&s).to_string();
                s.set("agents", self.agents.get(&id).map_or(0, HashSet::len).into());
                if let Some((at, u)) = self.usage.get(&id) {
                    let mut u = u.clone();
                    u.set("at", (*at).into());
                    s.set("usage", u);
                }
                if let Some(i) = self.inbox.get(&id) {
                    s.set("instruction", obj([("text", i.text.as_str().into()), ("at", i.at.into()), ("delivered", i.delivered.into())]));
                }
                s.set("connected", self.connected.contains(&id).into());
                s
            })
            .collect();
        let mut done: Vec<&Done> = self.done.values().collect();
        done.sort_by_key(|d| d.at);
        // The account's quota is the same for every session: the freshest report wins.
        let quota = self.usage.values().filter(|(_, u)| !u.get("rateLimits").items().is_empty()).max_by_key(|(at, _)| *at).map(|(_, u)| u.get("rateLimits").clone());
        obj([
            ("requests", Json::Arr(requests.iter().map(|r| r.to_json()).collect())),
            ("sessions", Json::Arr(sessions)),
            (
                "done",
                Json::Arr(
                    done.iter()
                        .map(|d| {
                            obj([
                                ("session", d.session.as_str().into()),
                                ("cwd", d.cwd.as_deref().into()),
                                ("at", d.at.into()),
                                ("message", d.message.as_str().into()),
                                ("failed", d.failed.then_some(true).into()),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("quota", quota.into()),
            ("quotaDryAt", self.quota_dry_at().into()),
            (
                "settled",
                Json::Arr(
                    self.settled
                        .iter()
                        .map(|(at, r)| {
                            let mut j = r.to_json();
                            j.set("settledAt", (*at).into());
                            j
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// The hook answer for a decision taken on the page; an error for a decision the request does not allow.
/// The decision: `action` (allow, rule, deny, answer, retry, dismiss), `reason`, `mode`, `answers`, `text`,
/// `typed`.
pub fn respond(r: &Request, d: &Json) -> Result<Json, String> {
    let action = d.get("action").str().unwrap_or("");
    let filled = |key: &str| d.get(key).str().filter(|s| !s.is_empty());
    let typed_go = d.get("typed").str() == Some("GO");
    if action == "dismiss" {
        return Ok(Json::Obj(Vec::new())); // no decision: the terminal dialog stays
    }
    let out = |event: &str, rest: Vec<(String, Json)>| {
        let mut inner = vec![("hookEventName".to_string(), Json::from(event))];
        inner.extend(rest);
        obj([("hookSpecificOutput", Json::Obj(inner))])
    };
    let decision = |d: Json| out("PermissionRequest", vec![("decision".into(), d)]);
    let deny = |message: &str| decision(obj([("behavior", "deny".into()), ("message", message.into())]));

    match r.kind {
        Kind::Permission => {
            if action == "deny" {
                return Ok(deny(filled("reason").unwrap_or("Denied from clearance.")));
            }
            if action == "rule" && (r.risk == Risk::Infra || r.suggestions.is_empty()) {
                return Err("no rule can be added for this request".into());
            }
            if action != "allow" && action != "rule" {
                return Err(format!("{action} does not apply to a permission"));
            }
            if r.risk == Risk::Infra && !typed_go {
                return Err("type GO to allow an infrastructure write".into());
            }
            let mut updates = if action == "rule" { r.suggestions.clone() } else { Vec::new() };
            if let Some(mode) = filled("mode") {
                updates.push(obj([("type", "setMode".into()), ("mode", mode.into()), ("destination", "session".into())]));
            }
            Ok(decision(obj([("behavior", "allow".into()), ("updatedPermissions", (!updates.is_empty()).then_some(Json::Arr(updates)).into())])))
        }
        Kind::Question | Kind::Plan => {
            if action == "deny" {
                return Ok(deny(filled("reason").unwrap_or("The user declined to answer.")));
            }
            if action != "answer" {
                return Err(format!("{action} does not apply to a question"));
            }
            // A free answer goes back as the message of a deny: AskUserQuestion only accepts option labels.
            if let Some(text) = filled("text") {
                return Ok(deny(&format!("The user answered: {text}")));
            }
            let answers = if d.get("answers").is_null() { Json::Obj(Vec::new()) } else { d.get("answers").clone() };
            for q in r.input.get("questions").items() {
                let question = q.get("question").str().unwrap_or("");
                let labels: Vec<&str> = q.get("options").items().iter().filter_map(|o| o.get("label").str()).collect();
                let picked = answers.get(question).str().unwrap_or("");
                let picked: Vec<&str> = picked.split(", ").collect();
                if !picked.iter().all(|l| labels.contains(l)) {
                    return Err(format!("no valid answer for \"{question}\""));
                }
                if r.kind == Kind::Plan && picked.iter().any(|l| is_go(l)) && !typed_go {
                    return Err("type GO to approve a plan".into());
                }
            }
            let mut input = r.input.clone();
            input.set("answers", answers);
            Ok(decision(obj([("behavior", "allow".into()), ("updatedInput", input)])))
        }
        Kind::Denied if action == "retry" => Ok(out("PermissionDenied", vec![("retry".into(), true.into())])),
        Kind::Denied => Err(format!("{action} does not apply to an auto mode denial")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "sess-orchard";

    fn j(text: &str) -> Json {
        Json::parse(text).unwrap()
    }

    fn permission(command: &str) -> Json {
        obj([("session_id", S.into()), ("tool_name", "Bash".into()), ("tool_input", obj([("command", command.into())])), ("permission_mode", "default".into())])
    }

    fn ask(labels: &[&str]) -> Json {
        let options = labels.iter().map(|l| obj([("label", (*l).into())])).collect();
        obj([
            ("session_id", S.into()),
            ("tool_name", "AskUserQuestion".into()),
            (
                "tool_input",
                obj([(
                    "questions",
                    Json::Arr(vec![obj([("question", "Ship the plan?".into()), ("header", "Plan".into()), ("multiSelect", false.into()), ("options", Json::Arr(options))])]),
                )]),
            ),
        ])
    }

    fn status(id: &str, status: &str) -> Json {
        obj([("sessionId", id.into()), ("status", status.into())])
    }

    #[test]
    fn requests_are_sorted_by_risk_then_age() {
        let mut b = Board::default();
        for (i, c) in ["npm test", "ls", "git push origin main", "npm run build"].iter().enumerate() {
            b.hook("PermissionRequest", &permission(c), i as u64 + 1);
        }
        let snap = b.snapshot();
        let got: Vec<&str> = snap.get("requests").items().iter().map(|r| r.get("input").get("command").str().unwrap()).collect();
        assert_eq!(got, ["git push origin main", "npm test", "npm run build", "ls"]);
    }

    #[test]
    fn a_request_leaves_once_its_session_stops_waiting_not_before_it_was_seen_waiting() {
        let mut b = Board::default();
        let (r, _) = b.hook("PermissionRequest", &permission("npm test"), 1);
        assert!(b.sync(vec![status(S, "busy")], 2).is_empty()); // the hook arrives before the poll shows it
        assert!(b.sync(vec![obj([("sessionId", S.into()), ("status", "waiting".into()), ("waitingFor", "permission prompt".into())])], 3).is_empty());
        assert_eq!(b.sync(vec![status(S, "busy")], 4), [r.unwrap().id]); // answered in the terminal
        assert!(b.requests.is_empty());
    }

    #[test]
    fn stop_ends_the_session_requests_and_marks_it_done_until_read_or_a_new_prompt() {
        let mut b = Board::default();
        let (r, _) = b.hook("PermissionRequest", &permission("npm test"), 1);
        let (_, ended) = b.hook("Stop", &obj([("session_id", S.into()), ("last_assistant_message", "All green.".into())]), 2);
        assert_eq!(ended, [r.unwrap().id]);
        assert_eq!(b.done[S].message, "All green.");
        b.hook("UserPromptSubmit", &obj([("session_id", S.into()), ("prompt", "next".into())]), 3);
        assert!(b.done.is_empty());
        b.hook("Stop", &obj([("session_id", S.into()), ("last_assistant_message", "Done again.".into())]), 4);
        b.read(S);
        assert!(b.done.is_empty());
    }

    #[test]
    fn subagents_are_counted_internal_ones_ignored() {
        let mut b = Board::default();
        b.hook("SubagentStart", &obj([("session_id", S.into()), ("agent_id", "a1".into()), ("agent_type", "reviewer".into())]), 1);
        b.hook("SubagentStart", &obj([("session_id", S.into()), ("agent_id", "a2".into()), ("agent_type", "".into())]), 1);
        b.sync(vec![status(S, "busy")], 2);
        assert_eq!(b.snapshot().get("sessions").items()[0].get("agents").num(), Some(1.0));
        b.hook("SubagentStop", &obj([("session_id", S.into()), ("agent_id", "a1".into())]), 3);
        assert_eq!(b.snapshot().get("sessions").items()[0].get("agents").num(), Some(0.0));
    }

    #[test]
    fn permission_answers() {
        let mut b = Board::default();
        let suggestion = j(r#"{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"npm test:*"}],"behavior":"allow","destination":"localSettings"}"#);
        let mut body = permission("npm test");
        body.set("permission_suggestions", Json::Arr(vec![suggestion.clone()]));
        let r = b.hook("PermissionRequest", &body, 1).0.unwrap();
        assert_eq!(
            respond(&r, &j(r#"{"action":"allow"}"#)).unwrap().to_string(),
            r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#
        );
        let rule = respond(&r, &j(r#"{"action":"rule","mode":"acceptEdits"}"#)).unwrap();
        assert_eq!(
            rule.get("hookSpecificOutput").get("decision").get("updatedPermissions").items(),
            [suggestion, j(r#"{"type":"setMode","mode":"acceptEdits","destination":"session"}"#)]
        );
        let deny = respond(&r, &j(r#"{"action":"deny","reason":"use the test script"}"#)).unwrap();
        assert_eq!(deny.get("hookSpecificOutput").get("decision").get("message").str(), Some("use the test script"));
        assert_eq!(respond(&r, &j(r#"{"action":"dismiss"}"#)).unwrap().to_string(), "{}");
    }

    #[test]
    fn an_infrastructure_write_needs_go_typed_and_never_becomes_a_rule() {
        let mut b = Board::default();
        let mut body = permission("git push --force");
        body.set("permission_suggestions", j(r#"[{"type":"addRules"}]"#));
        let r = b.hook("PermissionRequest", &body, 1).0.unwrap();
        assert!(respond(&r, &j(r#"{"action":"rule","typed":"GO"}"#)).unwrap_err().contains("no rule"));
        assert!(respond(&r, &j(r#"{"action":"allow"}"#)).unwrap_err().contains("type GO"));
        assert_eq!(respond(&r, &j(r#"{"action":"allow","typed":"GO"}"#)).unwrap().get("hookSpecificOutput").get("decision").get("behavior").str(), Some("allow"));
        assert!(respond(&r, &j(r#"{"action":"deny"}"#)).is_ok());
    }

    #[test]
    fn questions_take_option_labels_a_free_answer_goes_back_as_a_deny_message() {
        let mut b = Board::default();
        let r = b.hook("PermissionRequest", &ask(&["Red", "Blue"]), 1).0.unwrap();
        assert_eq!(r.kind, Kind::Question);
        let allow = respond(&r, &j(r#"{"action":"answer","answers":{"Ship the plan?":"Blue"}}"#)).unwrap();
        assert_eq!(allow.get("hookSpecificOutput").get("decision").get("behavior").str(), Some("allow"));
        assert_eq!(allow.get("hookSpecificOutput").get("decision").get("updatedInput").get("answers").to_string(), r#"{"Ship the plan?":"Blue"}"#);
        assert!(respond(&r, &j(r#"{"action":"answer","answers":{"Ship the plan?":"Green"}}"#)).unwrap_err().contains("no valid answer"));
        let free = respond(&r, &j(r#"{"action":"answer","text":"Green, please"}"#)).unwrap();
        assert_eq!(free.get("hookSpecificOutput").get("decision").get("behavior").str(), Some("deny"));
        assert!(free.get("hookSpecificOutput").get("decision").get("message").str().unwrap().contains("Green, please"));
    }

    #[test]
    fn a_plan_needs_go_typed() {
        let mut b = Board::default();
        let r = b.hook("PermissionRequest", &ask(&["GO", "No"]), 1).0.unwrap();
        assert_eq!(r.kind, Kind::Plan);
        assert_eq!(b.snapshot().get("requests").items()[0].get("risk").str(), Some("infra"));
        assert!(respond(&r, &j(r#"{"action":"answer","answers":{"Ship the plan?":"GO"}}"#)).unwrap_err().contains("type GO"));
        assert!(respond(&r, &j(r#"{"action":"answer","answers":{"Ship the plan?":"GO"},"typed":"go"}"#)).unwrap_err().contains("type GO"));
        assert!(respond(&r, &j(r#"{"action":"answer","answers":{"Ship the plan?":"GO"},"typed":"GO"}"#)).is_ok());
        assert!(respond(&r, &j(r#"{"action":"answer","answers":{"Ship the plan?":"No"}}"#)).is_ok());
    }

    #[test]
    fn auto_mode_denials_can_be_retried() {
        let mut b = Board::default();
        let body =
            obj([("session_id", S.into()), ("tool_name", "Bash".into()), ("tool_input", obj([("command", "git push --force".into())])), ("reason", "[Git Destructive]".into())]);
        let r = b.hook("PermissionDenied", &body, 1).0.unwrap();
        assert_eq!(r.reason.as_deref(), Some("[Git Destructive]"));
        assert_eq!(respond(&r, &j(r#"{"action":"retry"}"#)).unwrap().to_string(), r#"{"hookSpecificOutput":{"hookEventName":"PermissionDenied","retry":true}}"#);
        assert!(respond(&r, &j(r#"{"action":"allow"}"#)).is_err());
    }

    #[test]
    fn companion_mod_usage_per_session_freshest_quota_instructions_delivered_once() {
        let mut b = Board::default();
        b.sync(vec![status(S, "busy"), status("sess-quarry", "idle")], 1);
        b.report(S, &j(r#"{"context":{"window":200000,"percent":91},"rateLimits":[{"kind":"five_hour","percentUsed":40}],"cost":{"usd":1.5}}"#), 100);
        b.report("sess-quarry", &j(r#"{"context":{"window":200000,"percent":10},"rateLimits":[{"kind":"five_hour","percentUsed":42}]}"#), 200);
        let snap = b.snapshot();
        let mine = snap.get("sessions").items().iter().find(|s| s.get("sessionId").str() == Some(S)).unwrap().clone();
        assert_eq!(mine.get("usage").get("context").get("percent").num(), Some(91.0));
        assert_eq!(snap.get("quota").to_string(), r#"[{"kind":"five_hour","percentUsed":42}]"#);
        assert_eq!(b.take(S, 250), None);
        b.instruct(S, "Run the linter too.", 300);
        assert_eq!(b.take(S, 310).as_deref(), Some("Run the linter too."));
        assert_eq!(b.take(S, 320), None);
        assert_eq!(b.inbox[S].delivered, Some(310));
        b.sync(vec![status("sess-quarry", "idle")], 400); // the session ended
        assert!(!b.usage.contains_key(S));
        assert!(!b.inbox.contains_key(S));
    }

    #[test]
    fn infrastructure_writes_carry_their_environment_and_settled_requests_stay_in_view() {
        let mut b = Board { environments: j(r#"{"prod":["db-prod"]}"#), ..Board::default() };
        let (prod, _) = b.hook("PermissionRequest", &permission("psql -h db-prod -c 'vacuum'"), 1);
        let (unknown, _) = b.hook("PermissionRequest", &permission("ssh bastion"), 1);
        let (local, _) = b.hook("PermissionRequest", &permission("npm test"), 1);
        assert_eq!((prod.unwrap().env.as_deref(), unknown.unwrap().env.as_deref(), local.unwrap().env), (Some("prod"), Some("unknown"), None));
        b.sync(vec![obj([("sessionId", S.into()), ("status", "waiting".into())])], 2);
        let ended = b.sync(vec![status(S, "busy")], 3); // answered in the terminal
        assert_eq!(ended.len(), 3);
        assert_eq!(b.snapshot().get("settled").items().len(), 3);
        b.sync(vec![status(S, "busy")], 3 + SETTLED_MS);
        assert!(b.snapshot().get("settled").items().is_empty());
    }

    #[test]
    fn cost_rate_and_quota_running_dry() {
        let mut b = Board::default();
        let usage = |usd: f64, pct: f64| j(&format!(r#"{{"cost":{{"usd":{usd}}},"rateLimits":[{{"kind":"five_hour","percentUsed":{pct}}}]}}"#));
        b.report(S, &usage(1.0, 40.0), 0);
        assert!(b.usage[S].1.get("rate").is_null()); // one sample is no rate
        b.report(S, &usage(1.5, 50.0), 30 * 60_000);
        assert!(b.usage[S].1.get("rate").num().is_some_and(|r| (r - 1.0).abs() < 1e-9)); // 0.5 USD in half an hour
        assert_eq!(b.quota_dry_at(), Some(30 * 60_000 + 150 * 60_000)); // 10 points per half hour, 50 left
        b.report(S, &usage(1.6, 2.0), 31 * 60_000); // a new window: the measure starts over
        assert_eq!(b.quota_dry_at(), None);
    }

    #[test]
    fn a_turn_ending_or_starting_is_read_off_the_session_status() {
        let mut b = Board::default();
        b.sync(vec![status(S, "idle")], 1); // first sight: nothing to say yet
        assert!(b.done.is_empty());
        b.sync(vec![status(S, "busy")], 2);
        let (r, _) = b.hook("PermissionRequest", &permission("npm test"), 2);
        let ended = b.sync(vec![obj([("sessionId", S.into()), ("status", "idle".into()), ("lastText", "All green.".into())])], 3);
        assert_eq!(ended, [r.unwrap().id]);
        assert_eq!(b.done[S], Done { session: S.into(), cwd: None, at: 3, message: "All green.".into(), failed: false });
        assert!(b.snapshot().get("sessions").items()[0].get("lastText").is_null());
        b.sync(vec![status(S, "busy")], 4); // the user typed something
        assert!(b.done.is_empty());
    }
}
