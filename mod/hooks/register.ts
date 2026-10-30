// clearance companion mod (ADR-0004). Every 5 s it sends this session's usage to clearance, shows
// under the prompt what waits for you, submits an instruction typed for this session in clearance, and
// stops the session when asked to. With clearance stopped it says nothing and does nothing.
import type { Register } from 'claude-code'

export const register: Register = (on) => {
  // The main loop's running turn, the one a stop from clearance ends.
  let turn: string | undefined
  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) turn = e.turnId
    return next(e)
  })
  on('turn.complete', async ($, e, next) => {
    if (e.agentId === undefined && e.turnId === turn) turn = undefined
    return next(e)
  })
  on('session.start', async ($, e, next) => {
    const home = (await $.env.get('USERPROFILE')) ?? (await $.env.get('HOME'))
    $.clock.every(5000, async () => {
      try {
        // Read every time: after /resume or /clear the process goes on under another id, and no
        // session.start fires for it.
        const session = await $.session.id()
        // Written by clearance at start: { url, token }. Missing: clearance is not running.
        const { url, token } = JSON.parse(await $.fs.read(`${home}/.clearance/endpoint.json`))
        const res = await $.http.fetch(`${url}/mod/sync`, {
          method: 'POST',
          headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
          // mod 2: this version stops its session when clearance asks.
          body: JSON.stringify({ session, usage: await $.session.usage(), mod: 2 }),
        })
        if (!res.ok) return $.ui.status(undefined)
        const { waiting, here, instruction, stop, line } = JSON.parse(res.text)
        // Stopped from clearance: the running turn ends with its tools; "exit" then quits the session,
        // whose transcript stays for `claude --resume`.
        if (stop) {
          if (turn) await $.turn.abort({ turnId: turn }).catch(() => undefined)
          if (stop === 'exit') void $.command.run({ command: 'exit' })
        }
        // Typed by the user in clearance: their own words, so it goes in as theirs. A slash command
        // (/rename, /model, /compact) runs as one, the way the person would type it.
        const command = /^\/([\w:-]+)\s*([\s\S]*)$/.exec(instruction ?? '')
        if (command) void $.command.run({ command: command[1], args: command[2].trim() })
        else if (instruction) void $.prompt.submit({ text: instruction, asUser: true })
        // The server writes the line in the page's language; an older server does not.
        $.ui.status(line ?? (waiting ? `clearance · ${waiting} waiting for you${here ? `, ${here} here` : ''}` : 'clearance · nothing waiting'))
      } catch {
        $.ui.status(undefined)
      }
    })
    return next(e)
  })
}
