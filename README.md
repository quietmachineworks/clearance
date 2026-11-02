# clearance

A local control tower for the Claude Code sessions running in parallel on your machine.

With five or ten sessions open, the bottleneck is no longer the agent, it is you: a session sits blocked
on a permission and nobody notices, approving means finding the right terminal first, a finished session
waits unread for an hour, an auto mode denial goes unseen. clearance puts all of them on one page and
lets you answer from there.

- **What waits for you first**: permissions, questions, plan approvals and auto mode denials, sorted by
  risk then age, each with the full command. An infrastructure write shows its environment (production,
  staging, local, or unknown, handled like production) and asks you to type `GO`.
- **Answer from the page or the terminal**: Claude Code keeps its own dialog open, the first answer wins.
  Every answer can be taken back for 5 seconds.
- **Every session at a glance**: to read, working, idle, asleep, stopped; in groups you make, with each
  one's context, cost, memory and your quota.
- **One session in full**: its thread told turn by turn (what you asked, what it did, what it answered,
  how each permission was settled), its plan, the files it changed, tests, commits, subagents and
  workflows, the artifacts it published.
- **Sessions that live in the page**: start or resume one and it runs in a real terminal drawn on the
  page. Write to any session, paste images and files, send a saved prompt with a shortcut, interrupt or
  stop it, rename it, change its model.
- **Fewer interruptions**: permission rules suggested from what you approved by hand. clearance shows
  them; it never writes your settings.

Claude Code only, local only, one binary with no dependencies, Windows and macOS. Twelve skins, English
and French.

## Try it

Rust stable, 1.85 or later.

```sh
cargo build --release
target/release/clearance
```

clearance runs without a window (`clearance --foreground` keeps it in your terminal) and opens the page
in your browser, signed in. `clearance open` opens it again, or signs another browser in.

Connect a session by passing it clearance's hooks, and the companion mod for usage figures, messages and
stop:

```sh
claude --settings ~/.clearance/hooks.json --plugin-dir /absolute/path/to/clearance/mod
```

Its requests now show on the page as well as in its terminal. With clearance stopped, the session behaves
as usual. Sessions started from the page are connected already.

## Install for every session

A step to take once you trust the tool. clearance only installs hooks that stay silent when it is not
running.

```sh
clearance install --dry-run   # what would change in ~/.claude/settings.json
clearance install             # backup, then add clearance's hooks next to yours
```

Then, in Claude Code, `/plugin install clearance --marketplace quietmachineworks/clearance` installs the
companion mod (from a clone: `claude plugin marketplace add <this folder>`, then
`claude plugin install clearance@clearance`). Sessions started afterwards are connected.
`clearance install --remove` and `claude plugin uninstall clearance@clearance` undo it.

### Plan approvals

A plan approved in the chat leaves the session idle, which looks like a session that is done. To see it
on the page, ask Claude in your `CLAUDE.md` to present plans with `AskUserQuestion` (options GO / No),
with the commands, risks and rollback in the option descriptions.

### Environments

Tell clearance how your production looks in `~/.clearance/environments.json`:

```json
{ "prod": ["-prd", "db-prod", "--context prod"], "staging": ["-stg", "-n staging"] }
```

A remote write that matches nothing is treated as production.

## Safety

An "Allow" button is a right to run code on your machine, so:

- clearance listens on `127.0.0.1` only and checks `Host` and `Origin`;
- the secret your sessions can read only lets them report events: the page needs a browser session,
  obtained through a single-use link, so a session cannot approve its own request;
- the page runs only its own script, under a strict content security policy;
- on a Claude Code version it was not checked against, clearance allows nothing and says to decide in
  the terminal.

Details, and the limits accepted: [docs/how-it-works.md](docs/how-it-works.md).

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The page is `public/index.html`, read from the repository at each load: edit it and reload. Run a test
instance next to your own with `CLEARANCE_HOME` and `CLEARANCE_PORT` set. Changes go in the `Unreleased`
section of [CHANGELOG.md](CHANGELOG.md).

To release, rename `Unreleased` to the version (`## 0.5.0`), set it in `Cargo.toml`, commit, then tag
`v0.5.0` and push the tag: CI builds the Windows and macOS binaries and publishes them with that section
as notes.

## License

MIT
