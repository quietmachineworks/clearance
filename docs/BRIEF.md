# clearance - product brief

A local control tower for the Claude Code sessions running in parallel on one machine. It shows who is
waiting for you, lets you answer from one page, and unfolds a session down to its subagents.

This document is the source of truth. What is not in it is out of scope.

## 1. The problem

With 5 to 10 sessions open in as many terminals, the bottleneck is no longer the agent, it is the
developer:

- sessions sit blocked on a permission prompt and nobody notices;
- approving means finding the right terminal first;
- a plan approval given in the chat (plan, exact commands, risks, rollback, then "go") leaves the session
  `idle`, which looks exactly like a session that is done;
- auto mode denials go unseen;
- finished sessions pile up unread, some of them for hours;
- a multi-agent workflow (a dev / ci / review / qa pipeline, a scout / architect / critic analysis) can
  only be followed by reading its journal by hand.

## 2. What clearance does differently

Prior art studied: agent-standup.com, an observation tool that replays transcripts, guesses who "may
need you" and stages a meeting. It does not let you approve or answer anything.

| | agent-standup | clearance |
|---|---|---|
| Who is waiting | inferred from transcripts | real state from Claude Code (`waitingFor`) |
| Answer | no | permission, plan approval, question, from the page |
| Priority | everyone equal | queue sorted by risk, then age |
| Risk | nothing | exact command, risk class, typed `GO` for infrastructure writes |
| Auto mode denials | invisible | surfaced, retry possible |
| Session mode | no | auto, ask, plan, edits |
| Fewer interruptions | no | rules suggested from repeated approvals |
| Finished sessions | "done" | to read, then read, then asleep |
| Fallback | n/a | page closed = everything goes back to the terminal |

Deliberate choice: **Claude Code only**. No other agent CLI. This allows full use of hooks, modes,
subagents, workflows and permission rules.

## 3. V1 scope

### Board

Four areas, in this order:

1. **Waiting for you**: permission, plan approval, question, auto mode denial. Sorted by risk, then age.
   A card opens in place and carries the decision.
2. **Done**: turn finished, not read yet. Read when you type a prompt in that session, or on click.
3. **Working**: in progress, with their clones (subagents) and energy (context left).
4. **Asleep**: idle for several hours, candidates to close.

Possible decisions:

- permission: allow, deny with a reason sent to the agent, allow and add a rule (never for an
  infrastructure write);
- plan approval: plan, exact commands, risks, rollback; you must type `GO`;
- question: offered options or a free answer;
- auto mode denial: allow a retry, confirm the denial, switch the session to ask;
- session mode: auto, ask, plan, edits (no bypass).

Tower mode: clearance holds a request only while the page is open. Page closed or server stopped,
Claude Code behaves as usual in the terminal.

Plan approvals become visible through one convention in the user's `CLAUDE.md`: present them with
`AskUserQuestion` (options GO / No). The session then reports `waitingFor`, and clearance can answer.

### Session detail

Generic, valid for any workflow and for a session without one:

- header: avatar, branch, model, mode, what it is doing, energy, quota;
- progress: the phases the workflow declares, the current one;
- the current chapter: one row per step, parallel agents side by side, each with duration, tools,
  errors, result; what is still to come drawn dashed;
- diff: `git diff` of the branch, or an agent's Edit and Write calls from its transcript;
- errors and loops: tools in error, the same command failing 3 times in a row, stop on error
  (`StopFailure`);
- what ran: the session's thread, the user's prompts as speech bubbles;
- workflow history of the project: closed, interrupted, ended without closure;
- actions: go to the terminal, send an instruction, stop the workflow (with confirmation).

### Statusline data

Context left and 5-hour quota, forwarded by the user's statusline command to clearance.

### Later, already decided

- go to a session's terminal, start a session in a **real terminal tab** (not `--bg`);
- decision journal and suggested rules;
- avatar packs (first candidate: Halloween);
- keyboard shortcuts.

### Out of scope

Video replay, voices, other agent CLIs, cloud or desktop-app sessions, access from outside the machine.

## 4. Visual grammar

Manga style, chibi avatars drawn for the project. Every trait carries information, no decoration:

| Trait | Meaning |
|---|---|
| Hair colour | the project, the same everywhere |
| Glasses / headphones / cowlick | Opus / Sonnet / Haiku |
| Speed lines | working |
| Sweat drop | waiting for a permission |
| "!" | waiting for a GO |
| "?" | asking a question |
| Anger vein | blocked by auto mode |
| Sparkles | done |
| Zzz | asleep for hours |
| Spiral eyes | context almost full |
| Smaller clones | subagents |

Rules:

- an avatar pack changes the costume, never the meaning layer;
- original drawings only (or CC0 assets), fonts under the SIL Open Font License; no character, name or
  visual taken from an existing work;
- no decorative tilt;
- `prefers-reduced-motion` turns every animation off.

Mockups: `design/`.

## 5. Data sources (checked on 2026-10-20, Claude Code 2.1.304)

| Need | Source | Status |
|---|---|---|
| Sessions and their state | `claude agents --json`: `sessionId`, `name`, `cwd`, `kind`, `status` (busy, idle, waiting), `waitingFor`, `state` for background sessions | documented |
| Approve a permission | `PermissionRequest` hook of type `http`, answer `decision.behavior` allow or deny | documented, to test |
| Answer a question | `PreToolUse` hook on `AskUserQuestion`, `updatedInput.answers` | documented, to test |
| Auto mode denial | `PermissionDenied` hook | to test |
| End of turn, read | `Stop` hook (`last_assistant_message`), then `UserPromptSubmit` | documented |
| Subagents | `SubagentStart` / `SubagentStop` hooks; `~/.claude/projects/<project>/<session>/subagents/agent-<id>.jsonl` and `.meta.json` (`agentType`, `workflowPhase`) | observed |
| Workflows | `.../subagents/workflows/<run>/journal.jsonl`: `launched`, `started` (`label`, `phase`, `agentId`) and `result` lines; phases declared in the script's `meta.phases` | observed on 18 runs |
| An agent's model, tools, errors | the agent's transcript: `message.model`, `tool_use`, `tool_result.is_error`, `timestamp` | observed |
| Stop on error | `StopFailure` hook (`rate_limit`, `authentication_failed`...) | documented |
| Context, quota | statusline stdin: `context_window.*`, `rate_limits.*` | documented |
| Instruction to a session | cross-session messaging socket and token | wire format undocumented |
| Relay in parallel with the terminal | channels declaring `claude/channel/permission` | research preview, dev flag, organisation policy |

Internal files (`~/.claude/sessions/`, transcripts) may change between versions: prefer a documented
source whenever one exists, and keep every read of internal files in a single module.

## 6. Target architecture

- one Node 24 process, TypeScript run without a build step, **zero dependencies**;
- `node:http` on `127.0.0.1` only; the page gets updates over SSE;
- session state: `claude agents --json` every 2 s;
- Claude Code hooks of type `http` to `/hook/<event>`; server stopped = connection refused = Claude
  Code behaves as usual;
- local append-only journal of events and decisions (JSONL);
- page in plain HTML, CSS and JS, avatars as inline SVG;
- risk classification (read, write, infrastructure write) in one rules module.

Security: an "Allow" button is a right to run code on the machine.

- listen on 127.0.0.1, never on another interface;
- a random token required on hooks and actions;
- `Origin` checked on every request that writes;
- no infrastructure write can be approved without seeing the full command.

`http` hooks also spawn no process per event: faster, and friendlier to endpoint security tools than
`command` hooks.

## 7. Milestones

| Milestone | Content | Exit criterion |
|---|---|---|
| M0 | Technical spikes (section 8) in an isolated project | each question settled by an ADR |
| M1 | Board, decisions, tower mode, browser notifications | a real permission approved from the page |
| M2 | Session detail, subagents, workflows, errors and loops, diff | a multi-agent workflow followed end to end |
| M3 | Statusline (energy, quota), asleep sessions, journal and suggested rules | quota visible, one rule suggested on real data |
| M4 | Terminal (go to, start in a tab), avatar pack | start a session from the tower |

## 8. Open questions for M0

1. Does the terminal dialog stay visible while a `PermissionRequest` hook waits? What happens on
   timeout? Does an empty answer hand back to the terminal?
2. Does `updatedPermissions` with `setMode` change the session's mode?
3. Shape of `PermissionDenied`; can a retry happen without sending a message?
4. `AskUserQuestion` answered through `updatedInput.answers` while the hook waits.
5. Are channels allowed by the organisation policy? If so, can the parallel relay replace the blocking
   hook?
6. Wire format of the cross-session messaging socket.
7. Find and bring to front the terminal tab of a session (Windows Terminal first).
8. Where the Workflow tool persists its script (to read `meta.phases`).

## 9. Constraints

- M0 tests run in an isolated project with its own `.claude/settings.json`, never in the user's global
  settings while the behaviour is unknown.
- Nothing that needs administrator rights.
- No script written to a temp directory and run with a bypassed execution policy.
