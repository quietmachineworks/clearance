# ADR-0005 - Questions through PermissionRequest, the M1 board

Date: 2026-10-21. Status: accepted. Amends ADR-0003 on questions.

## Context

M1 exit criterion: a real permission approved from the page. Tested on Claude Code 2.1.304 with
`src/server.ts` and two background sessions (`claude --bg`, Haiku) in a throwaway trusted folder, hooks
passed with `--settings data/hooks.json`, the page driven in a browser.

## Results

| Question | Observed |
|---|---|
| A permission approved from the page | **Works.** `echo m1 > m1.txt` held by `PermissionRequest`, shown on the board, allowed from the page: the file was written within 3 s. |
| A `PreToolUse` hook holding `AskUserQuestion` | **Hides the question.** For the whole hold (60 s), the session reports `busy` and no dialog shows. At the timeout Claude Code goes through its normal permission flow: a `PermissionRequest` with `tool_name: "AskUserQuestion"` arrives, and the session reports `waiting` / `input needed` in parallel. |
| `AskUserQuestion` answered through `PermissionRequest` | **Works.** `decision: { behavior: "allow", updatedInput: { questions, answers: { "<question text>": "Blue" } } }`: the model received "Blue", the terminal dialog was never blocked. |
| 3: a real `PermissionDenied` | **Still not observed.** Asked in auto mode to run `git push --force origin main` in a repository without a remote, the model declined on its own before any tool call, as in M0, so the classifier never ran. The board handles the documented shape (`tool_name`, `tool_input`, `reason`; answer `retry: true`). |

## Decision

- Questions and plan approvals are answered through `PermissionRequest` only, like permissions, and in
  parallel with the terminal dialog. clearance installs no `PreToolUse` hook. A free answer goes back as
  a deny whose message carries the user's words, since `AskUserQuestion` only accepts option labels.
- Allowing an infrastructure write needs `GO` typed on the page, checked by the server, and never adds a
  rule (BRIEF section 2).
- `PermissionDenied` is held 60 s, the time to decide on a retry; lifecycle hooks answer at once with a
  5 s timeout.
- `CLEARANCE_LOG=1` keeps every hook payload in `data/hooks.jsonl` (ignored by git), to record the first
  real `PermissionDenied`.
- A second avatar pack, robots, ships with the board: same symbols, another costume, chosen on the page.
  The model accessory waits for M2, where the model is read from the transcript.

## Still open

- 3: a real `PermissionDenied` payload, on the first denial met in real use.
