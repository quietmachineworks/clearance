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

It prints the page address with its token, and writes `~/.clearance/hooks.json`. Connect a session by
passing those hooks to it:

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

The page can also start a session in a new terminal tab, connected to clearance, and bring a session's
tab to the front: Windows Terminal on Windows, Terminal on macOS.

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

## License

MIT
