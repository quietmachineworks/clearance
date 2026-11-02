//! clearance's hooks in the user's Claude Code settings, added or removed. An explicit step: a
//! backup first, the user's own hooks left as they are, and only entries that point at clearance are
//! ever replaced or removed.
//!   clearance install [--dry-run] [--remove] [--settings <file>]
//! The companion mod installs apart: `claude plugin marketplace add <this folder>`, then
//! `claude plugin install clearance@clearance`.

use crate::json::{Json, obj};
use crate::sys;
use std::path::{Path, PathBuf};

const EVENTS: [&str; 7] = ["PermissionRequest", "PermissionDenied", "Stop", "StopFailure", "UserPromptSubmit", "SubagentStart", "SubagentStop"];

// PermissionRequest (permissions, questions, plans) runs in parallel with the terminal dialog, so it
// waits up to a day: past its timeout a request leaves the page. An auto mode denial is held
// 60 s while a page is open. The others answer at once. Only hooks that stay silent while clearance is
// stopped: Stop and UserPromptSubmit would print an error in every session.
const HOOKS: [(&str, u64); 5] = [("PermissionRequest", 86_400), ("PermissionDenied", 60), ("StopFailure", 5), ("SubagentStart", 5), ("SubagentStop", 5)];

/// The `hooks` block of a settings file that connects a session to clearance at `base`.
pub fn hooks(base: &str, token: &str) -> Json {
    Json::Obj(
        HOOKS
            .iter()
            .map(|(event, timeout)| {
                let hook = obj([
                    ("type", "http".into()),
                    ("url", format!("{base}/hook/{event}").into()),
                    ("headers", obj([("Authorization", format!("Bearer {token}").into())])),
                    ("timeout", (*timeout).into()),
                ]);
                (event.to_string(), Json::Arr(vec![obj([("hooks", Json::Arr(vec![hook]))])]))
            })
            .collect(),
    )
}

/// The deny rules that keep a session's file tools out of clearance's folder: `Edit` covers
/// every tool that writes, `Read` the ones that read, Grep and Glob included.
pub fn deny(home: &Path) -> Vec<String> {
    let posix = |p: &Path| p.display().to_string().replace('\\', "/");
    // The long form of each path (no `C:\Users\NAME~1`, no `\\?\`), as Claude Code compares them.
    let real = |p: &Path| std::fs::canonicalize(p).ok().map(|c| PathBuf::from(c.display().to_string().trim_start_matches(r"\\?\"))).unwrap_or_else(|| p.to_path_buf());
    let (home, base) = (&real(home), real(&sys::home()));
    let at = match home.strip_prefix(&base) {
        Ok(rest) if !base.as_os_str().is_empty() => format!("~/{}", posix(rest)),
        // Claude Code matches Windows paths in POSIX form: C:\x becomes /c/x.
        _ => match posix(home).split_once(":/") {
            Some((drive, rest)) => format!("//{}/{rest}", drive.to_ascii_lowercase()),
            None => format!("/{}", posix(home)),
        },
    };
    ["Read", "Edit"].iter().map(|tool| format!("{tool}({}/**)", at.trim_end_matches('/'))).collect()
}

// ponytail: a deny rule is ours when it guards a `.clearance` folder; a custom CLEARANCE_HOME elsewhere is
// recognised by the exact rules `deny` gives for it.
fn our_rule(rule: &str) -> bool {
    (rule.starts_with("Read(") || rule.starts_with("Edit(")) && rule.ends_with("/.clearance/**)")
}

fn clearance_url(url: &str) -> bool {
    url.strip_prefix("http://127.0.0.1:")
        .and_then(|r| r.split_once("/hook/"))
        .is_some_and(|(port, event)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && EVENTS.contains(&event))
}

fn ours(group: &Json) -> bool {
    group.get("hooks").items().iter().any(|h| h.get("type").str() == Some("http") && h.get("url").str().is_some_and(clearance_url))
}

/// The settings with every clearance entry taken out (hooks, and deny rules on its folder or among
/// `deny`), and nothing else.
pub fn without_hooks(settings: &Json, deny: &[String]) -> Json {
    let mut out = settings.clone();
    let mut permissions = out.get("permissions").clone();
    if let Json::Arr(rules) = permissions.get("deny").clone() {
        let kept: Vec<Json> = rules.into_iter().filter(|r| !r.str().is_some_and(|r| our_rule(r) || deny.iter().any(|d| d == r))).collect();
        if kept.is_empty() {
            permissions.remove("deny");
        } else {
            permissions.set("deny", Json::Arr(kept));
        }
        if permissions.entries().is_empty() {
            out.remove("permissions");
        } else {
            out.set("permissions", permissions);
        }
    }
    let mut hooks = out.get("hooks").clone();
    if let Json::Obj(events) = &mut hooks {
        for (_, groups) in events.iter_mut() {
            if let Json::Arr(g) = groups {
                g.retain(|x| !ours(x));
            }
        }
        events.retain(|(_, groups)| !groups.items().is_empty());
        if events.is_empty() {
            out.remove("hooks");
        } else {
            out.set("hooks", hooks);
        }
    }
    out
}

/// A new install replaces the previous one (new token, new port), next to the user's own hooks and rules.
pub fn with_hooks(settings: &Json, hooks: &Json, deny: &[String]) -> Json {
    let mut out = without_hooks(settings, deny);
    let mut all = if out.get("hooks").is_null() { Json::Obj(Vec::new()) } else { out.get("hooks").clone() };
    for (event, groups) in hooks.entries() {
        let mut list = all.get(event).items().to_vec();
        list.extend(groups.items().iter().cloned());
        all.set(event, Json::Arr(list));
    }
    out.set("hooks", all);
    if deny.is_empty() {
        return out;
    }
    let mut permissions = if out.get("permissions").is_null() { Json::Obj(Vec::new()) } else { out.get("permissions").clone() };
    let mut rules = permissions.get("deny").items().to_vec();
    rules.extend(deny.iter().map(|r| Json::from(r.as_str())));
    permissions.set("deny", Json::Arr(rules));
    out.set("permissions", permissions);
    out
}

/// The settings file clearance writes for a session it connects (`claude --settings`).
pub fn session_settings(base: &str, token: &str, home: &Path) -> Json {
    obj([("hooks", hooks(base, token)), ("permissions", obj([("deny", Json::Arr(deny(home).into_iter().map(Json::from).collect()))]))])
}

/// Whether these settings already connect every session to clearance.
pub fn installed(settings: &Json) -> bool {
    settings.get("hooks").get("PermissionRequest").items().iter().any(ours)
}

pub fn user_settings() -> PathBuf {
    sys::home().join(".claude").join("settings.json")
}

pub fn read_settings(file: &Path) -> Result<Json, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Json::parse(&text).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Json::Obj(Vec::new())),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

// Which events hold a clearance entry, for the summary; never the token.
fn count(settings: &Json) -> String {
    let parts: Vec<String> =
        settings.get("hooks").entries().iter().map(|(e, g)| (e, g.items().iter().filter(|x| ours(x)).count())).filter(|(_, n)| *n > 0).map(|(e, n)| format!("{e}: {n}")).collect();
    format!("{{{}}}", parts.join(", "))
}

/// `clearance install`: prints what changes, then backs the file up and writes it.
pub fn run(args: &[String], base: &str, token: &str, home: &Path) -> Result<(), String> {
    let file = args.iter().position(|a| a == "--settings").and_then(|i| args.get(i + 1)).map_or_else(user_settings, PathBuf::from);
    let remove = args.iter().any(|a| a == "--remove");
    let settings = read_settings(&file)?;
    let rules = deny(home);
    let next = if remove { without_hooks(&settings, &rules) } else { with_hooks(&settings, &hooks(base, token), &rules) };
    let others: usize = settings.get("hooks").entries().iter().map(|(_, g)| g.items().iter().filter(|x| !ours(x)).count()).sum();
    let guards =
        |s: &Json| s.get("permissions").get("deny").items().iter().filter_map(Json::str).filter(|r| our_rule(r) || rules.iter().any(|d| d == r)).collect::<Vec<_>>().join(", ");
    println!("settings: {}", file.display());
    println!("clearance hooks before: {}", count(&settings));
    println!("clearance hooks after:  {}", count(&next));
    println!("deny rules on clearance's folder before: [{}]", guards(&settings));
    println!("deny rules on clearance's folder after:  [{}]", guards(&next));
    println!("other hooks kept as they are: {others}");
    if args.iter().any(|a| a == "--dry-run") {
        return Ok(());
    }
    if file.exists() {
        let stamp: String = sys::iso(sys::now_ms()).chars().map(|c| if c == ':' || c == '.' { '-' } else { c }).collect();
        let backup = PathBuf::from(format!("{}.bak-clearance-{stamp}", file.display()));
        std::fs::copy(&file, &backup).map_err(|e| format!("backup {}: {e}", backup.display()))?;
        println!("backup: {}", backup.display());
    }
    std::fs::write(&file, next.pretty() + "\n").map_err(|e| format!("{}: {e}", file.display()))?;
    println!("{}", if remove { "clearance hooks removed." } else { "clearance hooks installed; sessions started from now on use them." });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(text: &str) -> Json {
        Json::parse(text).unwrap()
    }

    fn http(event: &str, port: u16) -> Json {
        j(&format!(r#"[{{"hooks":[{{"type":"http","url":"http://127.0.0.1:{port}/hook/{event}","headers":{{"Authorization":"Bearer t"}},"timeout":5}}]}}]"#))
    }

    fn theirs() -> Json {
        j(
            r#"{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"python stamp.py"}]}],"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"rewrite-bash"}]}],"Stop":[{"hooks":[{"type":"http","url":"http://127.0.0.1:9000/notify"}]}]}"#,
        )
    }

    fn settings() -> Json {
        let mut s = j(r#"{"model":"opus","permissions":{"defaultMode":"auto"}}"#);
        s.set("hooks", theirs());
        s
    }

    fn ours_block(port: u16) -> Json {
        Json::Obj(vec![
            ("PermissionRequest".into(), http("PermissionRequest", port)),
            ("Stop".into(), http("Stop", port)),
            ("UserPromptSubmit".into(), http("UserPromptSubmit", port)),
        ])
    }

    #[test]
    fn install_adds_clearance_next_to_the_hooks_already_there() {
        let out = with_hooks(&settings(), &ours_block(47800), &[]);
        assert_eq!(out.get("model").str(), Some("opus"));
        assert_eq!(out.get("hooks").get("PreToolUse"), theirs().get("PreToolUse"));
        let joined = |event: &str| Json::Arr([theirs().get(event).items(), ours_block(47800).get(event).items()].concat());
        assert_eq!(out.get("hooks").get("UserPromptSubmit"), &joined("UserPromptSubmit"));
        assert_eq!(out.get("hooks").get("Stop"), &joined("Stop"));
        assert_eq!(out.get("hooks").get("PermissionRequest"), &http("PermissionRequest", 47800));
        assert!(installed(&out));
        assert!(!installed(&settings()));
    }

    #[test]
    fn installing_again_replaces_the_previous_clearance_entries_only() {
        let mut second = ours_block(47800);
        second.set("Stop", http("Stop", 47900));
        let twice = with_hooks(&with_hooks(&settings(), &ours_block(47800), &[]), &second, &[]);
        assert_eq!(twice.get("hooks").get("Stop").items().len(), 2);
        assert_eq!(twice.get("hooks").get("Stop").items()[1].get("hooks").items()[0].get("url").str(), Some("http://127.0.0.1:47900/hook/Stop"));
        assert_eq!(twice.get("hooks").get("PermissionRequest").items().len(), 1);
    }

    #[test]
    fn remove_takes_out_clearance_and_nothing_else() {
        assert_eq!(without_hooks(&with_hooks(&settings(), &ours_block(47800), &[]), &[]), settings());
        let mut only = Json::Obj(Vec::new());
        only.set("hooks", ours_block(47800));
        assert_eq!(without_hooks(&only, &[]).to_string(), "{}");
    }

    #[test]
    fn sessions_are_denied_clearance_s_folder_and_remove_takes_only_those_rules_out() {
        let rules = deny(&sys::home().join(".clearance"));
        assert_eq!(rules, ["Read(~/.clearance/**)", "Edit(~/.clearance/**)"]);
        let mut s = settings();
        s.set("permissions", j(r#"{"defaultMode":"auto","deny":["Bash(rm -rf *)"]}"#));
        let installed = with_hooks(&s, &ours_block(47800), &rules);
        assert_eq!(installed.get("permissions").get("deny").to_string(), r#"["Bash(rm -rf *)","Read(~/.clearance/**)","Edit(~/.clearance/**)"]"#);
        let again = with_hooks(&installed, &ours_block(47800), &rules);
        assert_eq!(again.get("permissions").get("deny").items().len(), 3, "installing twice adds nothing");
        assert_eq!(without_hooks(&again, &rules), s);
        if cfg!(windows) {
            assert_eq!(deny(Path::new(r"D:\tools\clr")), ["Read(//d/tools/clr/**)", "Edit(//d/tools/clr/**)"]);
        }
        let session = session_settings("http://127.0.0.1:1", "t", &sys::home().join(".clearance"));
        assert_eq!(session.get("permissions").get("deny").items().len(), 2);
    }

    #[test]
    fn the_hooks_clearance_writes() {
        let h = hooks("http://127.0.0.1:47800", "t0k");
        assert_eq!(h.entries().iter().map(|(e, _)| e.as_str()).collect::<Vec<_>>(), ["PermissionRequest", "PermissionDenied", "StopFailure", "SubagentStart", "SubagentStop"]);
        assert_eq!(
            h.get("PermissionRequest").items()[0].get("hooks").items()[0].to_string(),
            r#"{"type":"http","url":"http://127.0.0.1:47800/hook/PermissionRequest","headers":{"Authorization":"Bearer t0k"},"timeout":86400}"#
        );
    }
}
