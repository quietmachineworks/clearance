// clearance: the local control tower. One process on 127.0.0.1 (BRIEF section 6, ADR-0001, ADR-0003).
//   node src/server.ts            then open the address it prints
//   claude --settings data/hooks.json ...   to connect a session
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import { randomBytes, timingSafeEqual } from 'node:crypto'
import { appendFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { Board, respond, type Decision, type Session } from './board.ts'
import { agents, approvals, detail, modelOf, target, transcriptOf } from './claude.ts'
import { suggest } from './risk.ts'

const HOST = '127.0.0.1'
const PORT = Number(process.env.CLEARANCE_PORT ?? 47800)
const BASE = `http://${HOST}:${PORT}`
const DATA = new URL('../data/', import.meta.url)
const PAGE = new URL('../public/index.html', import.meta.url)

// The token survives restarts: running sessions keep the hooks they started with.
mkdirSync(DATA, { recursive: true })
const tokenFile = new URL('token', DATA)
const TOKEN = existsSync(tokenFile) ? readFileSync(tokenFile, 'utf8').trim() : randomBytes(24).toString('hex')
writeFileSync(tokenFile, TOKEN, { mode: 0o600 })

// Hook settings for `claude --settings`. PermissionRequest (permissions, questions, plans) runs in
// parallel with the terminal dialog, so it may wait. Everything else answers at once; a short
// timeout keeps a stuck server from slowing a session down.
// ponytail: an auto mode denial is held 60 s for a retry decision; set from what M1 observes.
const HOOKS: Record<string, { matcher?: string; timeout: number }> = {
  PermissionRequest: { timeout: 600 },
  PermissionDenied: { timeout: 60 },
  Stop: { timeout: 5 },
  StopFailure: { timeout: 5 },
  UserPromptSubmit: { timeout: 5 },
  SubagentStart: { timeout: 5 },
  SubagentStop: { timeout: 5 },
}
const hooksFile = new URL('hooks.json', DATA)
const settings = Object.fromEntries(
  Object.entries(HOOKS).map(([event, { matcher, timeout }]) => [
    event,
    [{ ...(matcher ? { matcher } : {}), hooks: [{ type: 'http', url: `${BASE}/hook/${event}`, headers: { Authorization: `Bearer ${TOKEN}` }, timeout }] }],
  ]),
)
writeFileSync(hooksFile, JSON.stringify({ hooks: settings }, null, 2), { mode: 0o600 })
// Where the companion mod finds us (mod/hooks/register.ts reads it every tick).
writeFileSync(new URL('endpoint.json', DATA), JSON.stringify({ url: BASE, token: TOKEN }), { mode: 0o600 })

// Append-only journal of what was decided from the page (BRIEF section 6).
const journalFile = new URL('journal.jsonl', DATA)
const journal = (entry: object) => appendFileSync(journalFile, JSON.stringify({ at: new Date().toISOString(), ...entry }) + '\n')
const journalTail = (n: number) =>
  existsSync(journalFile) ? readFileSync(journalFile, 'utf8').trim().split('\n').slice(-n).flatMap((l) => { try { return [JSON.parse(l)] } catch { return [] } }) : []

const board = new Board()
const held = new Map<string, ServerResponse>() // request id -> the hook call waiting for an answer
const clients = new Set<ServerResponse>() // open pages (SSE)
let last = ''

function publish() {
  const data = JSON.stringify(board.snapshot())
  if (data === last) return
  last = data
  for (const c of clients) c.write(`data: ${data}\n\n`)
}

// Answers a held hook call and takes the request off the board. {} means no decision: Claude Code
// carries on with its own dialog.
function finish(id: string, out: object = {}) {
  const res = held.get(id)
  held.delete(id)
  board.end(id)
  if (res && !res.writableEnded) send(res, 200, out)
}

function send(res: ServerResponse, status: number, body: object) {
  res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body))
}

const same = (a: string | undefined, b: string) => !!a && a.length === b.length && timingSafeEqual(Buffer.from(a), Buffer.from(b))
const cookie = (req: IncomingMessage) => /(?:^|;\s*)clearance=([^;]+)/.exec(req.headers.cookie ?? '')?.[1]

async function json(req: IncomingMessage): Promise<any> {
  let raw = ''
  for await (const chunk of req) raw += chunk
  try {
    return JSON.parse(raw || '{}')
  } catch {
    return {}
  }
}

// The model of each session gives its avatar's accessory; its transcript's last write, how long it
// has been idle.
function withModel(s: Session): Session {
  try {
    const file = transcriptOf(s.sessionId, s.cwd)
    return file ? { ...s, model: modelOf(file), lastActive: statSync(file).mtimeMs } : s
  } catch {
    return s
  }
}

async function poll() {
  const list = await agents()
  if (list) board.sync(list.map(withModel)).forEach((id) => finish(id))
  publish()
  setTimeout(poll, 2000)
}

const PAGE_HEADERS = {
  'content-type': 'text/html; charset=utf-8',
  'cache-control': 'no-store',
  'content-security-policy': "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:; frame-ancestors 'none'",
}

createServer(async (req, res) => {
  // Only our own host name: a page on another site cannot reach us through DNS rebinding.
  const host = req.headers.host ?? ''
  if (host !== `${HOST}:${PORT}` && host !== `localhost:${PORT}`) return void res.writeHead(421).end()
  const url = new URL(req.url ?? '/', BASE)

  if (req.method === 'POST' && url.pathname.startsWith('/hook/')) {
    if (!same(req.headers.authorization?.replace(/^Bearer /, ''), TOKEN)) return void res.writeHead(401).end()
    const event = url.pathname.slice('/hook/'.length)
    const body = await json(req)
    // CLEARANCE_LOG=1 keeps every hook payload in data/hooks.jsonl, to study a new event's shape.
    if (process.env.CLEARANCE_LOG) appendFileSync(new URL('hooks.jsonl', DATA), JSON.stringify({ at: new Date().toISOString(), event, body }) + '\n')
    const { request, ended } = board.hook(event, body)
    ended.forEach((id) => finish(id))
    if (request) {
      held.set(request.id, res)
      // Claude Code gave up (timeout) or the human answered in the terminal and the call was closed.
      res.on('close', () => {
        if (held.get(request.id) !== res) return
        held.delete(request.id)
        board.end(request.id)
        publish()
      })
    } else send(res, 200, {})
    publish()
    return
  }

  // The companion mod, every 5 s: its session's usage in, what waits and an instruction out.
  if (req.method === 'POST' && url.pathname === '/mod/sync') {
    if (!same(req.headers.authorization?.replace(/^Bearer /, ''), TOKEN)) return void res.writeHead(401).end()
    const { session, usage } = await json(req)
    if (typeof session !== 'string' || !session) return send(res, 400, { error: 'session required' })
    board.report(session, usage ?? {})
    const instruction = board.take(session)
    if (instruction) journal({ kind: 'instruction-delivered', session })
    publish()
    return send(res, 200, { waiting: board.requests.size, here: [...board.requests.values()].filter((r) => r.session === session).length, instruction })
  }

  if (req.method === 'GET' && url.pathname === '/') {
    const token = url.searchParams.get('token')
    if (token !== null) {
      if (!same(token, TOKEN)) return void res.writeHead(401).end('Wrong token.')
      return void res.writeHead(303, { location: '/', 'set-cookie': `clearance=${TOKEN}; HttpOnly; SameSite=Strict; Path=/` }).end()
    }
    if (!same(cookie(req), TOKEN)) return void res.writeHead(401, { 'content-type': 'text/plain' }).end('Open the address clearance printed at start.')
    return void res.writeHead(200, PAGE_HEADERS).end(readFileSync(PAGE))
  }

  if (!same(cookie(req), TOKEN)) return void res.writeHead(401).end()

  if (req.method === 'GET' && url.pathname === '/events') {
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' })
    res.write(`data: ${JSON.stringify(board.snapshot())}\n\n`)
    clients.add(res)
    req.on('close', () => clients.delete(res))
    return
  }

  if (req.method === 'GET' && url.pathname.startsWith('/api/session/')) {
    const id = url.pathname.slice('/api/session/'.length)
    try {
      const d = detail(id, board.sessions.get(id)?.cwd)
      return d ? send(res, 200, d) : send(res, 404, { error: 'No transcript for this session.' })
    } catch (e) {
      return send(res, 500, { error: (e as Error).message })
    }
  }

  if (req.method === 'GET' && url.pathname === '/api/journal') {
    try {
      return send(res, 200, { decisions: journalTail(200).reverse(), rules: suggest(approvals()) })
    } catch (e) {
      return send(res, 500, { error: (e as Error).message })
    }
  }

  // Every write from the page: same origin only.
  if (req.method !== 'POST' || req.headers.origin !== `http://${host}`) return void res.writeHead(403).end()

  if (url.pathname === '/api/decide') {
    const decision: Decision = await json(req)
    const request = board.requests.get(decision.id)
    if (!request) return send(res, 410, { error: 'Already answered, or the session moved on.' })
    let out: object
    try {
      out = respond(request, decision)
    } catch (e) {
      return send(res, 400, { error: (e as Error).message })
    }
    finish(request.id, out)
    publish()
    journal({ kind: 'decision', session: request.session, cwd: request.cwd, request: request.kind, tool: request.tool, target: target(request.input), risk: request.risk, action: decision.action, mode: decision.mode })
    return send(res, 200, { ok: true })
  }

  // An instruction for a session, handed to its companion mod on its next sync.
  if (url.pathname === '/api/instruct') {
    const { session, text } = await json(req)
    if (typeof session !== 'string' || typeof text !== 'string' || !text.trim()) return send(res, 400, { error: 'An instruction needs a session and some text.' })
    board.instruct(session, text.trim())
    journal({ kind: 'instruction', session, text: text.trim().slice(0, 500) })
    publish()
    return send(res, 200, { ok: true })
  }

  if (url.pathname === '/api/read') {
    board.read(String((await json(req)).session ?? ''))
    publish()
    return send(res, 200, { ok: true })
  }

  res.writeHead(404).end()
}).listen(PORT, HOST, () => {
  console.log(`clearance on ${BASE}/?token=${TOKEN}`)
  console.log(`hooks for a session: claude --settings ${fileURLToPath(hooksFile)}`)
  poll()
  // ponytail: the first scan of every transcript blocks a few seconds; done at start rather than on
  // the first journal view. Upgrade path: a worker thread if it grows.
  setTimeout(() => {
    try {
      approvals()
    } catch {}
  }, 3000)
})
