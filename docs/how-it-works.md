# How clearance works

What clearance reads, what it sends, what it refuses, and why. The README says what it does and how to
run it; this page is for anyone who changes it or wants to know what an "Allow" on the page goes through.

## The parts

- **One binary** (`src/`), Rust stable, standard library only, for Windows and macOS. It serves the page
  on `127.0.0.1`, receives Claude Code's hooks, polls `claude agents --json`, reads transcripts, runs the
  sessions started from the page in pseudo-terminals, and keeps its state in `~/.clearance/` (or
  `CLEARANCE_HOME`).
- **The page** (`public/index.html`): plain HTML, CSS and JS in one file, built into the binary and read
  from the repository while it is there, so editing it needs no rebuild. Updates arrive over server-sent
  events.
- **Hooks** of type `http`, posted to `/hook/<event>`: permissions, questions, plan approvals, auto mode
  denials, subagents. Never `command` hooks: no process per event, and a stopped clearance is a refused
  connection, so Claude Code behaves as usual.
- **The companion mod** (`mod/`), a Claude Code plugin loaded in each session: usage figures, messages
  and slash commands typed on the page, interrupt and stop, a line under the prompt.
- **xterm.js 5.5.0** with its fit addon, the only code not written for clearance, frozen in
  `public/vendor/xterm/` with its licence and SHA-256, checked by a test, loaded only when a Terminal tab
  opens.

A crate would need a reason good enough to be written here. What one would give is written in the
repository instead, small and tested: an HTTP/1.1 subset, a JSON value that keeps key order (the user's
`settings.json` is rewritten in place), ISO 8601 dates, the system's random numbers, process tables,
ConPTY and `openpty` through the system's own functions.

## Where the data comes from

Checked on Claude Code 2.1.304 and later 2.1 releases. Internal files (transcripts, `~/.claude/sessions/`)
may change between versions: a documented source is preferred whenever one exists, and every read of
Claude Code's own files lives in `src/claude.rs`.

| Need | Source | Status |
|---|---|---|
| Sessions and their state | `claude agents --json`: `sessionId`, `name`, `cwd`, `pid`, `status` (busy, idle, waiting), `waitingFor`; every 2 s while a page is open, every 10 s otherwise | documented |
| Approve a permission | `PermissionRequest` hook, answer `decision.behavior` allow or deny | tested |
| Answer a question, approve a plan | `PermissionRequest` on `AskUserQuestion`, `decision.updatedInput.answers` | tested |
| Change the mode | `updatedPermissions` with `setMode`, destination `session`, sent with an answer | tested |
| Auto mode denial | `PermissionDenied` hook, answer `retry: true` | documented, real payload not seen yet |
| Stop on error | `StopFailure` hook (`rate_limit`, `authentication_failed`...) | documented |
| End of a turn, read | `claude agents`: busy or waiting then idle is done, idle then busy is read; the last message from the transcript | observed |
| Thread, title, model | the transcript: prompts, text, `tool_use`, `tool_result.is_error`, `ai-title` records, `message.model` | observed |
| Who sent a message | a user entry's `origin.kind` (human, plugin, task notification, continuation) | observed |
| How a permission was settled | `permissionDecision` on tool results (`source: "user_temporary"` is a "yes, this once") | observed |
| Plan and current step | the last `TodoWrite` call of the transcript | observed |
| Tests, commits, merge request of a turn | the turn's `Bash` calls and their results | heuristic |
| Artifacts published | the transcript's `Artifact` calls and results | observed |
| Subagents | `SubagentStart` / `SubagentStop`; `<session>/subagents/agent-<id>.jsonl` and `.meta.json` | observed |
| Workflows | `<session>/subagents/workflows/<run>/journal.jsonl`; `toolUseResult.runId` links the call to its run; phases from the script's `meta.phases`; `<session>/workflows/<run>.json` once it ends | observed |
| Context, quota, cost | companion mod: `$.session.usage()` | tested |
| Message to a session | companion mod: `$.prompt.submit`; the first one of a session clearance starts: `claude -- <text>` | tested |
| Slash command, model for the next turn | companion mod, run as a command (`/rename`, `/model`, `/compact`) | tested |
| Interrupt, stop | companion mod: `$.turn.abort`, `$.command.run('exit')` | tested |
| Memory of a session | its `pid` and children in the system's process table (Toolhelp32, `ps`), every 10 s | tested |
| A session going on under a new id | the same `pid` with another `sessionId` (`/clear`, `/resume`): same terminal, not always the same work | observed |
| The session a handoff continues | the first other session id in the message a plugin sent in the user's place | observed, one plugin |
| Claude Code version | `claude --version`, at start and every 10 minutes | tested |

A session's files live in `~/.claude/projects/<cwd with every non-alphanumeric character as "-">/`. A
transcript is parsed whole, on demand, and kept until its size changes.

## Hooks: who answers, who wins

Claude Code keeps its own dialog open while a hook holds a request. So clearance can hold every request
without blocking anyone: the first answer wins, in the terminal or on the page. An answer given in the
terminal applies at once and a later one from clearance is ignored; the hook's connection is not reliably
closed, so a request leaves the page on the first of: its connection closes, the session stops reporting
`waitingFor`, or a later event of that session arrives.

| Hook | Timeout | Why |
|---|---|---|
| `PermissionRequest` | a day | permissions, questions and plans; the terminal keeps its dialog meanwhile |
| `PermissionDenied` | 60 s | held only while a page is open, the time to decide on a retry |
| `StopFailure`, `SubagentStart`, `SubagentStop` | 5 s | answered at once |

Only hooks that stay silent while clearance is stopped are installed: `Stop` and `UserPromptSubmit` print
an error in every session when nobody answers, so the end of a turn is read off `claude agents` instead.
Questions go through `PermissionRequest`, never a held `PreToolUse`, which would hide the terminal dialog
for as long as it waits. A free answer to a question goes back as a deny carrying the user's words, since
`AskUserQuestion` only accepts option labels.

Plan approvals become visible through one convention in the user's `CLAUDE.md`: present the plan with
`AskUserQuestion` (options GO / No), its commands, risks and rollback in the option descriptions.

`CLEARANCE_LOG=1` keeps every hook payload in `~/.clearance/hooks.jsonl`, to record the first real
`PermissionDenied`.

## The companion mod

Every 5 s the mod sends one request, `POST /mod/sync`, with the hook secret: the session's usage goes in;
the number of requests waiting and the messages queued for this session come back. It finds clearance in
`~/.clearance/endpoint.json`, written at start: nothing to configure. With clearance stopped it shows
nothing and does nothing.

Messages for a session queue up and are handed out one by one; the page shows a message as delivered
only once the mod took it. A slash command is run as a command, the way the person would type it. The
mod follows its session through `/clear` and `/resume`. Sessions without the mod, or with one older than
a feature, are told apart on the page with what to do.

## Sessions in the page

A new task or a resumed session runs in a pseudo-terminal of clearance's own (ConPTY on Windows,
`openpty` on macOS). clearance starts `claude` itself, never a shell, with the session's name, model and
mode, and the hooks and mod when the user's settings lack them. The environment drops the markers of the
session that started clearance, otherwise the new one would run as its child, with no transcript.

- The instruction of a new task is `claude`'s first prompt, after `--`: 8000 characters at most. When
  `claude` is a `.cmd` (an npm install), cmd.exe reads `" % ! ^ & | < >` and line breaks whatever the
  quoting, so such an instruction is refused with the reason.
- The last 512 KB of output are kept per terminal and streamed to the page; keys and size go back by POST.
- Images and documents pasted or dropped are kept in clearance's folder under the system's temporary
  folder (outside `~/.clearance`, which sessions are denied), and their path goes in the text.
- A session belongs to clearance's process: stopping clearance ends it. `~/.clearance/terminals.json`
  lists them, and the next start reopens each one with `claude --resume`. A restart from the page waits
  until they are idle (20 minutes at most, can be forced or cancelled).
- Folders are browsed in the page (the server lists subfolder names), never through a system dialog.

A session started elsewhere stays in its own terminal, shown and answered like the others.

## Groups and the life of a session

Groups are made by the user; a session never placed is in "No group". What waits for you and what needs
watching stay on top across groups, sorted by risk, so a production `GO` is never hidden in a folded
group. Nothing leaves on its own: a session that disappears from `claude agents` is kept as stopped, in
its group, with what it takes to resume it, until the user deletes it (its transcript is never touched).
One that goes on under a new id keeps its group and name. A session without a name is called after its
folder. Asleep: idle, nothing left to read, transcript untouched for 3 hours; "Close the asleep sessions"
quits them through their mod after saying how much memory it frees. Everything lives in
`~/.clearance/fleet.json`.

## Risk

`src/risk.rs` sorts a request into read, write or infrastructure write. It is a signal, not a guarantee:
the card always shows the full command, and an infrastructure write always asks for `GO` typed on the
page, checked by the server, and never adds a rule.

- The program word is normalised first (quotes, folder, `.exe`, `.cmd`, `.bat`, `.ps1`, case).
- An MCP tool whose name holds a writing verb is an infrastructure write: its server can reach a remote
  service.
- The environment comes from patterns the user declares in `~/.clearance/environments.json`, for example
  `{ "prod": ["-prd", "--context prod"], "staging": ["-stg"] }`; the card says which one matched. Nothing
  matched means unknown, handled like production. The repository ships no pattern.
- Tricky commands (PowerShell, POSIX, wrappers, MCP) are kept as tests.

Rules are suggested from approvals given by hand in the transcripts, three at least per rule. A rule is
a prefix and allows more than what was approved, so it is offered only for a single command (no `&&`,
`||`, `;`), for programs whose second word is a subcommand or that only read, never for open-ended
subcommands without a third word, subcommands that discard work, MCP tools that write, or infrastructure
writes. clearance shows the rule to copy; it never writes settings.

## Security

An "Allow" button is a right to run code on the machine, so its credential must not be readable by the
agents it supervises.

- **Local only**: `127.0.0.1`, `Host` checked, `Origin` checked on every request that writes.
- **Two secrets**. The hook secret (`~/.clearance/token`) opens `/hook/*`, `/mod/sync` and `/open` only;
  every connected session can read it, and it lets them report events, nothing more. A browser signs in
  through a single-use link (5 minutes, kept in memory only) and gets a session of its own, an
  `HttpOnly; SameSite=Strict` cookie stored as a SHA-256 in `browsers.json`, ended after 30 days unused.
  `clearance open` asks the running server to open the browser on a fresh link, never handed back to the
  caller.
- **Deny rules**: the settings clearance writes deny Claude Code's file tools on clearance's folder, so a
  session cannot slip a session of its own into `browsers.json`.
- **Content security policy**: only the page's own script and styles, through a nonce drawn at each load;
  no style attribute; fonts served by clearance with their OFL licences; nothing from another site. Text
  from sessions goes into the page as text, never as markup.
- **Claude Code versions**: outside the validated range the page and the server allow nothing, only deny,
  and say to decide in the terminal. Moving the range is a commit, after the end to end tests and a real
  permission.

Accepted limits: the deny rules cover Claude Code's file tools and the shell commands it recognises, not a
program that opens files itself once approved. A session holding the hook secret can post fake events or
make clearance open a browser tab; none of that answers a request. The hook secret is not rotated, since
that would disconnect every running session.

## Knowing when it does not know

- The page says its data is stale, and since when, once `claude agents` has failed for 15 s.
- A heartbeat every 15 s drops dead page streams, so the poll slows down once no page is open.
- The journal of decisions keeps about 10 MB in two files.
- End to end tests run a fake Claude Code over real sockets: a permission held then allowed, answered in
  the terminal first, dropped by a restart, seen by two pages, refused for an unvalidated version.
- CI on Windows and macOS: fmt, clippy, tests, release build.

## The look

Manga style, chibi avatars drawn for the project, where there is room for them (the session's header, the
folded queue, the search). The compact queue has none: there the state is said in words, a gauge and one
small animation per state, nothing moving at rest. Every trait carries information:

| Trait | Meaning |
|---|---|
| Hair colour | the group (the folder for a session in no group) |
| Glasses / headphones / cowlick | Opus / Sonnet / Haiku |
| Speed lines | working |
| Sweat drop | waiting for a permission |
| "!" | waiting for a GO |
| "?" | asking a question |
| Anger vein | blocked by auto mode |
| Sparkles | done |
| Zzz | asleep |
| Spiral eyes | context almost full |
| Smaller clones | subagents |

- An avatar pack (manga, robots, Halloween) changes the costume, a skin changes the look; neither changes
  the meaning.
- Twelve skins, styled only through CSS variables; tests check that no colour is written elsewhere and
  that every text and background pair reaches a contrast of 4.5:1 in every skin.
- Body text in Atkinson Hyperlegible Next, commands in Atkinson Hyperlegible Mono.
- The logo, a radar echo and the check mark of a cleared flight, is the same in every skin.
- Every text comes from a dictionary, English and French; the server answers with codes the page
  translates.
- Original drawings or CC0 only, fonts under the SIL Open Font License, no decorative tilt;
  `prefers-reduced-motion` turns every animation off.

## Open questions

- The shape of a real `PermissionDenied`: never triggered yet, the documented one is handled.
- Channels (`claude/channel/permission`) as a relay beside the terminal: a research preview behind an
  organisation policy, and less needed since hooks and the dialog run in parallel.

## Out of scope

Other agent CLIs, cloud or desktop app sessions, access from another machine, anything that needs
administrator rights.
