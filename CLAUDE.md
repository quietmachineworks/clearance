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
- **M3 done** (ADR-0007): the companion mod in `mod/` (usage, quota, instructions, status line), asleep
  sessions, the journal and rules suggested from approvals by hand in the transcripts.
- **M4 done** (ADR-0008): sessions started in a real Windows Terminal tab from the page (folder
  picker, session markers removed from the tab's environment), go to a session's tab, Halloween pack.
- **Install for every session** ready (ADR-0009): `src/install.ts`, the repository as a marketplace,
  only hooks that stay silent while clearance is stopped.
- **2026-10-22**: V2-A mockup chosen (BRIEF section 3b), skins, languages and a fixed logo (ADR-0011),
  environments of infrastructure writes (ADR-0012). **R1 done**: the server in Rust (ADR-0010). **R2**: the
  V2 page with logo A. The Rust binary serves the live page since 2026-10-22; the Node version is
  removed (in git history up to 0242ec5).

## Working rules

- Rust stable, edition 2024, **standard library only**: a crate needs an ADR. `cargo test`, `cargo
  clippy --all-targets -- -D warnings` and `cargo fmt --check` pass before a commit. Code that only
  builds on macOS is checked with `cargo check --target aarch64-apple-darwin`.
- The server listens on `127.0.0.1` only; token required, `Host` checked, `Origin` checked on every write.
- Claude Code hooks of type `http`, never `command`.
- Hook tests: an isolated project with its own `.claude/settings.json`. Never touch the user's global
  settings without their explicit consent. A test server runs with `CLEARANCE_HOME` and `CLEARANCE_PORT`
  set, so that it never takes over the live one.
- Reads of Claude Code internal files (transcripts, `subagents/`, `sessions/`) live in one module: their
  format may change.
- The page: CSS variables only (ADR-0011), every text from the dictionary, no colour written in place.
- Public repository: no real project names, hostnames, paths, session data or anything from the
  maintainer's work environment, in code, fixtures, docs or commit messages. Fixtures use invented names.
- Avatars: original drawings or CC0 only, OFL fonts. A pack or a skin changes the costume, never the
  meaning layer (section 4 of the brief).
- ASCII hyphen `-` everywhere, never an em dash or en dash.
- Commits in English, imperative, short. No AI attribution in commits or pull requests.
- `CHANGELOG.md`, `Unreleased` section, on every visible change.

## Repository map

| Path | Role |
|---|---|
| `docs/BRIEF.md` | product, scope, data sources, milestones |
| `docs/adr/` | decisions |
| `Cargo.toml`, `src/main.rs` | the Rust binary: `clearance` serves, `clearance install` sets the hooks |
| `src/http.rs`, `src/json.rs`, `src/sys.rs` | what a crate would give: HTTP subset, JSON, dates, randomness, processes |
| `src/board.rs` | board state and hook answers, pure |
| `src/risk.rs` | read / write / infrastructure write classification, suggested rules |
| `src/claude.rs` | the only reads of Claude Code itself: `claude agents`, transcripts, subagents, workflows |
| `src/terminal.rs` | terminal tabs: Windows Terminal, macOS Terminal |
| `src/install.rs` | adds or removes clearance's hooks in the user's settings |
| `.claude-plugin/marketplace.json` | the repository as a plugin marketplace for the mod |
| `public/index.html`, `src/page.rs` | the page, avatars included; its checks: colours, contrast, both languages |
| `mod/` | the companion mod |
| `design/` | mockups (canvas sources `.dc.html`) |
| `spike/` | M0 hook receiver, not part of the product |
