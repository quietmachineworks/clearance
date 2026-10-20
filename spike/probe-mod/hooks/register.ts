// M0 spike (question 6): can a mod carry clearance's data and instructions?
// Every 5 s it posts the session's usage to the spike receiver, then asks for an
// instruction queued for this session and submits it as a prompt.
// ponytail: polling, no token - a spike against 127.0.0.1 only.
import type { Register } from 'claude-code'

const BASE = 'http://127.0.0.1:47800'

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const id = await $.session.id()
    $.clock.every(5000, async () => {
      try {
        const usage = await $.session.usage()
        await $.http.fetch(`${BASE}/mod/usage?session=${id}`, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(usage),
        })
        const inbox = await $.http.fetch(`${BASE}/mod/inbox?session=${id}`)
        if (inbox.ok && inbox.text) {
          void $.prompt.submit({ text: inbox.text })
          $.ui.status('clearance: instruction received')
        } else {
          $.ui.status(`clearance: context ${usage.context.percent ?? '?'} %`)
        }
      } catch {
        $.ui.status('clearance: receiver unreachable')
      }
    })
    return next(e)
  })
}
