# ADR-0009 - Installing clearance for every session

Date: 2026-10-21. Status: accepted. Amends ADR-0001 and ADR-0003 on the hooks used.

## Context

Until now, a session reached clearance only when started with `--settings data/hooks.json` and
`--plugin-dir mod`. Installing for every session puts the hooks in the user's settings and the mod in
Claude Code's plugins, so clearance must be harmless whether it runs or not. Claude Code 2.1.306.

## Findings

- With clearance stopped, a refused connection is not silent for every event. A background session
  with the hooks printed `UserPromptSubmit hook error - connect ECONNREFUSED` under every prompt and
  `Stop hook error` at the end of every turn. `PermissionRequest`, `SubagentStart` and `SubagentStop`
  failed silently: the permission dialog showed as usual.
- An installed plugin runs from a copy in `~/.claude/plugins/cache/`, so a path relative to the mod
  (`$.plugin.root/..`) does not lead back to clearance.
- In auto mode, a denial held by clearance makes the session wait for the hook's timeout, even with no
  page open to answer it.

## Decision

- clearance installs only hooks that stay silent while it is stopped: `PermissionRequest`,
  `PermissionDenied`, `StopFailure` (a failure already shows an error) and `SubagentStart` /
  `SubagentStop`. The end of a turn and the start of the next are read off `claude agents` (busy or
  waiting, then idle: done; idle, then busy: read), and the done card's text from the end of the
  transcript.
- An auto mode denial is held only while a page is open; otherwise it is answered at once.
- The server writes its address and token to `~/.clearance/endpoint.json`, where the mod reads them,
  wherever the mod is installed.
- The repository is a plugin marketplace (`.claude-plugin/marketplace.json`, plugin `clearance` from
  `./mod`): `claude plugin marketplace add <folder or quietmachineworks/clearance>`, then
  `claude plugin install clearance@clearance`.
- `node src/install.ts` adds the hooks to `~/.claude/settings.json` after a timestamped backup, next to
  the user's own hooks, and replaces a previous clearance install; `--dry-run` only says what would
  change; `--remove` takes out the entries that point at clearance and nothing else.
- A session waiting in its terminal that never sent clearance a hook event is shown as not connected.
