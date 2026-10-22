//! Risk of a permission request: what an "Allow" would let run (BRIEF section 6), and the rules
//! suggested from approvals given by hand (ADR-0007).
//! ponytail: word rules on the command text, not a shell parser. Errors lean towards the higher class,
//! and the card always shows the full command whatever the class.

use crate::json::{Json, obj};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    Read,
    Write,
    Infra,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Write => "write",
            Risk::Infra => "infra",
        }
    }
}

const READ_TOOLS: [&str; 5] = ["Read", "Glob", "Grep", "WebFetch", "WebSearch"];
const SHELL_TOOLS: [&str; 2] = ["Bash", "PowerShell"];

// Reaches another machine, a service, a remote or a registry: never approved without the full command,
// never turned into a rule.
const REMOTE: [&str; 22] = [
    "ssh",
    "scp",
    "sftp",
    "rsync",
    "kubectl",
    "helm",
    "terraform",
    "tofu",
    "pulumi",
    "ansible",
    "ansible-playbook",
    "aws",
    "az",
    "gcloud",
    "sqlcmd",
    "psql",
    "mysql",
    "mongosh",
    "redis-cli",
    "systemctl",
    "shutdown",
    "reboot",
];
const CONTAINER_READS: [&str; 7] = ["compose", "ps", "images", "logs", "inspect", "version", "info"];
const WRITE_METHODS: [&str; 4] = ["post", "put", "patch", "delete"];
const READ_COMMANDS: [&str; 15] = ["ls", "dir", "cat", "head", "tail", "less", "grep", "rg", "find", "pwd", "cd", "wc", "which", "where", "echo"];
const GIT_READS: [&str; 7] = ["status", "log", "diff", "show", "branch", "remote", "rev-parse"];

pub fn classify(tool: &str, input: &Json) -> Risk {
    if READ_TOOLS.contains(&tool) {
        return Risk::Read;
    }
    if !SHELL_TOOLS.contains(&tool) {
        return Risk::Write;
    }
    let cmd = input.get("command").str().unwrap_or("");
    if word_starts(cmd).any(infra_at) || infra_anywhere(cmd) {
        return Risk::Infra;
    }
    let quiet = strip_quiet(cmd);
    let parts = split_parts(&quiet);
    let reads = !parts.is_empty() && !quiet.contains('>') && parts.iter().all(|p| read_command(p) && !dash_flag(p, &["delete", "exec"]));
    if reads { Risk::Read } else { Risk::Write }
}

/// The environment an infrastructure write reaches (ADR-0012): the first environment of the user's
/// `environments.json` with a pattern found in the command (or the tool's input), and that pattern. None
/// matched: unknown, which the page handles like production.
pub fn environment(input: &Json, patterns: &Json) -> (String, Option<String>) {
    let text = input.get("command").str().map_or_else(|| input.to_string(), String::from).to_lowercase();
    for (env, list) in patterns.entries() {
        if let Some(p) = list.items().iter().filter_map(Json::str).find(|p| !p.is_empty() && text.contains(&p.to_lowercase())) {
            return (env.clone(), Some(p.to_string()));
        }
    }
    ("unknown".into(), None)
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The rest of the line wherever a command word may start: at the start, or after a space, `;`, `&`,
/// `|` or `(`. So `sudo kubectl` or `time terraform` still count.
fn word_starts(cmd: &str) -> impl Iterator<Item = &str> {
    cmd.char_indices().filter(move |&(i, _)| i == 0 || cmd[..i].ends_with(|c: char| c.is_whitespace() || ";&|(".contains(c))).map(move |(i, _)| &cmd[i..])
}

/// The first word (up to a space) and what follows it.
fn split_word(s: &str) -> (&str, &str) {
    s.split_at(s.find(char::is_whitespace).unwrap_or(s.len()))
}

/// What follows one or more spaces; None when no space comes first.
fn after_ws(s: &str) -> Option<&str> {
    let t = s.trim_start();
    (t.len() < s.len()).then_some(t)
}

/// The letters, digits and underscores at the start.
fn lead_word(s: &str) -> &str {
    &s[..s.find(|c: char| !is_word_char(c)).unwrap_or(s.len())]
}

fn first_line(s: &str) -> &str {
    s.split('\n').next().unwrap_or("")
}

/// What follows the first character when it is a space.
fn after_one_ws(s: &str) -> Option<&str> {
    let mut chars = s.chars();
    chars.next().filter(|c| c.is_whitespace()).map(|_| chars.as_str())
}

/// `flag`, then spaces (at least one when `need_ws`), then an HTTP method that writes. Lowercase input.
fn flag_then_method(text: &str, flag: &str, need_ws: bool) -> bool {
    text.match_indices(flag).any(|(i, _)| {
        let rest = &text[i + flag.len()..];
        let r = rest.trim_start();
        (!need_ws || r.len() < rest.len()) && WRITE_METHODS.iter().any(|m| r.starts_with(m))
    })
}

fn infra_at(rest: &str) -> bool {
    let (word, after) = split_word(rest);
    let lower = word.to_ascii_lowercase();
    let next = after_ws(after);
    REMOTE.contains(&word)
        || (matches!(word, "docker" | "podman") && next.is_some_and(container_write))
        || (word == "git" && next.is_some_and(|n| lead_word(n) == "push"))
        || (matches!(word, "npm" | "pnpm" | "yarn") && next.is_some_and(|n| lead_word(n) == "publish"))
        || (matches!(lower.as_str(), "gh" | "glab") && next.is_some_and(|n| forge_write(&n.to_ascii_lowercase())))
        || (matches!(lower.as_str(), "curl" | "wget") && after_one_ws(after).is_some_and(|r| http_write(&first_line(r).to_ascii_lowercase())))
}

// `docker rm`, `docker compose up`; not `docker ps`, `docker compose logs`.
fn container_write(next: &str) -> bool {
    let changes = |s: &str| s.starts_with(|c: char| !c.is_whitespace()) && !CONTAINER_READS.contains(&lead_word(s));
    next.strip_prefix("compose").and_then(after_ws).is_some_and(changes) || changes(next)
}

fn forge_write(n: &str) -> bool {
    let sub = |name: &str, verbs: &[&str]| n.strip_prefix(name).and_then(after_ws).is_some_and(|r| verbs.iter().any(|v| r.starts_with(v)));
    sub("pr", &["create", "merge", "close"])
        || sub("mr", &["create", "merge", "close"])
        || n.starts_with("release")
        || sub("repo", &["create", "delete", "edit"])
        || n.strip_prefix("api").and_then(after_one_ws).is_some_and(|r| flag_then_method(first_line(r), "-x", false))
}

fn http_write(r: &str) -> bool {
    let data_flag = r.match_indices("-d").any(|(i, _)| r[..i].ends_with(char::is_whitespace) && r[i + 2..].starts_with(char::is_whitespace));
    flag_then_method(r, "-x", false) || flag_then_method(r, "--request", true) || data_flag || r.contains("--data")
}

// PowerShell writes and a download piped into a shell, wherever they are.
fn infra_anywhere(cmd: &str) -> bool {
    let l = cmd.to_ascii_lowercase();
    let bounded_after = |i: usize| !l[i..].starts_with(is_word_char);
    let web = ["invoke-restmethod", "invoke-webrequest"]
        .iter()
        .any(|name| l.match_indices(name).any(|(i, _)| bounded_after(i + name.len()) && flag_then_method(first_line(&l[i + name.len()..]), "-method", true)));
    let service = ["restart-", "stop-", "remove-"].iter().any(|verb| {
        ["service", "computer", "vm"].iter().any(|noun| {
            let w = format!("{verb}{noun}");
            l.match_indices(&w).any(|(i, _)| !l[..i].ends_with(is_word_char) && bounded_after(i + w.len()))
        })
    });
    let piped = l.match_indices('|').any(|(i, _)| {
        let r = l[i + 1..].trim_start();
        ["sh", "bash", "zsh", "iex", "invoke-expression"].iter().any(|s| r.starts_with(s) && !r[s.len()..].starts_with(is_word_char))
    });
    web || service || piped
}

/// Without the redirections that write nothing: `2>&1`, `>/dev/null`, `2>$null`, `>NUL`.
fn strip_quiet(cmd: &str) -> String {
    let b = cmd.as_bytes();
    let (mut out, mut copied, mut i) = (String::new(), 0, 0);
    while i < b.len() {
        let start = i;
        let gt = if b[i].is_ascii_digit() && b.get(i + 1) == Some(&b'>') {
            i + 1
        } else if b[i] == b'>' {
            i
        } else {
            i += 1;
            continue;
        };
        let end = if b.get(gt + 1) == Some(&b'&') && b.get(gt + 2).is_some_and(u8::is_ascii_digit) {
            Some(gt + 3)
        } else {
            let mut j = gt + 1;
            while b.get(j).is_some_and(u8::is_ascii_whitespace) {
                j += 1;
            }
            ["/dev/null", "$null", "nul"].iter().find_map(|t| {
                let k = j + t.len();
                (cmd.get(j..k).is_some_and(|s| s.eq_ignore_ascii_case(t)) && !b.get(k).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')).then_some(k)
            })
        };
        match end {
            Some(e) => {
                out.push_str(&cmd[copied..start]);
                copied = e;
                i = e;
            }
            None => i += 1,
        }
    }
    out.push_str(&cmd[copied..]);
    out
}

/// The commands of a chain: split on `&&`, `||`, `;`, `|` and new lines.
fn split_parts(s: &str) -> Vec<String> {
    s.replace("&&", "\n").replace("||", "\n").split([';', '|', '\n']).map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect()
}

fn read_command(p: &str) -> bool {
    let (word, after) = split_word(p);
    let w = word.to_ascii_lowercase();
    READ_COMMANDS.contains(&w.as_str())
        || (w == "git" && after_ws(after).is_some_and(|n| GIT_READS.contains(&split_word(n).0.to_ascii_lowercase().as_str())))
        || w.strip_prefix("get-").is_some_and(|rest| !rest.is_empty() && rest.chars().all(is_word_char))
}

/// ` -delete`, ` -exec` and the like as whole flags.
fn dash_flag(p: &str, names: &[&str]) -> bool {
    names.iter().any(|n| {
        let flag = format!("-{n}");
        p.match_indices(&flag).any(|(i, _)| p[..i].ends_with(char::is_whitespace) && !p[i + flag.len()..].starts_with(is_word_char))
    })
}

// ---- suggested rules (BRIEF section 3: fewer interruptions) ----

pub struct Approval {
    pub tool: String,
    pub input: Json,
    pub at: String,
    pub session: String,
}

// A rule allows every command that starts with its prefix, not only those approved. So a prefix is
// offered only for programs whose second word is a subcommand, or for programs that only read.
// ponytail: two allowlists; anything else gets no suggestion rather than a broad one.
const SUBCOMMANDS: [&str; 22] = [
    "git", "npm", "pnpm", "yarn", "npx", "bun", "deno", "cargo", "go", "dotnet", "mvn", "gradle", "make", "docker", "gh", "glab", "claude", "pip", "uv", "poetry", "winget",
    "choco",
];
const READERS: [&str; 18] =
    ["ls", "dir", "cat", "head", "tail", "grep", "rg", "wc", "pwd", "which", "where", "tree", "sort", "uniq", "diff", "select-string", "test-path", "measure-object"];
// Subcommands that run anything (`npm run`, `exec`, `gh api`): the next word must be named too.
const OPEN_ENDED: [&str; 5] = ["run", "exec", "x", "dlx", "api"];
// Subcommands that discard or rewrite local work: a rule for them is never offered.
const DESTRUCTIVE: [&str; 12] = ["checkout", "reset", "clean", "restore", "rm", "rebase", "branch", "stash", "config", "uninstall", "remove", "prune"];
// An MCP tool that writes to the outside world, or runs code, gets no rule; names are split into words
// (`create_issue`, `addComment`) so that `job_runs` is not mistaken for `run`.
const MCP_WRITE: [&str; 31] = [
    "create",
    "delete",
    "update",
    "send",
    "post",
    "put",
    "patch",
    "set",
    "add",
    "remove",
    "edit",
    "write",
    "transition",
    "move",
    "rename",
    "upload",
    "trash",
    "modify",
    "publish",
    "deploy",
    "execute",
    "run",
    "start",
    "stop",
    "restart",
    "javascript",
    "computer",
    "click",
    "type",
    "press",
    "form",
];
const NO_RULE: [&str; 6] = ["AskUserQuestion", "ExitPlanMode", "Workflow", "Skill", "Agent", "Task"];

fn name_words(name: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    for c in name.chars() {
        if c == '_' || c == '-' {
            out.push(String::new());
            continue;
        }
        if c.is_ascii_uppercase() && out.last().is_some_and(|w| !w.is_empty()) {
            out.push(String::new());
        }
        if let Some(w) = out.last_mut() {
            w.push(c.to_ascii_lowercase());
        }
    }
    out
}

pub fn hostname(url: &str) -> Option<String> {
    let authority = url.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = if host.starts_with('[') { format!("{}]", host.split(']').next()?) } else { host.split(':').next()?.to_string() };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

pub fn rule_of(tool: &str, input: &Json) -> Option<String> {
    if NO_RULE.contains(&tool) || classify(tool, input) == Risk::Infra {
        return None;
    }
    if let Some(rest) = tool.strip_prefix("mcp__") {
        let name = rest.split("__").skip(1).collect::<Vec<_>>().join("_");
        return (!name_words(&name).iter().any(|w| MCP_WRITE.contains(&w.as_str()))).then(|| tool.to_string());
    }
    if tool == "WebFetch" {
        return hostname(input.get("url").str()?).map(|h| format!("WebFetch(domain:{h})"));
    }
    if !SHELL_TOOLS.contains(&tool) {
        return None; // Read, Edit, Write: a rule by path is the user's call
    }
    // A chain (`a && b; c`) was approved as a whole, not as its first command: it counts for nothing.
    let command = input.get("command").str().unwrap_or("").trim();
    if command.contains("&&") || command.contains("||") || command.contains([';', '\n']) {
        return None;
    }
    let mut words = command.split('|').next().unwrap_or("").split_whitespace();
    let (program, sub, third) = (words.next().unwrap_or(""), words.next(), words.next());
    let word = |w: Option<&str>| w.is_some_and(|w| w.starts_with(|c: char| c.is_ascii_alphabetic()) && w.chars().all(|c| is_word_char(c) || c == '-'));
    let reader = |p: &str| {
        let l = p.to_ascii_lowercase();
        READERS.contains(&l.as_str()) || ["get-", "format-"].iter().any(|v| l.strip_prefix(v).is_some_and(|r| !r.is_empty() && r.chars().all(is_word_char)))
    };
    let prefix = if !SUBCOMMANDS.contains(&program) {
        reader(program).then(|| program.to_string())?
    } else {
        let sub = sub.filter(|s| word(Some(s)) && !DESTRUCTIVE.contains(s))?;
        if !OPEN_ENDED.contains(&sub) {
            format!("{program} {sub}")
        } else if word(third) {
            format!("{program} {sub} {}", third?)
        } else {
            return None;
        }
    };
    if classify(tool, &obj([("command", prefix.as_str().into())])) == Risk::Infra {
        return None;
    }
    Some(format!("{tool}({prefix}:*)"))
}

pub struct Suggestion {
    pub rule: String,
    pub count: usize,
    pub sessions: usize,
    pub last: String,
    pub example: String,
}

impl Suggestion {
    pub fn to_json(&self) -> Json {
        obj([
            ("rule", self.rule.as_str().into()),
            ("count", self.count.into()),
            ("sessions", self.sessions.into()),
            ("last", self.last.as_str().into()),
            ("example", self.example.as_str().into()),
        ])
    }
}

/// Approvals given by hand at least `min` times for the same rule, most frequent first.
pub fn suggest(approvals: &[Approval], min: usize) -> Vec<Suggestion> {
    let mut groups: Vec<(String, Vec<&Approval>)> = Vec::new();
    for a in approvals {
        let Some(rule) = rule_of(&a.tool, &a.input) else { continue };
        match groups.iter_mut().find(|(r, _)| *r == rule) {
            Some((_, list)) => list.push(a),
            None => groups.push((rule, vec![a])),
        }
    }
    let mut out: Vec<Suggestion> = groups
        .into_iter()
        .filter(|(_, list)| list.len() >= min)
        .map(|(rule, list)| {
            let last = list.iter().copied().reduce(|x, y| if y.at > x.at { y } else { x }).expect("a group has an approval");
            let mut sessions: Vec<&str> = list.iter().map(|a| a.session.as_str()).collect();
            sessions.sort_unstable();
            sessions.dedup();
            let example = last.input.get("command").str().or(last.input.get("url").str()).unwrap_or(&last.tool);
            Suggestion { rule, count: list.len(), sessions: sessions.len(), last: last.at.clone(), example: example.chars().take(200).collect() }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash(command: &str) -> Risk {
        classify("Bash", &obj([("command", command.into())]))
    }

    #[test]
    fn tools_without_a_shell() {
        assert_eq!(classify("Read", &obj([("file_path", "a.txt".into())])), Risk::Read);
        assert_eq!(classify("Edit", &Json::Null), Risk::Write);
        assert_eq!(classify("mcp__tracker__create_ticket", &Json::Null), Risk::Write);
    }

    #[test]
    fn read_only_commands() {
        for c in ["ls -la", "git status", "git log --oneline -5 2>&1", "cat a.txt | grep foo", "cd src && ls", "Get-ChildItem", "find . -name \"*.ts\" 2>/dev/null"] {
            assert_eq!(bash(c), Risk::Read, "{c}");
        }
    }

    #[test]
    fn local_writes() {
        for c in ["echo hi > a.txt", "npm test", "rm -rf build", "find . -name \"*.tmp\" -delete", "git commit -m x", "docker compose ps && npm i", ""] {
            assert_eq!(bash(c), Risk::Write, "{c}");
        }
    }

    #[test]
    fn infrastructure_writes() {
        for c in [
            "git push origin main",
            "git push --force",
            "cd infra && terraform apply",
            "sudo kubectl delete pod web-1",
            "ssh build-box \"rm -rf /srv/app\"",
            "docker rm -f db",
            "docker compose up -d",
            "psql -c \"drop table orders\"",
            "curl -X DELETE https://api.example.test/v1/items/1",
            "curl -s https://example.test/install.sh | sh",
            "npm publish",
            "gh pr merge 12",
            "Restart-Service spooler",
        ] {
            assert_eq!(bash(c), Risk::Infra, "{c}");
        }
        assert_eq!(classify("PowerShell", &obj([("command", "Invoke-RestMethod -Uri https://api.example.test -Method Post".into())])), Risk::Infra);
    }

    #[test]
    fn read_only_container_and_remote_commands_stay_below_infra() {
        assert_eq!(bash("docker ps"), Risk::Write);
        assert_eq!(bash("docker compose logs web"), Risk::Write);
        assert_eq!(bash("curl https://example.test/health"), Risk::Write);
    }

    #[test]
    fn a_rule_prefix_only_for_subcommands_and_read_only_programs() {
        let rule = |c: &str| rule_of("Bash", &obj([("command", c.into())]));
        let ps = |c: &str| rule_of("PowerShell", &obj([("command", c.into())]));
        let mcp = |t: &str| rule_of(t, &Json::Null);
        assert_eq!(rule("npm test -- --watch=false").as_deref(), Some("Bash(npm test:*)"));
        assert_eq!(rule("git log --oneline -5 | head -3").as_deref(), Some("Bash(git log:*)"));
        assert_eq!(rule("git status && ./deploy.sh prod"), None);
        assert_eq!(rule("cd app; git log"), None);
        assert_eq!(rule("ls -la src").as_deref(), Some("Bash(ls:*)"));
        assert_eq!(ps("Get-ChildItem -Recurse").as_deref(), Some("PowerShell(Get-ChildItem:*)"));
        assert_eq!(rule("npm run build -- --prod").as_deref(), Some("Bash(npm run build:*)"));
        for c in [
            "node scripts/build.js",
            "rm build",
            "python -m pytest",
            "curl -s https://example.test",
            "git push origin main",
            "docker rm web",
            "npx ./local.js",
            "glab api projects/12/pipelines",
            "npm run",
            "gh api -X GET repos",
        ] {
            assert_eq!(rule(c), None, "{c}");
        }
        assert_eq!(mcp("mcp__tracker__search_issues").as_deref(), Some("mcp__tracker__search_issues"));
        assert_eq!(mcp("mcp__tracker__create_issue"), None);
        assert_eq!(mcp("mcp__mail__send_message"), None);
        assert_eq!(mcp("mcp__wiki__addCommentToPage"), None);
        assert_eq!(mcp("mcp__browser__javascript_tool"), None);
        assert_eq!(mcp("mcp__scheduler__job_runs").as_deref(), Some("mcp__scheduler__job_runs"));
        for c in ["git checkout -- .", "git reset --hard", "git clean -fd", "git branch -D old"] {
            assert_eq!(rule(c), None, "{c}");
        }
        assert_eq!(rule_of("WebFetch", &obj([("url", "https://docs.example.test/a?b".into())])).as_deref(), Some("WebFetch(domain:docs.example.test)"));
        assert_eq!(rule_of("AskUserQuestion", &Json::Null), None);
        assert_eq!(rule_of("Edit", &obj([("file_path", "a.ts".into())])), None);
    }

    #[test]
    fn suggestions_need_three_approvals_and_are_sorted_by_count() {
        let a = |command: &str, session: &str, at: &str| Approval { tool: "Bash".into(), input: obj([("command", command.into())]), session: session.into(), at: at.into() };
        let out = suggest(
            &[
                a("npm test", "s1", "2026-10-01"),
                a("npm test -- --ci", "s2", "2026-10-03"),
                a("npm test", "s2", "2026-10-02"),
                a("git status", "s1", "2026-10-01"),
                a("git status -s", "s1", "2026-10-02"),
                a("git log", "s1", "2026-10-02"),
                a("git status", "s3", "2026-10-04"),
                a("git status", "s3", "2026-10-05"),
            ],
            3,
        );
        let got: Vec<(&str, usize, usize, &str, &str)> = out.iter().map(|s| (s.rule.as_str(), s.count, s.sessions, s.last.as_str(), s.example.as_str())).collect();
        assert_eq!(got, [("Bash(git status:*)", 4, 2, "2026-10-05", "git status"), ("Bash(npm test:*)", 3, 2, "2026-10-03", "npm test -- --ci")]);
    }

    #[test]
    fn environment_from_the_user_patterns() {
        let patterns = Json::parse(r#"{"prod":["-prd","db-prod"],"staging":["-n staging","-STG"]}"#).unwrap();
        let env = |c: &str| environment(&obj([("command", c.into())]), &patterns);
        assert_eq!(env("kubectl rollout restart deploy/gw -n staging"), ("staging".into(), Some("-n staging".into())));
        assert_eq!(env("psql -h DB-PROD -c 'vacuum'"), ("prod".into(), Some("db-prod".into())));
        assert_eq!(env("sqlcmd -S sql-stg"), ("staging".into(), Some("-STG".into())));
        assert_eq!(env("ssh bastion"), ("unknown".into(), None));
        assert_eq!(environment(&obj([("instance", "app-prd".into())]), &patterns).0, "prod");
        assert_eq!(environment(&obj([("command", "ssh x".into())]), &Json::Null).0, "unknown");
    }

    #[test]
    fn quiet_redirections_and_odd_spots() {
        assert_eq!(strip_quiet("ls 2>&1 >/dev/null 2>$NULL > nul"), "ls    ");
        assert_eq!(bash("a || bash"), Risk::Infra);
        assert_eq!(bash("docker psql"), Risk::Infra);
        assert_eq!(bash("(ssh box)"), Risk::Infra);
    }
}
