# clearance

A local control tower for Claude Code sessions running in parallel.

One page on your machine shows who is waiting for you (permission, plan approval, question, auto mode
denial), who needs watching, who is done, who is working and who is asleep, and lets you answer without
hunting for the right terminal. Pick a session to see its thread, its plan, the files it changed, its
tests, agents and cost. Twelve looks, English and French.

Claude Code only, local only, one binary with no dependencies.

Status: V1 milestones done (M0 to M4), the server rewritten in Rust (R1), the V2 page (R2). See
`docs/BRIEF.md` for the whole plan.

## Try it

Rust stable, 1.85 or later.

```sh
cargo build --release
target/release/clearance
```

clearance runs without a window (`clearance --foreground` keeps it in your terminal). The first time, it
opens the page in your browser, already signed in; the browser stays signed in across
restarts. `clearance open` opens the page again, or signs another browser in: the sign-in link works
once, for 5 minutes, and never shows the hook secret. It writes `~/.clearance/hooks.json`. Connect a
session by passing those hooks to it:

```sh
claude --settings ~/.clearance/hooks.json
```

Requests from that session now show on the page as well as in its terminal; the first answer wins. With
clearance stopped, the session behaves as usual.

Add the companion mod to see the session's context, cost and your quota, and to send it instructions
from the page:

```sh
claude --settings ~/.clearance/hooks.json --plugin-dir /absolute/path/to/clearance/mod
```

Sessions started from the page run inside it: each one is the real Claude Code, in a terminal drawn in its
Terminal tab (no terminal application needed). Paste or drop an image or a document on the message field
or on the terminal: the session gets its path. Arrange sessions in your own groups; a stopped session stays
in its group until you delete it, and resumes in one click. The Settings restart clearance (it waits for
its sessions to be idle, then reopens them) or stop it.

The Journal page suggests permission rules from what you approved by hand; clearance never writes
them in your settings.

## Install for every session

A step to take once you trust the tool. With clearance stopped, sessions behave as usual: it only
installs hooks that stay silent when nobody answers.

```sh
clearance install --dry-run   # what would change in ~/.claude/settings.json
clearance install             # backup, then add clearance's hooks next to yours
/plugin install clearance --marketplace quietmachineworks/clearance
```

The last line, typed in Claude Code, installs the companion mod; from a clone,
`claude plugin marketplace add <this folder>` then `claude plugin install clearance@clearance` does the
same. Sessions started afterwards are connected. `clearance install --remove` and
`claude plugin uninstall clearance@clearance` undo it.

Checks: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## Releasing

Rename the `Unreleased` section of `CHANGELOG.md` to the version (`## 0.5.0`), set it in `Cargo.toml`,
commit, then tag `v0.5.0` and push the tag: CI builds the Windows and macOS binaries and publishes them
with that section as notes.

## License

MIT
