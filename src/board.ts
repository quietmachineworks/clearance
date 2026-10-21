// Board state: what waits for the user, what is done, who is working (BRIEF section 3, ADR-0003).
// Pure: the server holds the HTTP responses and sends what respond() builds.
import { classify, RANK, type Risk } from './risk.ts'

export type Kind = 'permission' | 'question' | 'plan' | 'denied'

export interface Request {
  id: string
  session: string
  cwd?: string
  kind: Kind
  at: number
  tool: string
  input: any
  risk: Risk
  suggestions: any[] // Claude Code's own "always allow" proposal
  mode?: string // permission mode when asked
  reason?: string // auto mode denial reason
  seenWaiting: boolean // the session reported `waiting` since this request arrived
}

// One entry of `claude agents --json`.
export interface Session {
  sessionId: string
  name?: string
  cwd?: string
  kind?: string
  status?: string
  waitingFor?: string
  model?: string // added by the server, from the transcript
  lastActive?: number // added by the server: the transcript's last write, epoch ms
}

export interface Done {
  session: string
  cwd?: string
  at: number
  message: string
  failed?: boolean
}

// What the companion mod reports: `$.session.usage()` (ADR-0004), stamped on arrival.
export interface Usage {
  at: number
  context?: { tokens?: number; window: number; percent?: number }
  rateLimits?: { kind: string; percentUsed: number; resetsAt?: string }[]
  cost?: { usd: number }
}

// An instruction typed in clearance, delivered once by the session's mod.
export interface Instruction {
  text: string
  at: number
  delivered?: number
}

export interface Decision {
  id: string
  action: 'allow' | 'rule' | 'deny' | 'answer' | 'retry' | 'dismiss'
  reason?: string // sent to the agent with a deny
  mode?: string // setMode sent with an allow
  answers?: Record<string, string> // question text -> option label(s), comma separated when multiSelect
  text?: string // free answer to a question
  typed?: string // must be "GO" to approve a plan
}

// Plan approvals come as AskUserQuestion with a GO option (BRIEF section 3).
const isGo = (label: string) => /^go$/i.test(label.trim())

export class Board {
  requests = new Map<string, Request>()
  sessions = new Map<string, Session>()
  done = new Map<string, Done>()
  agents = new Map<string, Set<string>>() // session -> running subagents
  usage = new Map<string, Usage>() // session -> last report of its companion mod
  inbox = new Map<string, Instruction>() // session -> last instruction typed for it
  private seq = 0

  report(session: string, usage: Omit<Usage, 'at'>, now = Date.now()) {
    this.usage.set(session, { context: usage.context, rateLimits: usage.rateLimits, cost: usage.cost, at: now })
  }

  instruct(session: string, text: string, now = Date.now()) {
    this.inbox.set(session, { text, at: now })
  }

  // The instruction waiting for this session, handed out once.
  take(session: string, now = Date.now()): string | undefined {
    const i = this.inbox.get(session)
    if (!i || i.delivered) return undefined
    i.delivered = now
    return i.text
  }

  // A hook event arrived: the request to hold, if any, and the requests it ends.
  hook(event: string, body: any, now = Date.now()): { request?: Request; ended: string[] } {
    const session = String(body.session_id ?? '')
    const base = { id: String(++this.seq), session, cwd: body.cwd, at: now, tool: body.tool_name ?? '', input: body.tool_input ?? {}, mode: body.permission_mode, suggestions: [], seenWaiting: false }
    let request: Request | undefined
    let ended: string[] = []

    // Questions come through PermissionRequest too, in parallel with the terminal dialog. A held
    // PreToolUse hook would hide that dialog until it answers (observed in M1, ADR-0005).
    if (event === 'PermissionRequest' && base.tool === 'AskUserQuestion') {
      const plan = (base.input.questions ?? []).some((q: any) => (q.options ?? []).some((o: any) => isGo(o.label ?? '')))
      request = { ...base, kind: plan ? 'plan' : 'question', risk: plan ? 'infra' : 'read' }
    } else if (event === 'PermissionRequest') {
      request = { ...base, kind: 'permission', risk: classify(base.tool, base.input), suggestions: body.permission_suggestions ?? [] }
    } else if (event === 'PermissionDenied') {
      request = { ...base, kind: 'denied', risk: classify(base.tool, base.input), reason: body.reason }
    } else if (event === 'Stop' || event === 'StopFailure') {
      ended = this.drop(session)
      const failed = event === 'StopFailure'
      this.done.set(session, { session, cwd: body.cwd, at: now, failed, message: body.last_assistant_message ?? body.error ?? (failed ? 'Stopped on error.' : '') })
    } else if (event === 'UserPromptSubmit') {
      ended = this.drop(session)
      this.done.delete(session)
    } else if (event === 'SubagentStart' && body.agent_type) { // an empty type is an internal agent
      if (!this.agents.has(session)) this.agents.set(session, new Set())
      this.agents.get(session)!.add(body.agent_id)
    } else if (event === 'SubagentStop') {
      this.agents.get(session)?.delete(body.agent_id)
    }

    if (request) this.requests.set(request.id, request)
    return { request, ended }
  }

  // New `claude agents --json` reading: the requests whose session stopped waiting, i.e. answered
  // in the terminal or gone.
  // ponytail: per session, not per request. With parallel subagents, a request answered in the
  // terminal while a sibling still waits stays on the board until Stop; answering it is harmless.
  sync(list: Session[]): string[] {
    const before = this.sessions
    this.sessions = new Map(list.map((s) => [s.sessionId, s]))
    for (const map of [this.done, this.agents, this.usage, this.inbox]) {
      for (const id of map.keys()) if (before.has(id) && !this.sessions.has(id)) map.delete(id)
    }
    const ended: string[] = []
    for (const r of this.requests.values()) {
      if (this.sessions.get(r.session)?.status === 'waiting') r.seenWaiting = true
      else if (r.seenWaiting) ended.push(r.id)
    }
    ended.forEach((id) => this.requests.delete(id))
    return ended
  }

  end(id: string) {
    this.requests.delete(id)
  }

  read(session: string) {
    this.done.delete(session)
  }

  private drop(session: string): string[] {
    const ids = [...this.requests.values()].filter((r) => r.session === session).map((r) => r.id)
    ids.forEach((id) => this.requests.delete(id))
    return ids
  }

  snapshot() {
    const requests = [...this.requests.values()].sort((a, b) => RANK[b.risk] - RANK[a.risk] || a.at - b.at)
    const sessions = [...this.sessions.values()].map((s) => ({
      ...s,
      agents: this.agents.get(s.sessionId)?.size ?? 0,
      usage: this.usage.get(s.sessionId),
      instruction: this.inbox.get(s.sessionId),
    }))
    // The account's quota is the same for every session: the freshest report wins.
    const quota = [...this.usage.values()].filter((u) => u.rateLimits?.length).sort((a, b) => b.at - a.at)[0]?.rateLimits
    return { requests, sessions, done: [...this.done.values()].sort((a, b) => a.at - b.at), quota }
  }
}

// The hook answer for a decision taken on the page. Throws on a decision the request does not allow.
export function respond(r: Request, d: Decision): object {
  if (d.action === 'dismiss') return {} // no decision: the terminal dialog stays
  const out = (hookEventName: string, rest: object) => ({ hookSpecificOutput: { hookEventName, ...rest } })
  const decision = (decision: object) => out('PermissionRequest', { decision })

  if (r.kind === 'permission') {
    if (d.action === 'deny') return decision({ behavior: 'deny', message: d.reason || 'Denied from clearance.' })
    if (d.action === 'rule' && (r.risk === 'infra' || r.suggestions.length === 0)) throw new Error('no rule can be added for this request')
    if (d.action !== 'allow' && d.action !== 'rule') throw new Error(`${d.action} does not apply to a permission`)
    if (r.risk === 'infra' && d.typed !== 'GO') throw new Error('type GO to allow an infrastructure write')
    const updates = [...(d.action === 'rule' ? r.suggestions : []), ...(d.mode ? [{ type: 'setMode', mode: d.mode, destination: 'session' }] : [])]
    return decision({ behavior: 'allow', ...(updates.length ? { updatedPermissions: updates } : {}) })
  }

  if (r.kind === 'question' || r.kind === 'plan') {
    if (d.action === 'deny') return decision({ behavior: 'deny', message: d.reason || 'The user declined to answer.' })
    if (d.action !== 'answer') throw new Error(`${d.action} does not apply to a question`)
    // A free answer goes back as the message of a deny: AskUserQuestion only accepts option labels.
    if (d.text) return decision({ behavior: 'deny', message: `The user answered: ${d.text}` })
    const questions: any[] = r.input.questions ?? []
    const answers = d.answers ?? {}
    for (const q of questions) {
      const labels = (q.options ?? []).map((o: any) => o.label)
      const picked = String(answers[q.question] ?? '').split(', ')
      if (!picked.every((l) => labels.includes(l))) throw new Error(`no valid answer for "${q.question}"`)
      if (r.kind === 'plan' && picked.some(isGo) && d.typed !== 'GO') throw new Error('type GO to approve a plan')
    }
    return decision({ behavior: 'allow', updatedInput: { ...r.input, answers } })
  }

  if (d.action === 'retry') return out('PermissionDenied', { retry: true })
  throw new Error(`${d.action} does not apply to an auto mode denial`)
}
