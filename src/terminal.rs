//! How clearance starts a session: checked arguments for claude and its environment. Sessions run in
//! clearance's own terminals only: no terminal application, no PowerShell, no AppleScript. Windows Terminal on Windows, Terminal.app on macOS; system specific code stays here.

use crate::json::Json;

pub const MODELS: [&str; 5] = ["", "fable", "opus", "sonnet", "haiku"];
pub const MODES: [&str; 5] = ["", "default", "acceptEdits", "plan", "auto"];

pub struct Start {
    pub cwd: String,
    pub name: String,
    pub model: String,
    pub mode: String,
    pub resume: String, // the id of a session to resume, or empty for a new one
    pub prompt: String, // a first instruction, or empty
}

impl Start {
    pub fn from_json(b: &Json) -> Start {
        let s = |k: &str| b.get(k).str().unwrap_or("").to_string();
        Start { cwd: s("cwd"), name: s("name"), model: s("model"), mode: s("mode"), resume: s("resume"), prompt: s("prompt").trim().to_string() }
    }

    /// Why the request is refused, if it is.
    pub fn check(&self) -> Result<(), &'static str> {
        if !std::path::Path::new(&self.cwd).is_dir() {
            return Err("That folder does not exist.");
        }
        if !(1..=40).contains(&self.name.len()) || !self.name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
            return Err("A name is 1 to 40 letters, digits, dots, dashes or underscores.");
        }
        if !MODELS.contains(&self.model.as_str()) {
            return Err("Unknown model.");
        }
        if !MODES.contains(&self.mode.as_str()) {
            return Err("Unknown mode.");
        }
        if self.resume.len() > 64 || !self.resume.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err("Unknown session to resume.");
        }
        // A Windows command line holds 32767 characters.
        if self.prompt.chars().count() > 8000 {
            return Err("A first instruction is 8000 characters at most.");
        }
        Ok(())
    }

    /// claude and its arguments. `extra`: the hooks and the mod when the user's settings lack them.
    pub fn claude(&self, extra: &[String]) -> Vec<String> {
        let mut args = vec!["claude".to_string(), "-n".into(), self.name.clone()];
        if !self.resume.is_empty() {
            args.extend(["--resume".into(), self.resume.clone()]);
        }
        if !self.model.is_empty() {
            args.extend(["--model".into(), self.model.clone()]);
        }
        if !self.mode.is_empty() {
            args.extend(["--permission-mode".into(), self.mode.clone()]);
        }
        args.extend(extra.iter().cloned());
        // After `--`, an instruction starting with a dash is not read as an option.
        if !self.prompt.is_empty() {
            args.extend(["--".into(), self.prompt.clone()]);
        }
        args
    }
}

// What a running Claude Code session sets for the processes it starts. A new tab inherits the
// environment of whoever starts it: with these, clearance started from inside a session would open
// child sessions (no transcript, not listed by `claude agents`) holding that session's messaging token.
const SESSION_MARKERS: [&str; 10] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
];

/// clearance's environment without those markers, for a session it starts in its own terminal; a
/// terminal type when there is none.
pub fn session_env() -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = std::env::vars().filter(|(k, _)| !SESSION_MARKERS.contains(&k.as_str())).collect();
    if !env.iter().any(|(k, _)| k == "TERM") {
        env.push(("TERM".into(), "xterm-256color".into()));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn start(name: &str, model: &str, mode: &str) -> Start {
        Start { cwd: tmp(), name: name.into(), model: model.into(), mode: mode.into(), resume: String::new(), prompt: String::new() }
    }

    #[test]
    fn claude_gets_the_name_model_mode_and_what_connects_it() {
        let extra = ["--settings".to_string(), "/opt/clearance/hooks.json".into()];
        let want = ["claude", "-n", "orchard-1", "--model", "haiku", "--permission-mode", "plan", "--settings", "/opt/clearance/hooks.json"];
        assert_eq!(start("orchard-1", "haiku", "plan").claude(&extra), want);
        let mut first = start("x", "", "");
        first.prompt = "-v fix the tests".into();
        assert_eq!(first.claude(&[]), ["claude", "-n", "x", "--", "-v fix the tests"]);
        let mut back = start("x", "", "");
        back.resume = "0f3c-9a".into();
        assert_eq!(back.claude(&[]), ["claude", "-n", "x", "--resume", "0f3c-9a"]);
    }

    #[test]
    fn anything_odd_is_refused_before_a_terminal_sees_it() {
        let mut s = start("orchard", "", "");
        s.cwd = "/no/such/folder".into();
        assert!(s.check().unwrap_err().contains("folder"));
        for name in ["", "two words", "a;b", &"x".repeat(41), "\"quoted\""] {
            assert!(start(name, "", "").check().is_err(), "{name}");
        }
        assert!(start("orchard", "gpt", "").check().unwrap_err().contains("model"));
        assert!(start("orchard", "", "bypassPermissions").check().unwrap_err().contains("mode"));
        let mut s = start("orchard", "", "");
        s.resume = "abc --dangerously-skip-permissions".into();
        assert!(s.check().unwrap_err().contains("resume"));
    }
}
