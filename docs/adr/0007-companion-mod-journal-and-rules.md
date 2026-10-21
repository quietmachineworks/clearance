# ADR-0007 - Companion mod, journal and suggested rules

Date: 2026-10-21. Status: accepted.

## Context

M3: energy, quota and instructions through the companion mod (ADR-0004), asleep sessions, a journal of
decisions and rules suggested from repeated approvals. Exit criterion: the quota visible, one rule
suggested on real data. Claude Code 2.1.304.

## Findings

- Every tool result in a transcript records how its call was allowed:
  `permissionDecision: { decision, source, reasonType }`. `source: "user_temporary"` is a "yes, this
  once" given in the terminal dialog; others are `config` (a rule, the mode, the auto mode classifier),
  `hook` and `user_permanent`. Any setup used for a while holds many approvals by hand, terminal
  included, so suggestions need no history of clearance's own.
- Many of those approvals are chains (`git status && ./release.sh prod`): approved as a whole, they say
  nothing about their first command.
- `$.plugin.root` is the mod's folder, the parent of `.claude-plugin/`.
- Live test: a background session loaded with `--plugin-dir mod` reported its context, cost and both
  rate-limit windows; an instruction typed on the page was delivered within 5 s, as the user's own
  words, and answered.

## Decision

- The mod syncs every 5 s with one request, `POST /mod/sync` with the hooks' token: the session's usage
  goes in; the number of requests waiting, and an instruction queued for this session, come back. It
  finds clearance in `data/endpoint.json`, written at start next to it: nothing to configure. With
  clearance stopped, it shows nothing and does nothing.
- One instruction per session, handed out once; the page shows queued or delivered, and says when the
  session runs no mod.
- The quota is the account's: the freshest report wins. Energy is the context left; spiral eyes from 90 %
  used.
- Asleep: idle, nothing left to read, and a transcript untouched for 3 hours. No "close" action: no safe
  channel for an interactive session.
- Decisions taken on the page, instructions and their delivery go to `data/journal.jsonl`, append-only.
- Suggested rules come from approvals by hand in the transcripts, three at least per rule. A rule is a
  prefix and allows more than what was approved, so it is offered only for a single command (no `&&`,
  `||`, `;`), only for programs whose second word is a subcommand or programs that only read, never for
  open-ended subcommands without a third word (`npm run build`, not `npm run`), never for subcommands
  that discard work (`git checkout`, `git reset`...), never for an MCP tool that writes or runs code,
  and never for an infrastructure write. clearance shows the rule to copy; it never writes settings.
