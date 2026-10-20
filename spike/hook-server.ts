// M0 spike: a throwaway hook receiver to observe how Claude Code behaves with
// blocking http hooks. Logs every hook call to spike/out/events.jsonl.
// ponytail: no token, no Origin check - it listens on 127.0.0.1 for the length of
// a test session only. The real server (M1) gets both, see BRIEF section 6.
import { createServer } from 'node:http'
import { appendFileSync, mkdirSync } from 'node:fs'

const PORT = 47800
const OUT = new URL('./out/', import.meta.url)
mkdirSync(OUT, { recursive: true })
const LOG = new URL('events.jsonl', OUT)

// Current scenario, changed between tests with GET /mode?permission=...&ask=...&denied=...
//   permission: none | hang | hold:<s>:<allow|deny|none> | manual | setmode:<mode>
//   manual holds every request until GET /release?behavior=<allow|deny|none>
//   ask:        none | answer:<label>
//   denied:     none | retry
const mode: Record<string, string> = { permission: 'none', ask: 'none', denied: 'none' }

const pending: Array<(behavior: string) => void> = []
// Instructions queued for the probe mod (question 6), handed out once, to any session.
const inbox: string[] = []

const log = (entry: object) => appendFileSync(LOG, JSON.stringify({ t: new Date().toISOString(), ...entry }) + '\n')
const sleep = (s: number) => new Promise((r) => setTimeout(r, s * 1000))

function permission(behavior: string, extra: object = {}) {
  if (behavior === 'none') return {}
  return { hookSpecificOutput: { hookEventName: 'PermissionRequest', decision: { behavior, ...extra } } }
}

async function answer(event: string, body: any): Promise<object | null> {
  if (event === 'PermissionRequest') {
    const [kind, a, b] = mode.permission.split(':')
    if (kind === 'hang') return null
    if (kind === 'manual') return new Promise((resolve) => pending.push((behavior) => resolve(permission(behavior))))
    if (kind === 'hold') { await sleep(Number(a)); return permission(b) }
    if (kind === 'setmode') return permission('allow', { updatedPermissions: [{ type: 'setMode', mode: a, destination: 'session' }] })
    return {}
  }
  if (event === 'PreToolUse' && body.tool_name === 'AskUserQuestion' && mode.ask.startsWith('answer:')) {
    const label = mode.ask.slice('answer:'.length)
    const questions = body.tool_input?.questions ?? []
    const answers = Object.fromEntries(questions.map((q: any) => [q.question, label]))
    return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'allow', updatedInput: { questions, answers } } }
  }
  if (event === 'PermissionDenied' && mode.denied === 'retry') {
    return { hookSpecificOutput: { hookEventName: 'PermissionDenied', retry: true } }
  }
  return {}
}

createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', `http://127.0.0.1:${PORT}`)
  if (req.method === 'GET' && url.pathname === '/mode') {
    for (const [k, v] of url.searchParams) mode[k] = v
    log({ kind: 'mode', mode })
    res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(mode))
    return
  }
  if (req.method === 'GET' && url.pathname === '/release') {
    const behavior = url.searchParams.get('behavior') ?? 'none'
    const n = pending.length
    pending.splice(0).forEach((release) => release(behavior))
    log({ kind: 'release', behavior, released: n })
    res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ behavior, released: n }))
    return
  }
  if (req.method === 'GET' && url.pathname === '/instruct') {
    inbox.push(url.searchParams.get('text') ?? '')
    log({ kind: 'instruct', queued: inbox.length })
    res.writeHead(200).end('queued')
    return
  }
  if (req.method === 'GET' && url.pathname === '/mod/inbox') {
    const text = inbox.shift()
    if (text) log({ kind: 'inbox', session: url.searchParams.get('session'), text })
    res.writeHead(200, { 'content-type': 'text/plain' }).end(text ?? '')
    return
  }
  if (req.method === 'POST' && url.pathname === '/mod/usage') {
    let raw = ''
    for await (const chunk of req) raw += chunk
    log({ kind: 'usage', session: url.searchParams.get('session'), usage: JSON.parse(raw || '{}') })
    res.writeHead(200).end()
    return
  }
  if (req.method !== 'POST' || !url.pathname.startsWith('/hook/')) { res.writeHead(404).end(); return }

  const event = url.pathname.slice('/hook/'.length)
  let raw = ''
  for await (const chunk of req) raw += chunk
  const body = JSON.parse(raw || '{}')
  const started = Date.now()
  let closed = false
  // Fires when Claude Code gives up on the hook (timeout) before we answer.
  res.on('close', () => { if (!res.writableEnded) { closed = true; log({ kind: 'abandoned', event, session: body.session_id, heldMs: Date.now() - started }) } })
  log({ kind: 'hook', event, session: body.session_id, permission_mode: body.permission_mode, tool: body.tool_name, body, scenario: { ...mode } })

  const out = await answer(event, body)
  if (out === null || closed) return
  res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(out))
  log({ kind: 'answered', event, session: body.session_id, heldMs: Date.now() - started, out })
}).listen(PORT, '127.0.0.1', () => console.log(`spike hook server on http://127.0.0.1:${PORT}`))
