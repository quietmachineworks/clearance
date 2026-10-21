# clearance - notes for Claude Code

Product source of truth: `docs/BRIEF.md` (read it in full before any change). What is not in it is out
of scope. Decisions taken outside the brief: `docs/adr/`.

## Where we are

- **2026-10-20**: framing and UI trials done, direction chosen (manga style, chibi avatars, generic
  session detail). No application code yet.
- **M0 done** (ADR-0003, ADR-0004): the terminal dialog stays open while a hook holds a request, the
  terminal answer wins, `setMode` and `AskUserQuestion` answers work; a companion mod carries usage
  figures and instructions; terminal tabs are found by title. Question 3 (`PermissionDenied`) is checked
  on real events in M1; 5 (channels) is no longer needed.
- **M1 done** (ADR-0005): a real permission and a real question answered from the page; questions go
  through `PermissionRequest`, a held `PreToolUse` would hide the terminal dialog. Question 3 still
  waits for a real denial (`CLEARANCE_LOG=1` records it).
- **M2 done** (ADR-0006): the session detail, read from transcripts, subagent and workflow files in
  `src/claude.ts`; a live two-phase workflow followed end to end.
- **Next**: M3, the companion mod (energy, quota, instructions), asleep sessions, journal and suggested
  rules. `spike/` holds the M0 receiver and probe mod.

## Working rules

- Node 24, TypeScript run directly (`node file.ts`), **zero dependencies**. A dependency needs an ADR.
- The server listens on `127.0.0.1` only; token required and `Origin` checked on every write.
- Claude Code hooks of type `http`, never `command`.
- Hook tests: an isolated project with its own `.claude/settings.json`. Never touch the user's global
  settings without their explicit consent.
- Reads of Claude Code internal files (transcripts, `subagents/`, `sessions/`) live in one module: their
  format may change.
- Public repository: no real project names, hostnames, paths, session data or anything from the
  maintainer's work environment, in code, fixtures, docs or commit messages. Fixtures use invented names.
- Avatars: original drawings or CC0 only, OFL fonts. A pack changes the costume, never the meaning layer
  (section 4 of the brief).
- ASCII hyphen `-` everywhere, never an em dash or en dash.
- Commits in English, imperative, short. No AI attribution in commits or pull requests.
- `CHANGELOG.md`, `Unreleased` section, on every visible change.

## Repository map

| Path | Role |
|---|---|
| `docs/BRIEF.md` | product, scope, data sources, milestones |
| `docs/adr/` | decisions |
| `src/server.ts` | the one process: hooks, page, SSE, token and Origin checks |
| `src/board.ts` | board state and hook answers, pure |
| `src/risk.ts` | read / write / infrastructure write classification |
| `src/claude.ts` | the only reads of Claude Code itself: `claude agents`, transcripts, subagents, workflows |
| `public/index.html` | the page, avatars included |
| `test/` | `node --test` |
| `design/` | mockups (canvas sources `.dc.html`) |
| `spike/` | M0 hook receiver, not part of the product |
