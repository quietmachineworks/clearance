# ADR-0001 - Local web page, dependency-free Node 24, http hooks

Date: 2026-10-20. Status: accepted; tower mode superseded by ADR-0003.

## Context

We need to see the state of every Claude Code session on the machine and answer their requests from one
place. Options considered: a pane inside a Claude Code session (mod), a local web page, access from a
phone.

## Decision

- **Local web page**: the only surface with room for a real visual identity. Phone access is out: a page
  that approves commands, exposed beyond `localhost`, is a disproportionate risk.
- **Node 24, TypeScript run without a build step, zero dependencies**: already on the machines we
  target, nothing to install, nothing to audit.
- **Session state from `claude agents --json`**, the documented source, rather than the files under
  `~/.claude/sessions/`.
- **Requests through `http` hooks** (`PermissionRequest`, `PreToolUse` on `AskUserQuestion`,
  `PermissionDenied`, `Stop`, `SubagentStart` / `SubagentStop`, `StopFailure`): no process spawned per
  event, and a stopped server means a refused connection, so Claude Code behaves as usual.
- **Tower mode**: a request is held only while the page is open; otherwise the hook answers at once with
  no decision and the terminal dialog shows.

## Consequences

- The exact behaviour of a blocking hook (terminal dialog hidden or not, timeout) is not documented: it
  is question 1 of M0. If the parallel relay of channels is allowed by the organisation, it may replace
  the blocking hook; an ADR will settle it.
- Reading internal files (transcripts, `subagents/workflows/*/journal.jsonl`) is required for the
  session detail; it stays in one module.
