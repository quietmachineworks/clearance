# Changelog

## Unreleased

- Product brief, first decisions (local page, http hooks, manga visual grammar) and UI mockups.
- M0 spike: hook receiver and results on blocking http hooks (ADR-0003); tower mode dropped.
- M0 spike: probe mod for usage figures and instructions, terminal tabs by title (ADR-0004).
- M1 board: permissions, questions, plan approvals and auto mode denials answered from a local page,
  sorted by risk; done and working sessions; browser notifications; manga and robots avatar packs.
- Questions answered through `PermissionRequest`, never a held `PreToolUse` (ADR-0005).
- M2 session detail: workflow phases and agents side by side, subagents, errors and loops, changes made
  by the session and its agents, what ran, workflow history (ADR-0006).
- Avatars wear their model's accessory; a workflow permission card shows the whole script.
- M3 companion mod: context, cost and quota on the page, energy bars, instructions sent to a session,
  a line under the prompt; asleep sessions; a journal of decisions; permission rules suggested from
  approvals given by hand (ADR-0007).
- M4: start a session in a new Windows Terminal tab from the page, with a folder picker; bring a
  session's tab to the front; Halloween avatar pack (ADR-0008).
- Install for every session: `node src/install.ts` for the hooks, the repository as a plugin
  marketplace for the mod. Only hooks that stay silent while clearance is stopped; turns read off
  `claude agents`; auto mode denials held only while a page is open; sessions not connected to
  clearance shown as such (ADR-0009).
- Done cards show the session's last message in full and take a reply, typed into the session by the
  companion mod.
- Text in Atkinson Hyperlegible Next, commands in Atkinson Hyperlegible Mono, both drawn for low vision
  readers, in place of M PLUS Rounded 1c; body text at 16px.
- The board fills the whole screen: waiting, done, then a rail for working, ignored and asleep sessions;
  Waiting narrows when no request card is shown, done cards sit side by side and render light Markdown
  (headings, bold, code, bullets) as text nodes.
- Ignore a session from its done card: it leaves Done, Working and Asleep for an Ignored row, in this
  browser; its requests still show in Waiting.
- Start a session sits behind a button at the top of the page; Fable is one of the models offered.
- A permission, question or plan stays on the page up to a day instead of 10 minutes (ADR-0009).
- The server rewritten in Rust, standard library only (ADR-0010): one binary for Windows and macOS,
  `clearance install` for the hooks; its state moves from `data/` to `~/.clearance`. The Node version
  is removed.
- The environment of an infrastructure write (PROD, STAGING, LOCAL or unknown, handled like PROD) from
  patterns in `~/.clearance/environments.json`; a request answered in the terminal stays a while as
  settled, one line per session (ADR-0012).
- The V2 page (BRIEF section 3b): a queue that holds still under the pointer, groups
  with their counts, the session with its thread or its terminal view, an inspector (task, plan, files,
  tests, agents, cost, model), every answer sent after 5 s with an undo, Ctrl K, keyboard shortcuts;
  twelve skins, English and French, logo A of the design canvas, the same in every skin (ADR-0011).
- A session opens on an Action tab that shows only what it needs from you; its thread, terminal view
  and a Settings tab come after. Settings: rename (`/rename`), model, compact the context, group of its
  folder, notifications, ignore, the command to resume it elsewhere. The companion mod runs a slash
  command typed for a session as a command; a renamed session's tab is still found by its old title.
- The terminal view follows the end of the session, and stays where the reader scrolled up to.
- The thread shows one line per tool call, paths shortened to the session's folder or their last two
  parts.
- The page works on a tablet (details in a tab) and a phone (the queue or one session, larger touch
  targets).
- Stop sessions from the page through their companion mod: the running turn ends with its tools, and on
  request the session quits (`/exit`), its transcript kept for `claude --resume`. A button at the top
  stops every running agent at the end of the day; the Interrupt and Stop buttons of a session and of a
  group work. A stop not picked up within a minute is dropped. Sessions without the mod are listed with
  a way to their tab.
- A session waiting in its terminal says why: not connected to clearance (with the command to resume it
  connected), or its request asked while clearance was restarting. Connected sessions are remembered
  across restarts, and clearance's tab says when stopping it would drop held requests.
- An Idle section lists the sessions done with their turn and already read, newest first: they no longer
  vanish from the queue until they fall asleep.
- The Asleep and Ignored drawers stay open across redraws; a session in them opens on a click, and an
  ignored one has a labelled Stop ignoring button.
- The start form picks its folder in the system's folder dialog in place of a list of buttons.
- Sessions quit from the stop dialog are kept in the browser; a button at the top lists them and
  resumes one or all in new terminal tabs (`claude --resume`).
- The companion mod follows its session through `/resume` and `/clear`, which change its id; a message
  sent from the page reads delivered only once the mod took it, and says so while it waits.
- A session whose subagents or workflow wrote in the last two minutes is at work, with how many of them,
  even while `claude agents` says its main loop is idle: no early To read card, no false stall.
- The companion mod says its version; the page offers to stop a session only when its mod can, and
  otherwise says why (no mod, or a mod loaded before stopping existed: `/reload-plugins`).
- A Connection block in a session's Settings: hooks received or not, companion mod up to date, old or
  absent, with what to do and a way to its tab.
- The inspector lists the links a session pointed to, newest first: artifacts, merge and pull requests
  (Claude Code's own `pr-link` records), local servers, pages named in its replies.
- Rename, model and compact are offered only when the session's companion mod can run a slash command;
  otherwise they are greyed out with the reason and what to type in its terminal.
- A field being typed in a session's view (rename, group, GO) keeps its focus while the session works;
  the view catches up once it is left.
- Two secrets (ADR-0013): the token of the hooks no longer opens the page nor answers a request, so a
  session that reads it cannot approve its own permission. Browsers sign in through a single-use link
  (5 minutes) and keep a session of their own, stored as a SHA-256 only; the Settings list the signed-in
  browsers and sign them out. `clearance open` opens the page signed in; a second `clearance` does the
  same. The settings clearance writes deny the file tools on its folder. `/?token=` no longer signs in.
- A strict content security policy (ADR-0014): only the page's own script and styles, through a nonce
  drawn at each load; the twelve fonts are served by clearance with their OFL licences, nothing comes
  from Google any more.
- Groups made by the user (ADR-0015): create, rename, delete, put a session in one (from its header, its
  Settings or the start form); the others are in "No group". The queue keeps what waits for you and what
  needs watching on top, across groups; below, one foldable block per group with its counts and memory.
  The former folder groups are set aside (`groups.json.old`).
- A session that stops, from clearance or not, stays in its group as stopped, with Resume (a new tab,
  `claude --resume`) and Delete (forgotten in clearance, its transcript kept). One that goes on under a
  new id (`/clear`, `/resume`) keeps its group and name. Replaces the stopped list kept in the browser.
- Memory: each session's processes, per group and in total; "Close the asleep sessions" says how much
  memory quitting them frees.
- A session without a name is called after its folder, numbered when the folder has several.
- The browser tab's title counts what waits for you: "(3) clearance".
- Sessions run inside the page (ADR-0016): a new task or a resumed session starts the real Claude Code in a
  pseudo-terminal of clearance's own (ConPTY on Windows, `openpty` on macOS), drawn by xterm.js 5.5.0 in
  its Terminal tab, shown at once (a folder to trust, for one). Text from the message field is typed in it.
  clearance drives no terminal application, PowerShell or AppleScript any more: no "go to its tab", folders
  are browsed in the page. xterm.js is frozen in `public/vendor/xterm` with its licence and hashes.
  The instruction of a new task is the session's first prompt (`claude -- <text>`): no companion mod
  needed for it.
- Images and documents pasted or dropped on the message field or on a terminal are kept by clearance where
  sessions can read them, and their path goes in the text.
- clearance runs without a window by default (`--foreground` keeps it in the terminal). The Settings
  restart it once its sessions are idle (they reopen with `claude --resume`) or stop it; its terminals
  are reopened at its next start. The page follows a restart, a terminal it shows included.
- The thread folds two tool calls or more in a row into one line ("5 actions · Bash, Read", failures
  counted, opened when one failed, kept open or closed across refreshes); "My messages" shows only what
  you wrote; each reply copies as Markdown.
- Shortcuts: one global list of prompts (`~/.clearance/prompts.json`), edited in the Settings, sent to
  the session on screen from chips above its message field or with Ctrl+Shift+1 to 0, from a terminal
  too. Messages for a session queue up and reach it one by one: none overwrites another.
- Ctrl K finds with case and accents ignored, every word in any order.
- The Settings show clearance's version, the commit it was built from (`build.rs`) and when it started.
- What clearance knows, and says when it does not (ADR-0017): the page says its data is stale, and since
  when, once `claude agents` stops answering; a heartbeat drops dead page streams, so the poll slows
  down once no page is open.
- Claude Code's version is read every 10 minutes; outside the validated range (2.1.304 and later 2.1
  releases) the page and the server allow nothing, only deny, and say to decide in the terminal.
- Risk: program names are normalised (quotes, folder, `.exe`, case), `git -C x push`, PowerShell
  remoting and `Invoke-Sqlcmd` are infrastructure writes, and so is an MCP tool whose name holds a writing
  verb; tricky commands are kept as tests.
- End to end tests with a fake Claude Code: a permission held then allowed, answered in the terminal
  first, dropped by a restart, seen by two pages, refused for an unvalidated version.
- The journal keeps about 10 MB, in two files; the Journal page says so.
- CI on Windows and macOS (fmt, clippy, tests, release build); a `v*` tag publishes the binaries with the
  tag's CHANGELOG section as notes.
- A compact queue for ten sessions a group (BRIEF 3b, mockup `design/canvas/QueueCompact.dc.html`): one
  32 px line per session, no avatar, its state on the right (a 5-cell context gauge with dots that wave
  while it works, a check that pops once its turn ends, a cross when it failed, how long it has been
  idle); no state subheadings; slim group headers whose actions show on hover or focus; asleep, stopped
  and ignored as counters that open in place. Waiting cards show the request's icon, flash once when they
  arrive; what needs watching breathes. The queue's width is dragged between 300 and 640 px (arrow keys,
  double-click for 420), kept per browser.
- The thread in turns (BRIEF 3b, mockup `design/canvas/ThreadTurns.dc.html`): each message you sent, a
  summary line with result chips (files, tests, commits, MR, failures), the reply in full; what it did
  as sentences, with how each permission was settled (by you, from clearance, refused, blocked by auto
  mode) and the artifacts it published; folded but the running turn and a failed one; "All | My
  messages"; a resume after compaction is a line, not a message, and what a plugin sent in your place is
  not shown as yours.
- The right panel hides down to a column of icons, or is dragged between 240 and 520 px; its task is the
  session's title and first request on three lines; it lists the artifacts the session published; nothing
  overflows sideways.
- Attachments wait as chips above the message field (thumbnail, size, remove), from a paste, a drop
  anywhere on the session or the paperclip, and go with the message. A message with an image now reaches
  a session in clearance's terminal: Enter is sent apart from the paste.
- The right panel always shows its Artifacts section, with a link to all your artifacts on claude.ai.
  A session a handoff plugin started also lists, dashed, the artifacts of the session it continues
  (mockup `design/canvas/ThreadArtifactsResumed.dc.html`); one it publishes again keeps its name.
- Durations over an hour read "5 h 01", not "5 h 1".
- The thread names an MCP tool as people say it ("playwright · browser click"), and the session's own
  sentences show their `code` as code.
- Files a session wrote in its scratchpad are no longer listed as changes.
- Ctrl+Enter in a GO field allows the infrastructure write or launches the plan, once GO is typed.
- An open drawer of the queue stays within it: Resume and Delete no longer slip out of view.
- The page's script is compiled by the tests when node is installed.
