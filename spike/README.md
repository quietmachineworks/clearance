# M0 spike

A throwaway hook receiver used to settle how Claude Code behaves with blocking `http` hooks. Results:
`docs/adr/0003-m0-hook-behaviour.md`. Not part of the product.

## Run it again

1. Start the receiver: `node spike/hook-server.ts` (listens on `127.0.0.1:47800`, logs to
   `spike/out/events.jsonl`, which is git-ignored because it holds real session payloads).
2. Pick a scenario: `curl "http://127.0.0.1:47800/mode?permission=hold:20:allow"`.
   - `permission`: `none`, `hang`, `hold:<seconds>:<allow|deny|none>`, `setmode:<mode>`
   - `ask`: `none`, `answer:<option label>`
   - `denied`: `none`, `retry`
3. From a throwaway folder you have trusted once (`claude`, accept, `/exit`), start a session:
   `claude --bg --model haiku --permission-mode default --settings <abs path>/spike/hooks.json -n t1 "<task>"`.
   `spike/hooks-timeout.json` sets a 15 s timeout on `PermissionRequest`.
4. Watch `claude agents --json`, then clean up with `claude stop <id>` and `claude rm <id>`.

Never point these hooks at your global settings: they would answer for every open session.
