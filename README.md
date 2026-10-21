# clearance

A local control tower for Claude Code sessions running in parallel.

One page on your machine shows who is waiting for you (permission, plan approval, question, auto mode
denial), who is done, who is working and who is asleep, and lets you answer without hunting for the
right terminal. Click a session to unfold its workflow: subagents, what ran, progress, errors, diff.

Claude Code only, local only, zero dependencies.

Status: M3, the board, its decisions, the session detail and the companion mod. See `docs/BRIEF.md` for the whole plan.

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

Installing the hooks and the mod in your user settings, for every session, is a step to take once you
trust the tool. The Journal page suggests permission rules from what you approved by hand; clearance
never writes your settings.

Tests: `node --test`.

## License

MIT
