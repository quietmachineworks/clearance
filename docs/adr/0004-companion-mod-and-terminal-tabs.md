# ADR-0004 - A companion mod for usage and instructions, terminal tabs by title

Date: 2026-10-20. Status: accepted.

## Context

Questions 6 and 7 of section 8 of the brief: send an instruction to a running session, and bring its
terminal tab to the front. Plus a need from section 3: each session's context and the account's quota.

## Findings

**Cross-session socket.** Documented up to the auth line (`{"type":"auth","token":"<token>"}`, required
on native Windows, token from `CLAUDE_CODE_MESSAGING_TOKEN`); the message format after it is not.
Recovering it would mean taking the Claude Code binary apart, which we rule out.

**Mods API.** Documented, and covers it. Spike: `spike/probe-mod`, loaded in one background session only
with `--plugin-dir`, polling `spike/hook-server.ts` every 5 s.

- `$.session.usage()` returned real figures: context (`tokens`, `window`, `percent`), `rateLimits`
  (`five_hour` and `seven_day` with `percentUsed` and `resetsAt`) and `cost.usd`. These are the
  statusline's own figures, with no statusline change.
- An instruction queued on the receiver was fetched by the mod and submitted with
  `$.prompt.submit({ text })`: the session started a turn, read it as "The clearance-probe plugin sent a
  message: ...", and answered as asked. `asUser: true` would present it as the user's own words.
- `$.ui.status(text)` writes a line under the prompt of the session it runs in.

**Terminal tabs (Windows Terminal).** Each interactive `claude.exe` runs under its own shell, all under
one `WindowsTerminal.exe`. A tab's name is the title Claude Code gives the session, preceded by a status
glyph, and the transcript records that title as `{"type":"ai-title","aiTitle":"..."}`. UI Automation
lists the tabs (`TabItem`) and `SelectionItemPattern.Select()` brings one to the front: tested on a
live tab, then back. Sessions that have no title yet all read "Claude Code".

## Decision

- clearance ships a **companion mod**, loaded in every session once the user installs it:
  - posts `$.session.usage()` to clearance (energy, quota, cost) - replaces the statusline route;
  - fetches instructions queued for its session and submits them, `asUser: true` when the user typed
    them in clearance;
  - shows a one-line count of what waits for the user under every prompt (`$.ui.status`).
  Hooks stay the channel for permissions, questions and lifecycle events (ADR-0003). Installing the mod
  and the hooks in the user's settings is an explicit step, never automatic.
- **Go to the terminal**: match the session's latest `aiTitle` with the tab names (glyph stripped),
  then select the tab through UI Automation. Untitled or duplicate titles fall back to telling the user
  which session to look for. Windows Terminal first; other terminals later.
- The cross-session socket is not used.

## Consequences

- The mod authenticates to clearance with the same token as the hooks; anything that can reach
  clearance with it can type into every session, hence `127.0.0.1` only and the `Origin` check.
- Mods need Claude Code 2.1.287 or later in the terminal.
- Focusing a tab is Windows-specific code, kept in one module.
