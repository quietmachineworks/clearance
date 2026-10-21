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
