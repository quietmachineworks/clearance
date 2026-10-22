// clearance companion mod (ADR-0004). Every 5 s it sends this session's usage to clearance, shows
// under the prompt what waits for you, and submits an instruction typed for this session in clearance.
// With clearance stopped it says nothing and does nothing.
import type { Register } from 'claude-code'

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    const session = await $.session.id()
    const home = (await $.env.get('USERPROFILE')) ?? (await $.env.get('HOME'))
    $.clock.every(5000, async () => {
      try {
        // Written by clearance at start: { url, token }. Missing: clearance is not running.
        const { url, token } = JSON.parse(await $.fs.read(`${home}/.clearance/endpoint.json`))
        const res = await $.http.fetch(`${url}/mod/sync`, {
          method: 'POST',
          headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
          body: JSON.stringify({ session, usage: await $.session.usage() }),
        })
        if (!res.ok) return $.ui.status(undefined)
        const { waiting, here, instruction, line } = JSON.parse(res.text)
        // Typed by the user in clearance: their own words, so it goes in as theirs.
        if (instruction) void $.prompt.submit({ text: instruction, asUser: true })
        // The server writes the line in the page's language; an older server does not.
        $.ui.status(line ?? (waiting ? `clearance · ${waiting} waiting for you${here ? `, ${here} here` : ''}` : 'clearance · nothing waiting'))
      } catch {
        $.ui.status(undefined)
      }
    })
    return next(e)
  })
}
