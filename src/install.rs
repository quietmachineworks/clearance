//! clearance's hooks in the user's Claude Code settings, added or removed (ADR-0009). An explicit step: a
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
// waits up to a day: past its timeout a request leaves the page (ADR-0009). An auto mode denial is held
// 60 s while a page is open. The others answer at once. Only hooks that stay silent while clearance is
// stopped: Stop and UserPromptSubmit would print an error in every session (ADR-0009).
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

fn clearance_url(url: &str) -> bool {
    url.strip_prefix("http://127.0.0.1:")
        .and_then(|r| r.split_once("/hook/"))
        .is_some_and(|(port, event)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && EVENTS.contains(&event))
}

fn ours(group: &Json) -> bool {
    group.get("hooks").items().iter().any(|h| h.get("type").str() == Some("http") && h.get("url").str().is_some_and(clearance_url))
}

/// The settings with every clearance entry taken out, and nothing else.
pub fn without_hooks(settings: &Json) -> Json {
    let mut out = settings.clone();
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

/// A new install replaces the previous one (new token, new port), next to the user's own hooks.
pub fn with_hooks(settings: &Json, hooks: &Json) -> Json {
    let mut out = without_hooks(settings);
    let mut all = if out.get("hooks").is_null() { Json::Obj(Vec::new()) } else { out.get("hooks").clone() };
    for (event, groups) in hooks.entries() {
        let mut list = all.get(event).items().to_vec();
        list.extend(groups.items().iter().cloned());
        all.set(event, Json::Arr(list));
    }
    out.set("hooks", all);
    out
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
pub fn run(args: &[String], base: &str, token: &str) -> Result<(), String> {
    let file = args.iter().position(|a| a == "--settings").and_then(|i| args.get(i + 1)).map_or_else(user_settings, PathBuf::from);
    let remove = args.iter().any(|a| a == "--remove");
    let settings = read_settings(&file)?;
    let next = if remove { without_hooks(&settings) } else { with_hooks(&settings, &hooks(base, token)) };
    let others: usize = settings.get("hooks").entries().iter().map(|(_, g)| g.items().iter().filter(|x| !ours(x)).count()).sum();
    println!("settings: {}", file.display());
    println!("clearance hooks before: {}", count(&settings));
    println!("clearance hooks after:  {}", count(&next));
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
        let out = with_hooks(&settings(), &ours_block(47800));
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
        let twice = with_hooks(&with_hooks(&settings(), &ours_block(47800)), &second);
        assert_eq!(twice.get("hooks").get("Stop").items().len(), 2);
        assert_eq!(twice.get("hooks").get("Stop").items()[1].get("hooks").items()[0].get("url").str(), Some("http://127.0.0.1:47900/hook/Stop"));
        assert_eq!(twice.get("hooks").get("PermissionRequest").items().len(), 1);
    }

    #[test]
    fn remove_takes_out_clearance_and_nothing_else() {
        assert_eq!(without_hooks(&with_hooks(&settings(), &ours_block(47800))), settings());
        let mut only = Json::Obj(Vec::new());
        only.set("hooks", ours_block(47800));
        assert_eq!(without_hooks(&only).to_string(), "{}");
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
