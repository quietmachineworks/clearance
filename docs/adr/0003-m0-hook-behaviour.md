# ADR-0003 - How Claude Code behaves with blocking http hooks (M0 results)

Date: 2026-10-20. Status: accepted. Partly supersedes ADR-0001 (tower mode).

## Context

Questions 1 to 4 and 8 of section 8 of the brief, tested on Claude Code 2.1.304 with
`spike/hook-server.ts`: background sessions (`claude --bg`, Haiku, `--permission-mode default`) in a
throwaway trusted folder, hooks passed with `--settings spike/hooks.json`, state read with
`claude agents --json` every 2 s.

## Results

| # | Question | Observed |
|---|---|---|
| 1 | Is the terminal dialog shown while a `PermissionRequest` hook waits? | **Yes.** About 2 s after the hook call, the session reports `waiting` / `blocked` / `permission prompt`, while the hook is still held. The `permission_prompt` Notification fires about 6 s in. An `allow` sent after 20 s and after 30 s is applied: the command runs. |
| 1 | What happens on timeout? | Claude Code closes the hook's HTTP request at the timeout (14.97 s for `timeout: 15`) and the dialog **stays open**. No allow, no deny: the command does not run until someone answers. |
| 1 | Empty answer? | Not run: same path as a timeout per the docs ("staying silent doesn't approve it"). |
| 2 | Does `updatedPermissions` with `setMode` change the mode? | **Yes.** `allow` with `{ "type": "setMode", "mode": "acceptEdits", "destination": "session" }`: the next Bash call asked nothing, and the following `Stop` hook reports `permission_mode: "acceptEdits"`. |
| 3 | `PermissionDenied` | Not observed: the test session refused the command itself (its own instructions) before any tool call, so the classifier never ran. Shape taken from the docs: input `reason` (e.g. `[Git Destructive]`), output `hookSpecificOutput.retry: true` adds a message telling the model it may retry, the denial itself stands. To verify on real events in M1. |
| 4 | `AskUserQuestion` answered through `updatedInput.answers` | **Yes.** `permissionDecision: "allow"` with the original `questions` and `answers: { "<question text>": "Blue" }`: the model received "Blue". The question text follows the session's language; the answer must be one of the option labels. |
| 8 | Where is a workflow's script? | `~/.claude/projects/<project>/<session>/workflows/<runId>.json`, field `script`, which holds `meta.phases`. |

Other findings:

- `PermissionRequest` carries `permission_suggestions` (for instance `addDirectories` with
  `destination: "session"`): Claude Code's own proposal for "always allow", usable as is.
- `Stop` carries `last_assistant_message`: the text for the "Done" card.
- A `SubagentStop` with an empty `agent_type` fired for an internal agent: filter it out.
- `claude --bg` refuses an untrusted folder ("Workspace not trusted"); a background session must
  isolate itself in a worktree before writing to a shared git repository, and cannot when the
  repository has no commit. Starting sessions from clearance (M4) goes through a real terminal tab,
  which also sidesteps both.

## Decision

- **No tower mode.** Claude Code keeps its own dialog open while clearance holds a request, and a
  timeout simply leaves the dialog in place. clearance can therefore hold every request until its
  timeout, whether the page is open or not, without ever blocking anyone. The "tower active / idle"
  toggle goes away.
- `PermissionRequest` hook timeout: 600 s (the default). A request still pending when the connection
  closes leaves the board.
- Mode changes are sent with the answer to a request (`setMode`, destination `session`). A session that
  asks nothing cannot have its mode changed from clearance: the mode selector is shown on pending
  requests only.
- "Always allow" uses `permission_suggestions` when present, then the classified rule.

## Still open

- 1b: when the human answers in the terminal first, does Claude Code close the hook's request at once
  (so the card can leave the board), and is a late answer from clearance ignored? Needs a human at an
  attached terminal; one manual test.
- 3: a real `PermissionDenied` payload (M1).
- 5, 6, 7: channels policy, messaging socket, terminal tab.
