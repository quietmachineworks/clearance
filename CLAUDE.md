# clearance - notes for Claude Code

`README.md` says what clearance does; `docs/how-it-works.md` what it reads, sends and refuses, and why
(read it before any change); `CHANGELOG.md` what changed. A change that makes one of them wrong updates it
in the same commit. A detail too long for them goes in another page under `docs/`, linked from the README.

## Where we are

- Built: the board, the session detail, the companion mod, sessions in a terminal of the page, groups and
  memory, two secrets, a strict CSP, trust signals, the compact queue, the thread in turns, the right
  panel, attachments as chips. The Rust binary serves the live page; the Node version is in git history
  up to 0242ec5.
- Not validated yet: a full day of work without a terminal application.
- Open: the shape of a real `PermissionDenied` (`CLEARANCE_LOG=1` records it); channels.

## Working rules

- Rust stable, edition 2024, **standard library only**: a crate needs a reason, written in
  `docs/how-it-works.md`. `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt
  --check` pass before a commit. Code that only builds on macOS is checked with `cargo check --target
  aarch64-apple-darwin`.
- The server listens on `127.0.0.1` only; `Host` checked, `Origin` checked on every write.
- Claude Code hooks of type `http`, never `command`.
- Hook tests: an isolated project with its own `.claude/settings.json`. Never touch the user's global
  settings without their explicit consent. A test server runs with `CLEARANCE_HOME` and `CLEARANCE_PORT`
  set, so that it never takes over the live one.
- Reads of Claude Code internal files (transcripts, `subagents/`, `sessions/`) live in `src/claude.rs`:
  their format may change.
- The page: CSS variables only, every text from the dictionary, no colour written in place.
- Public repository: no real project names, hostnames, paths, session data or anything from the
  maintainer's work environment, in code, fixtures, docs or commit messages. Fixtures use invented names.
- Avatars: original drawings or CC0 only, OFL fonts. A pack or a skin changes the costume, never the
  meaning.
- ASCII hyphen `-` everywhere, never an em dash or en dash.
- Commits in English, imperative, short. No AI attribution in commits or pull requests.
- `CHANGELOG.md`, `Unreleased` section, on every visible change.

## Repository map

| Path | Role |
|---|---|
| `Cargo.toml`, `src/main.rs` | the binary: `clearance` serves, `clearance install` sets the hooks |
| `src/http.rs`, `src/json.rs`, `src/sys.rs` | what a crate would give: HTTP subset, JSON, dates, randomness, processes |
| `src/auth.rs` | login links and browser sessions |
| `src/board.rs` | board state and hook answers, pure |
| `src/fleet.rs` | groups, stopped sessions, names, pure |
| `src/risk.rs` | read / write / infrastructure write classification, environments, suggested rules |
| `src/claude.rs` | the only reads of Claude Code itself: `claude agents`, transcripts, subagents, workflows |
| `src/terminal.rs`, `src/pty.rs`, `src/terms.rs` | starting `claude`, pseudo-terminals, what each one draws |
| `src/install.rs` | adds or removes clearance's hooks in the user's settings |
| `public/index.html`, `src/page.rs` | the page, avatars included; its checks: colours, contrast, policy, pure helpers |
| `public/vendor/xterm/` | xterm.js, frozen with its licence and hashes |
| `mod/`, `.claude-plugin/marketplace.json` | the companion mod, and the repository as its marketplace |
