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
  settled (ADR-0012).
- The V2 page (BRIEF section 3b): a queue that holds still under the pointer, groups
  with their counts, the session with its thread or its terminal view, an inspector (task, plan, files,
  tests, agents, cost, model), every answer sent after 5 s with an undo, Ctrl K, keyboard shortcuts;
  twelve skins, English and French, logo A of the design canvas, the same in every skin (ADR-0011).
