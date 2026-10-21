# clearance

A local control tower for Claude Code sessions running in parallel.

One page on your machine shows who is waiting for you (permission, plan approval, question, auto mode
denial), who is done, who is working and who is asleep, and lets you answer without hunting for the
right terminal. Click a session to unfold its workflow: subagents, what ran, progress, errors, diff.

Claude Code only, local only, zero dependencies.

Status: V1 milestones done (M0 to M4). See `docs/BRIEF.md` for the whole plan.

## Try it

Node 24 or later.

```sh
node src/server.ts
```

It prints the page address, with a one-time token, and writes `data/hooks.json`. Connect a session by
passing those hooks to it:

```sh
claude --settings /absolute/path/to/clearance/data/hooks.json
```

Requests from that session now show on the page as well as in its terminal; the first answer wins. With
clearance stopped, the session behaves as usual.

Add the companion mod to see the session's context, cost and your quota, and to send it instructions
from the page:

```sh
claude --settings /absolute/path/to/clearance/data/hooks.json --plugin-dir /absolute/path/to/clearance/mod
```

On Windows, the page can also start a session in a new Windows Terminal tab, connected to clearance,
and bring a session's tab to the front.

The Journal page suggests permission rules from what you approved by hand; clearance never writes
them in your settings.

## Install for every session

A step to take once you trust the tool. With clearance stopped, sessions behave as usual: it only
installs hooks that stay silent when nobody answers.

```sh
node src/install.ts --dry-run   # what would change in ~/.claude/settings.json
node src/install.ts             # backup, then add clearance's hooks next to yours
/plugin install clearance --marketplace quietmachineworks/clearance
```

The last line, typed in Claude Code, installs the companion mod; from a clone,
`claude plugin marketplace add <this folder>` then `claude plugin install clearance@clearance` does the
same. Sessions started afterwards are connected. `node src/install.ts --remove` and
`claude plugin uninstall clearance@clearance` undo it.

Tests: `node --test`.

## License

MIT
