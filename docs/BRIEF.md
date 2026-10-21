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
| Fallback | n/a | the terminal dialog stays open in parallel, first answer wins |

Deliberate choice: **Claude Code only**. No other agent CLI. This allows full use of hooks, modes,
subagents, workflows and permission rules.

## 3. V1 scope

### Board

Four areas, in this order:

1. **Waiting for you**: permission, plan approval, question, auto mode denial. Sorted by risk, then age.
   A card opens in place and carries the decision.
2. **Done**: turn finished, not read yet. Read when you type a prompt in that session, or on click.
3. **Working**: in progress, with their clones (subagents) and energy (context left).
4. **Asleep**: idle for 3 hours with nothing left to read, candidates to close.

Possible decisions:

- permission: allow, deny with a reason sent to the agent, allow and add a rule (never for an
  infrastructure write);
- plan approval: plan, exact commands, risks, rollback; you must type `GO`;
- question: offered options or a free answer;
- auto mode denial: allow a retry, confirm the denial, switch the session to ask;
- session mode: auto, ask, plan, edits (no bypass).

Parallel with the terminal: Claude Code keeps its own dialog open while clearance holds a request;
on timeout or with the server stopped, the dialog simply stays (ADR-0003). clearance never blocks
anyone. The mode selector is offered on pending requests, since a mode change travels with an answer.

Plan approvals become visible through one convention in the user's `CLAUDE.md`: present them with
`AskUserQuestion` (options GO / No). The session then reports `waitingFor`, and clearance can answer.
The card shows the question and the option descriptions, so the plan, its commands, risks and rollback
belong there.

### Session detail

Generic, valid for any workflow and for a session without one:

- header: avatar, branch, model, mode, what it is doing, energy, quota;
- progress: the phases the workflow declares, the current one;
- the current chapter: one row per step, parallel agents side by side, each with duration, tools,
  errors, result; what is still to come drawn dashed;
- diff: the Edit and Write calls of the session and its agents, from their transcripts (ADR-0006);
- errors and loops: tools in error, the same command failing 3 times in a row, stop on error
  (`StopFailure`);
- what ran: the session's thread, the user's prompts as speech bubbles;
- workflow history of the project: closed, interrupted, ended without closure;
- actions: go to the terminal (M4), send an instruction (M3). Stopping one workflow is left out: no
  documented way to stop a run without stopping its session (ADR-0006).

### Usage data

Context left, 5-hour and 7-day quota, session cost: read by clearance's companion mod with
`$.session.usage()` (ADR-0004). The same mod shows what waits for the user under every prompt.

### Later, already decided

- go to a session's terminal, start a session in a **real terminal tab** (not `--bg`): done in M4
  (ADR-0008);
- decision journal and suggested rules;
- avatar packs: robots (M1) and Halloween (M4) ship; more later;
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
| Approve a permission | `PermissionRequest` hook of type `http`, answer `decision.behavior` allow or deny | documented, tested (M1) |
| Answer a question | `PermissionRequest` on `AskUserQuestion`, `decision.updatedInput.answers` (ADR-0005) | tested (M1) |
| Auto mode denial | `PermissionDenied` hook | documented, real payload not seen yet |
| End of turn, read | `Stop` hook (`last_assistant_message`), then `UserPromptSubmit` | documented |
| Subagents | `SubagentStart` / `SubagentStop` hooks; `~/.claude/projects/<project>/<session>/subagents/agent-<id>.jsonl` and `.meta.json` (`agentType`, `workflowPhase`) | observed |
| Workflows | `.../subagents/workflows/<run>/journal.jsonl`: `launched`, `started` (`label`, `phase`, `agentId`) and `result` lines; phases declared in the script's `meta.phases`; `toolUseResult.runId` links the `Workflow` call to its run; `<session>/workflows/<run>.json` once it ends (ADR-0006) | observed, followed live in M2 |
| An agent's model, tools, errors | the agent's transcript: `message.model`, `tool_use`, `tool_result.is_error`, `timestamp` | observed |
| Stop on error | `StopFailure` hook (`rate_limit`, `authentication_failed`...) | documented |
| Context, quota, cost | companion mod: `$.session.usage()` (`context`, `rateLimits`, `cost`) | documented, tested |
| Approvals given by hand | transcript `permissionDecision` on tool results, `source: "user_temporary"` (ADR-0007) | observed |
| Instruction to a session | companion mod: `$.prompt.submit({ text, asUser })` | documented, tested |
| Session title, terminal tab | transcript `ai-title` records; Windows Terminal tabs through UI Automation | observed, tested |
| Relay in parallel with the terminal | channels declaring `claude/channel/permission` | research preview, dev flag, organisation policy |

Internal files (`~/.claude/sessions/`, transcripts) may change between versions: prefer a documented
source whenever one exists, and keep every read of internal files in a single module.

## 6. Target architecture

- one Node 24 process, TypeScript run without a build step, **zero dependencies**;
- `node:http` on `127.0.0.1` only; the page gets updates over SSE;
- session state: `claude agents --json` every 2 s;
- Claude Code hooks of type `http` to `/hook/<event>`; server stopped = connection refused = Claude
  Code behaves as usual;
- a companion mod in every session for usage figures, instructions and the line under the prompt;
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
| M1 | Board, decisions, browser notifications | a real permission approved from the page |
| M2 | Session detail, subagents, workflows, errors and loops, diff | a multi-agent workflow followed end to end |
| M3 | Companion mod (energy, quota, instructions), asleep sessions, journal and suggested rules | quota visible, one rule suggested on real data |
| M4 | Terminal (go to, start in a tab), avatar pack | start a session from the tower |

## 8. Open questions for M0

Results: ADR-0003.

| # | Question | Status |
|---|---|---|
| 1 | Terminal dialog while a `PermissionRequest` hook waits, timeout, empty answer | settled: dialog shown in parallel, timeout leaves it open |
| 1b | Human answers in the terminal first: is the hook request closed, a late answer ignored? | settled: terminal wins, late answer ignored, closure not reliable |
| 2 | `updatedPermissions` with `setMode` | settled: works, destination `session` |
| 3 | Shape of `PermissionDenied`, retry | from the docs; not triggered in M1 either (ADR-0005), recorded on the first real denial |
| 4 | `AskUserQuestion` answered through `updatedInput.answers` | settled: works, through `PermissionRequest` (ADR-0005) |
| 5 | Channels allowed by the organisation policy; can the relay replace the hook? | open, and less needed now that hook and dialog run in parallel |
| 6 | Send an instruction to a session | settled another way: companion mod, `$.prompt.submit` (ADR-0004); socket format left aside |
| 7 | Find and bring to front the terminal tab of a session (Windows Terminal first) | settled: title match and UI Automation (ADR-0004) |
| 8 | Where the Workflow tool persists its script | settled: `<session>/workflows/<runId>.json` |

## 9. Constraints

- M0 tests run in an isolated project with its own `.claude/settings.json`, never in the user's global
  settings while the behaviour is unknown.
- Nothing that needs administrator rights.
- No script written to a temp directory and run with a bypassed execution policy.
