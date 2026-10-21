# ADR-0008 - Terminal tabs from the tower

Date: 2026-10-21. Status: accepted. Builds on ADR-0004.

## Context

M4: start a session in a real terminal tab from the page, bring a session's tab to the front, and a
second costume pack. Exit criterion: start a session from the tower. Windows Terminal first; Claude
Code 2.1.306.

## Findings

- `wt.exe -w 0 new-tab --title <name> --suppressApplicationTitle -d <folder> claude -n <name> ...` opens
  the tab in the current window, and the session shows in `claude agents` under that name.
- **A new tab inherits the environment of the process that called `wt`.** With clearance itself
  started from inside a Claude Code session, the tab got that session's markers
  (`CLAUDE_CODE_CHILD_SESSION`, `CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`, the messaging socket and token,
  ...): it ran as a child session, with no transcript and absent from `claude agents`.
- `wt` splits its own command line on `;`.
- UI Automation sees each tab as a `TabItem` named after its title, Claude Code's status glyph first.
  Windows PowerShell 5.1, always present, runs the lookup as an inline `-Command`; its
  `ConvertFrom-Json` does not unroll a JSON array, so the titles go in one per line.
- A browser never hands a page the full path of a folder the user picks.

## Decision

- **Start**: `src/terminal.ts` builds the `wt` arguments, refuses anything odd first (a missing folder,
  a name outside `[\w.-]{1,40}`, an unknown model or mode, a `;` anywhere), and removes the session
  markers from the tab's environment. The tab keeps the session's name as its title, so it can be
  found again. Connected to clearance by default: `--settings data/hooks.json --plugin-dir mod`.
  No first prompt (the `;` split): an instruction through the companion mod does that job.
- **Folder picker**: the server lists the sub-folders of a path (hidden ones left out) and the drives;
  the page offers the folders of the live sessions as recent ones.
- **Go to terminal**: Windows PowerShell, inline script, no file, no execution policy change, the titles
  passed through the environment, never into the script text. It matches the session's title from its
  transcript, or its name; when nothing matches, the page says which tab to look for.
- **Halloween pack**: a witch's hat and a cape over the manga chibi. The hair keeps the project's
  colour and every state symbol stays (BRIEF section 4).
