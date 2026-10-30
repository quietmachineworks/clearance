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
4. **Asleep**: idle for 3 hours with nothing left to read, candidates to close: "close the asleep
   sessions" quits them after showing the memory it frees (ADR-0015).
5. **Stopped**: no longer running, kept with what it takes to resume them until the user deletes them
   (ADR-0015).

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
- actions: its terminal in the page (ADR-0016), send an instruction (M3). Stopping one workflow is left out: no
  documented way to stop a run without stopping its session (ADR-0006).

### Usage data

Context left, 5-hour and 7-day quota, session cost: read by clearance's companion mod with
`$.session.usage()` (ADR-0004). The same mod shows what waits for the user under every prompt.

### Later, already decided

- start a session in its own terminal: done in M4 in a terminal tab, replaced by a terminal inside the page (ADR-0016); clearance drives no terminal application
  (ADR-0008);
- decision journal and suggested rules;
- avatar packs: robots (M1) and Halloween (M4) ship; more later;
- keyboard shortcuts.

### Out of scope

Video replay, voices, other agent CLIs, cloud or desktop-app sessions, access from outside the machine.

## 3b. V2 scope (mockup V2-A, 2026-10-22)

The target is the V2-A mockup on the design canvas: one queue and one session pane, for many sessions
at once, mostly from the keyboard.

- **Queue** (left, folds into a rail of avatars): waiting for you (production and unknown environments
  first, then permissions, questions, plans), to watch (loops, repeated errors, context almost full),
  to read, working; asleep and ignored folded away. New requests do not move the queue while the pointer
  or focus is in it: a "new" marker shows them. A request answered in the terminal shows as settled.
- **Compact queue** (2026-10-30, mockup `design/canvas/QueueCompact.dc.html`): about ten sessions per
  group must fit. Outside the waiting cards, one 32 px line per session, no avatar: name, the session's
  title, then its state on the right (context gauge of 5 cells while it works, the time it finished, a
  cross when it failed, how long it has been idle). No state subheadings: to read, working, idle, in that
  order. A slim group header, its mute and interrupt actions on hover or keyboard focus; asleep, stopped
  and ignored as small counters that open in place. Waiting cards keep the command, with the request's
  icon in place of the avatar. The queue's width is dragged between 300 and 640 px, kept per browser.
- **Groups** (ADR-0015): made by the user, sessions moved into them, "No group" for the others. What
  waits for you and what needs watching stay on top across groups; below, one foldable block per group
  (to read, working, idle, asleep, stopped, ignored) with its counts and memory. Group actions: mute,
  interrupt.
- **Memory**: each session's processes, per group and in total (ADR-0015).
- **Session pane**: the thread (prompts, the assistant's text, tool calls with their outcome), the
  decision card in place, a field to write to the session with its delivery state (files pasted or dropped there or on the terminal go in as paths); the session's real terminal for the sessions clearance runs, a terminal view for the others (ADR-0016).
  Inspector beside it: the task, the plan and its current step, files changed, tests, subagents, cost and
  its rate, the model for the next turn.
- **Decisions**: after an answer the next request opens; every answer can be taken back for 5 seconds.
  Infrastructure writes show their environment (ADR-0012) and what clearance checked apart from what the
  agent claims; no "always allow" for them. Stop asks for confirmation; interrupt does not.
- **To read**: facts first (tests, files, commits, merge request), then validate, ask for a fix, open
  the diff.
- **New task**: instruction, recent folder, group from the folder, model, mode; the session opens in a
  terminal inside the page (ADR-0016); folders are browsed in the page, never through a system dialog.
- **Thread** (2026-10-30, mockup `design/canvas/ThreadTurns.dc.html`): told in turns, not the terminal
  copied. Each message you sent opens one: below it, a summary line (how long, how many actions) and
  result chips (files changed, tests, commits, merge request, failures), then the reply in full, which
  copies as Markdown. What it did unfolds as sentences ("Read 6 files", "Ran `npm test` → 32 pass", a
  failure in red with its first line, "Published the artifact « Export screen » (version 3)"), each with
  how a permission it needed was settled when it was not automatic (by you, from clearance, refused,
  blocked by auto mode: the transcript's `permissionDecision`). Turns are folded but the running one and
  one whose last action failed. "All | My messages" keeps only your requests with their summary line. A
  resume after compaction is a line, not a message; what a plugin, a notification or a continuation sent
  in your place (the transcript's `origin`) is not a request and stays out.
- **Right panel**: hidden down to a column of icons with their counts, or dragged between 240 and 520 px,
  both kept per browser. Task: the session's title, then its first request on three lines. The
  artifacts the session published (title, version, when; opened on claude.ai, no preview: they are
  private); when a handoff plugin started it, those of the session it continues, one handoff back, under
  "Before the resume", dashed; then a link to the whole gallery on claude.ai. Nothing overflows sideways.
- **Attachments**: an image pasted, a file dropped anywhere on the session or picked with the paperclip
  waits as a chip above the message field (thumbnail, size, remove), and goes with the message: its path
  at the end of the text. In the Terminal tab, paste and drop still type the path.
- **Shortcuts**: one global list of prompts (`~/.clearance/prompts.json`), as chips above the message
  field and on Ctrl+Shift+1 to 0, sent to the session on screen. Messages for a session queue up and
  are delivered one by one, none overwrites another.
- **Search** (Ctrl K): case and accents ignored, every word, in any order.
- **Version**: the build's version and commit, and when clearance started, in the Settings.
- **Freshness**: when `claude agents` stops answering, the page says its data is stale and since when;
  a Claude Code version outside the validated range allows nothing from the page (ADR-0017).
- **Quota**: 5-hour and 7-day gauges with the time they run dry at the current rate.
- Skins, languages and the logo: ADR-0011.

## 4. Visual grammar

Manga style, chibi avatars drawn for the project, where there is room for them: the session's header,
the folded queue, the search. The compact queue has none: there, the state is said in words, a gauge and
one small animation per state (dots that wave while it works, a flash when a request arrives, a slow pulse
on what needs watching, a check that pops once a turn ends), nothing moving at rest. Every trait carries
information, no decoration:

| Trait | Meaning |
|---|---|
| Hair colour | the group (the folder for a session in no group), the same everywhere |
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

- an avatar pack changes the costume, never the meaning layer; a skin (ADR-0011) changes the look,
  never the meaning either;
- the logo is the same in every skin;
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
| End of turn, read | `claude agents` status, busy or waiting then idle, idle then busy; last message from the transcript (ADR-0009: the `Stop` and `UserPromptSubmit` hooks print errors while clearance is stopped) | observed |
| Subagents | `SubagentStart` / `SubagentStop` hooks; `~/.claude/projects/<project>/<session>/subagents/agent-<id>.jsonl` and `.meta.json` (`agentType`, `workflowPhase`) | observed |
| Workflows | `.../subagents/workflows/<run>/journal.jsonl`: `launched`, `started` (`label`, `phase`, `agentId`) and `result` lines; phases declared in the script's `meta.phases`; `toolUseResult.runId` links the `Workflow` call to its run; `<session>/workflows/<run>.json` once it ends (ADR-0006) | observed, followed live in M2 |
| An agent's model, tools, errors | the agent's transcript: `message.model`, `tool_use`, `tool_result.is_error`, `timestamp` | observed |
| Stop on error | `StopFailure` hook (`rate_limit`, `authentication_failed`...) | documented |
| Context, quota, cost | companion mod: `$.session.usage()` (`context`, `rateLimits`, `cost`) | documented, tested |
| Approvals given by hand | transcript `permissionDecision` on tool results, `source: "user_temporary"` (ADR-0007) | observed |
| Instruction to a session | companion mod: `$.prompt.submit({ text, asUser })`; the first one of a session clearance starts: `claude -- <text>` (ADR-0016) | documented, tested |
| Session title | transcript `ai-title` records | observed |
| Claude Code version | `claude --version`, checked against the validated range (ADR-0017) | tested |
| A session's terminal | clearance's own pseudo-terminal: ConPTY on Windows, `openpty` on macOS (ADR-0016) | tested |
| Relay in parallel with the terminal | channels declaring `claude/channel/permission` | research preview, dev flag, organisation policy |
| A session's plan and current step | the last `TodoWrite` call of the transcript (`todos`: `content`, `status`) | to check in R1 |
| Tests, commits, merge request of a turn | the turn's `Bash` calls and their results (test commands, `git commit`, a merge request URL) | heuristic, R2 |
| Model for the next turn | `/model <name>` sent as an instruction through the companion mod | to check in R2 |
| Interrupt, stop a session | companion mod: `$.turn.abort`, `$.command.run('exit')` | tested |
| Memory of a session | `pid` from `claude agents --json`, the system's process table (Toolhelp32, `ps`) | ADR-0015 |
| A session going on under a new id | the same `pid` with another `sessionId` | to check on real `/clear` |

Internal files (`~/.claude/sessions/`, transcripts) may change between versions: prefer a documented
source whenever one exists, and keep every read of internal files in a single module.

## 6. Target architecture

- one Rust binary, standard library only, **zero dependencies** (ADR-0010), on Windows and macOS;
- HTTP on `127.0.0.1` only; the page gets updates over SSE;
- session state: `claude agents --json` every 2 s while a page is open, every 10 s otherwise;
- Claude Code hooks of type `http` to `/hook/<event>`; server stopped = connection refused = Claude
  Code behaves as usual;
- a companion mod in every session for usage figures, instructions and the line under the prompt;
- sessions started from clearance run in its own pseudo-terminals (ConPTY, `openpty`), reopened with
  `claude --resume` at its next start; the page draws them with xterm.js, the only code not written for
  clearance, frozen in the repository with its hashes (ADR-0016);
- local append-only journal of events and decisions (JSONL);
- page in plain HTML, CSS and JS, avatars as inline SVG;
- risk classification (read, write, infrastructure write) in one rules module.

Security: an "Allow" button is a right to run code on the machine.

- listen on 127.0.0.1, never on another interface;
- two secrets (ADR-0013): the hook secret, which every connected session can read and which only lets
  it report events; and the browser's own session, obtained through a single-use login link, which
  nothing an agent can read gives away. The settings clearance writes deny the file tools on its folder;
- `Origin` checked on every request that writes;
- a strict content security policy: the page's own script and styles only, fonts served by clearance
  (ADR-0014);
- no infrastructure write can be approved without seeing the full command;
- the risk class is a signal, not a guarantee: program names are normalised, MCP tools with a writing
  verb count as infrastructure writes, tricky commands are kept as tests (ADR-0017);
- the journal keeps about 10 MB, two files (ADR-0017).

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
| R1 | The server in Rust (ADR-0010), Windows and macOS | every Node test passes in Rust, a real permission answered |
| R2 | The V2-A page: queue, session pane, inspector, skins, languages, logo | a day of real work from it |
| R3 | Interrupt and stop a session, a real terminal in the page (ADR-0016), groups and memory (ADR-0015), two secrets (ADR-0013) | a day of work without a terminal application |

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
- CI on Windows and macOS for every push; a `v*` tag publishes the binaries (ADR-0017).
