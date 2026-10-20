# clearance - notes for Claude Code

Product source of truth: `docs/BRIEF.md` (read it in full before any change). What is not in it is out
of scope. Decisions taken outside the brief: `docs/adr/`.

## Where we are

- **2026-10-20**: framing and UI trials done, direction chosen (manga style, chibi avatars, generic
  session detail). No application code yet.
- **Next**: M0, the technical spikes of section 8 of the brief, in `spike/` and an isolated test
  project. Do not start M1 before each M0 question has its ADR.

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
| `design/` | mockups (canvas sources `.dc.html`) |
