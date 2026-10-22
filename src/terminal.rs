//! Terminal tabs: start a session in a real tab, bring a session's tab to the front (ADR-0004,
//! ADR-0008). Windows Terminal on Windows, Terminal.app on macOS; system specific code stays here.

use crate::json::Json;
use std::process::Command;

pub const MODELS: [&str; 5] = ["", "fable", "opus", "sonnet", "haiku"];
pub const MODES: [&str; 5] = ["", "default", "acceptEdits", "plan", "auto"];

pub struct Start {
    pub cwd: String,
    pub name: String,
    pub model: String,
    pub mode: String,
}

impl Start {
    pub fn from_json(b: &Json) -> Start {
        let s = |k: &str| b.get(k).str().unwrap_or("").to_string();
        Start { cwd: s("cwd"), name: s("name"), model: s("model"), mode: s("mode") }
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
        Ok(())
    }

    /// claude and its arguments. `extra`: the hooks and the mod when the user's settings lack them.
    pub fn claude(&self, extra: &[String]) -> Vec<String> {
        let mut args = vec!["claude".to_string(), "-n".into(), self.name.clone()];
        if !self.model.is_empty() {
            args.extend(["--model".into(), self.model.clone()]);
        }
        if !self.mode.is_empty() {
            args.extend(["--permission-mode".into(), self.mode.clone()]);
        }
        args.extend(extra.iter().cloned());
        args
    }
}

/// The `wt` arguments for a new tab running claude. The tab keeps its title (the session's name), so
/// that it can be found again. `wt` splits its command line on ";", so none may appear.
/// ponytail: no first prompt for the same reason; it is sent as an instruction once the session runs.
pub fn wt_args(s: &Start, extra: &[String]) -> Result<Vec<String>, &'static str> {
    s.check()?;
    let mut args: Vec<String> = ["-w", "0", "new-tab", "--title", &s.name, "--suppressApplicationTitle", "-d", &s.cwd].iter().map(|a| a.to_string()).collect();
    args.extend(s.claude(extra));
    if args.iter().any(|a| a.contains(';')) {
        return Err("A path with \";\" cannot be passed to Windows Terminal.");
    }
    Ok(args)
}

/// A POSIX shell word: single quotes, a quote inside written `'\''`.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The line Terminal.app runs in the new tab.
pub fn shell_line(s: &Start, extra: &[String]) -> Result<String, &'static str> {
    s.check()?;
    Ok(format!("cd {} && exec {}", quote(&s.cwd), s.claude(extra).iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")))
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

fn without_markers(cmd: &mut Command) -> &mut Command {
    for m in SESSION_MARKERS {
        cmd.env_remove(m);
    }
    cmd
}

/// Opens the tab; an error message when it could not.
pub fn start(s: &Start, extra: &[String]) -> Result<(), String> {
    if cfg!(windows) {
        let args = wt_args(s, extra)?;
        let status = without_markers(crate::sys::hidden(&mut Command::new("wt.exe"))).args(args).status().map_err(|e| e.to_string())?;
        status.success().then_some(()).ok_or_else(|| format!("wt.exe ended with {status}"))
    } else if cfg!(target_os = "macos") {
        let line = shell_line(s, extra)?;
        // Every value goes in as an AppleScript argument, never into the script text.
        let script = [
            "on run argv",
            "tell application \"Terminal\"",
            "activate",
            "set t to do script (item 1 of argv)",
            "set custom title of t to (item 2 of argv)",
            "set title displays custom title of t to true",
            "end tell",
            "end run",
        ];
        osascript(&script, &[line, s.name.clone()]).map(|_| ())
    } else {
        Err("Starting a session in a terminal tab is supported on Windows and macOS.".into())
    }
}

fn osascript(lines: &[&str], args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new("osascript");
    for l in lines {
        cmd.args(["-e", l]);
    }
    cmd.arg("--").args(args);
    crate::sys::output(&mut cmd, std::time::Duration::from_secs(15)).ok_or_else(|| "osascript failed".to_string())
}

// UI Automation through Windows PowerShell: every tab of every Windows Terminal window, its name without
// the status glyph Claude Code puts in front. The titles come in through the environment, never into the
// script text. Inline, no file, no execution policy change.
const FOCUS: &str = r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$titles = $env:CLEARANCE_TITLES -split [char]10
$A = [System.Windows.Automation.AutomationElement]
$scope = [System.Windows.Automation.TreeScope]
$windows = New-Object System.Windows.Automation.PropertyCondition($A::ClassNameProperty, 'CASCADIA_HOSTING_WINDOW_CLASS')
$tabs = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
foreach ($w in $A::RootElement.FindAll($scope::Children, $windows)) {
  foreach ($t in $w.FindAll($scope::Descendants, $tabs)) {
    $name = ($t.Current.Name -replace '^[^\p{L}\p{N}]+', '').Trim()
    if ($titles -contains $name) {
      $t.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
      try { $w.SetFocus() } catch {}
      'found'
      exit 0
    }
  }
}
'not found'
"#;

/// Brings to the front the tab whose title is one of `titles`; whether one was found.
pub fn focus(titles: &[String]) -> bool {
    let found = |out: Option<String>| out.is_some_and(|o| o.trim() == "found");
    if cfg!(windows) {
        // One title per line: Windows PowerShell 5.1's ConvertFrom-Json would not unroll a JSON array.
        let lines = titles.iter().map(|t| t.replace(['\r', '\n'], " ")).collect::<Vec<_>>().join("\n");
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", FOCUS]).env("CLEARANCE_TITLES", lines);
        found(crate::sys::output(&mut cmd, std::time::Duration::from_secs(15)))
    } else if cfg!(target_os = "macos") {
        // ponytail: Terminal.app only, matched on the custom title clearance set; iTerm2 when asked for.
        let script = [
            "on run argv",
            "tell application \"Terminal\"",
            "repeat with w in windows",
            "repeat with t in tabs of w",
            "if (custom title of t) is in argv then",
            "set selected of t to true",
            "set index of w to 1",
            "activate",
            "return \"found\"",
            "end if",
            "end repeat",
            "end repeat",
            "end tell",
            "return \"not found\"",
            "end run",
        ];
        found(osascript(&script, titles).ok())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn start(name: &str, model: &str, mode: &str) -> Start {
        Start { cwd: tmp(), name: name.into(), model: model.into(), mode: mode.into() }
    }

    #[test]
    fn a_new_tab_runs_claude_with_the_name_as_a_fixed_title() {
        let extra = ["--settings".to_string(), "/opt/clearance/hooks.json".into()];
        let args = wt_args(&start("orchard-1", "haiku", "plan"), &extra).unwrap();
        let want = [
            "-w",
            "0",
            "new-tab",
            "--title",
            "orchard-1",
            "--suppressApplicationTitle",
            "-d",
            &tmp(),
            "claude",
            "-n",
            "orchard-1",
            "--model",
            "haiku",
            "--permission-mode",
            "plan",
            "--settings",
            "/opt/clearance/hooks.json",
        ];
        assert_eq!(args, want);
        assert_eq!(wt_args(&start("x", "", ""), &[]).unwrap(), ["-w", "0", "new-tab", "--title", "x", "--suppressApplicationTitle", "-d", &tmp(), "claude", "-n", "x"]);
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
        assert!(wt_args(&start("orchard", "", ""), &["--plugin-dir".into(), "/opt/a;b".into()]).unwrap_err().contains("\";\""));
    }

    #[test]
    fn the_macos_line_quotes_every_word() {
        let mut s = start("o", "", "");
        s.cwd = tmp();
        let line = shell_line(&s, &["--settings".into(), "/it's/here.json".into()]).unwrap();
        assert!(line.starts_with("cd '"));
        assert!(line.ends_with(" && exec 'claude' '-n' 'o' '--settings' '/it'\\''s/here.json'"), "{line}");
    }
}
