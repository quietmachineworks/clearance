// The one module that reads Claude Code itself: `claude agents --json` and the files under
// ~/.claude/projects (transcripts, subagents, workflows). Their formats may change between versions.
import { execFile } from 'node:child_process'
import { closeSync, existsSync, openSync, readdirSync, readFileSync, readSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { basename, join } from 'node:path'
import type { Session } from './board.ts'

// `claude agents --json`: every live session, its status and what it waits for. null when the reading
// failed, so that a hiccup does not empty the board.
export function agents(): Promise<Session[] | null> {
  return new Promise((resolve) => {
    execFile('claude', ['agents', '--json'], { timeout: 15_000, windowsHide: true }, (err, out) => {
      try {
        resolve(err ? null : JSON.parse(out))
      } catch {
        resolve(null)
      }
    })
  })
}

const projects = () => join(process.env.CLAUDE_CONFIG_DIR ?? join(homedir(), '.claude'), 'projects')
const list = (dir: string) => (existsSync(dir) ? readdirSync(dir) : [])

// <projects>/<cwd, every non-alphanumeric character turned into "-">/<session id>.jsonl, else any
// project folder holding that session.
export function transcriptOf(id: string, cwd?: string): string | null {
  if (!/^[\w-]{8,64}$/.test(id)) return null
  const guess = cwd ? join(projects(), cwd.replace(/[^A-Za-z0-9]/g, '-'), `${id}.jsonl`) : ''
  if (guess && existsSync(guess)) return guess
  for (const dir of list(projects())) {
    const file = join(projects(), dir, `${id}.jsonl`)
    if (existsSync(file)) return file
  }
  return null
}

// The model a session talks to now, from the end of its transcript.
// ponytail: a regex on the last 256 KB, cached on the file size; a full parse is for the detail view.
const models = new Map<string, { size: number; model?: string }>()
export function modelOf(file: string): string | undefined {
  const size = statSync(file).size
  const hit = models.get(file)
  if (hit?.size === size) return hit.model
  const len = Math.min(size, 256 * 1024)
  const buf = Buffer.alloc(len)
  const fd = openSync(file, 'r')
  readSync(fd, buf, 0, len, size - len)
  closeSync(fd)
  const model = [...buf.toString('utf8').matchAll(/"model":"(claude-[^"]+)"/g)].pop()?.[1] ?? hit?.model
  models.set(file, { size, model })
  return model
}

// ---- transcripts ----

export interface Step {
  at: string
  tool: string
  target: string
  error?: string
}

export interface Edit {
  at: string
  file: string
  before?: string
  after?: string
  agent?: string
}

interface Launch {
  id: string
  at: string
  name?: string
  script?: string
  scriptPath?: string
  runId?: string
}

export interface Transcript {
  title?: string
  branch?: string
  model?: string
  mode?: string
  start?: string
  end?: string
  lastText?: string
  prompts: { at: string; text: string }[]
  steps: Step[]
  edits: Edit[]
  launches: Launch[]
}

const EDIT_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit'])
const cut = (s: unknown, n: number) => (typeof s === 'string' ? (s.length > n ? s.slice(0, n) + '…' : s) : undefined)
const text = (content: unknown): string =>
  typeof content === 'string' ? content : Array.isArray(content) ? content.map((c: any) => c?.text ?? '').join('\n') : ''

// The name a workflow script declares in `meta.name`.
const metaName = (script = '') => /\bname\s*:\s*(['"`])(.*?)\1/.exec(script)?.[2]

// What a tool call is about, in one line.
export function target(input: any = {}): string {
  const t = input.command ?? input.file_path ?? input.notebook_path ?? input.url ?? input.pattern ?? input.query ?? input.description ?? input.prompt ?? input.name ?? input.skill ?? input.scriptPath ?? metaName(input.script) ?? ''
  return cut(String(t).replace(/\s+/g, ' ').trim(), 200) ?? ''
}

function lines(file: string): any[] {
  return readFileSync(file, 'utf8')
    .split('\n')
    .flatMap((l) => {
      try {
        return l ? [JSON.parse(l)] : []
      } catch {
        return []
      }
    })
}

// Parsed once per file size: a live transcript only grows.
const parsed = new Map<string, { size: number; value: Transcript }>()
export function readTranscript(file: string): Transcript {
  const size = statSync(file).size
  const hit = parsed.get(file)
  if (hit?.size === size) return hit.value

  const t: Transcript = { prompts: [], steps: [], edits: [], launches: [] }
  const calls = new Map<string, Step>()
  const launches = new Map<string, Launch>()
  for (const e of lines(file)) {
    if (e.timestamp) {
      t.start ??= e.timestamp
      t.end = e.timestamp
    }
    if (e.gitBranch) t.branch = e.gitBranch
    if (e.type === 'ai-title') t.title = e.aiTitle
    if (e.type === 'permission-mode') t.mode = e.permissionMode
    if (e.type === 'assistant') {
      if (/^claude-/.test(e.message?.model ?? '')) t.model = e.message.model // not "<synthetic>" replays
      for (const c of e.message?.content ?? []) {
        if (c.type === 'text' && c.text) t.lastText = c.text
        if (c.type !== 'tool_use') continue
        const step: Step = { at: e.timestamp, tool: c.name, target: target(c.input) }
        calls.set(c.id, step)
        t.steps.push(step)
        if (EDIT_TOOLS.has(c.name)) {
          const file = c.input?.file_path ?? c.input?.notebook_path ?? ''
          const parts = c.name === 'MultiEdit' ? (c.input?.edits ?? []) : [c.input ?? {}]
          for (const p of parts) t.edits.push({ at: e.timestamp, file, before: cut(p.old_string, 2000), after: cut(p.new_string ?? p.content ?? p.new_source, 2000) })
        }
        if (c.name === 'Workflow') {
          const launch: Launch = { id: c.id, at: e.timestamp, name: c.input?.name, script: c.input?.script, scriptPath: c.input?.scriptPath }
          launches.set(c.id, launch)
          t.launches.push(launch)
        }
      }
    }
    if (e.type === 'user') {
      const content = e.message?.content
      const results = Array.isArray(content) ? content.filter((c: any) => c.type === 'tool_result') : []
      for (const r of results) {
        const step = calls.get(r.tool_use_id)
        if (step && r.is_error) step.error = cut(text(r.content), 400)
        const launch = launches.get(r.tool_use_id)
        if (launch && e.toolUseResult?.runId) launch.runId = e.toolUseResult.runId
      }
      // What the user typed; injected context (reminders, command output) starts with a tag.
      const typed = results.length ? '' : text(content).trim()
      if (typed && !e.isMeta && !typed.startsWith('<')) t.prompts.push({ at: e.timestamp, text: cut(typed, 2000)! })
    }
  }
  parsed.set(file, { size, value: t })
  return t
}

// ---- subagents and workflows ----

export interface Agent {
  id: string
  label: string
  type?: string
  phase?: string
  model?: string
  start?: string
  end?: string
  tools: number
  errors: number
  running: boolean
  result?: string
}

export interface Run {
  runId: string
  name: string
  status: string // completed, running, interrupted, or what Claude Code records
  start?: string
  durationMs?: number
  phases: string[] // declared, in order
  current?: string
  agents: Agent[]
}

const json = (file: string) => {
  try {
    return JSON.parse(readFileSync(file, 'utf8'))
  } catch {
    return undefined
  }
}

// ponytail: "running" means no result yet and a transcript written in the last 10 minutes; an agent
// stuck longer in one tool call reads as stopped. Upgrade path: a SubagentStop seen by the hooks.
const RUNNING_MS = 10 * 60_000

function agentOf(dir: string, id: string, label?: string, phase?: string, result?: unknown): { agent: Agent; transcript?: Transcript } {
  const meta = json(join(dir, `agent-${id}.meta.json`)) ?? {}
  const file = join(dir, `agent-${id}.jsonl`)
  const t = existsSync(file) ? readTranscript(file) : undefined
  const recent = !!t?.end && Date.now() - Date.parse(t.end) < RUNNING_MS
  const agent: Agent = {
    id,
    label: label ?? meta.description ?? meta.agentType ?? id,
    type: meta.agentType,
    phase: phase ?? meta.workflowPhase,
    model: t?.model,
    start: t?.start,
    end: t?.end,
    tools: t?.steps.length ?? 0,
    errors: t?.steps.filter((s) => s.error).length ?? 0,
    running: result === undefined && !meta.finished && recent,
    result: result === undefined ? undefined : cut(typeof result === 'string' ? result : JSON.stringify(result), 300),
  }
  return { agent, transcript: t }
}

// The phase titles a workflow script declares in `meta.phases`.
// ponytail: a regex on the script text, not a JS parser; titles must be plain string literals.
export function phasesOf(script = ''): string[] {
  const block = /phases\s*:\s*\[([\s\S]*?)\]\s*[,}\n]/.exec(script)?.[1] ?? ''
  return [...block.matchAll(/title\s*:\s*(['"`])(.*?)\1/g)].map((m) => m[2])
}

function scriptOf(launch?: Launch): string {
  if (launch?.script) return launch.script
  const path = launch?.scriptPath
  if (!path || !/\.(m?js|ts)$/.test(path) || !existsSync(path) || statSync(path).size > 1_000_000) return ''
  return readFileSync(path, 'utf8')
}

function runOf(sessionDir: string, runId: string, launch: Launch | undefined, edits: Edit[]): Run {
  const record = json(join(sessionDir, 'workflows', `${runId}.json`))
  const dir = join(sessionDir, 'subagents', 'workflows', runId)
  const journal = existsSync(join(dir, 'journal.jsonl')) ? lines(join(dir, 'journal.jsonl')) : []
  const results = new Map(journal.filter((e) => e.type === 'result').map((e) => [e.agentId, e.result]))
  const agents = journal
    .filter((e) => e.type === 'started' && e.agentId)
    .map((e) => {
      const { agent, transcript } = agentOf(dir, e.agentId, e.label, e.phase, results.has(e.agentId) ? (results.get(e.agentId) ?? null) : undefined)
      for (const edit of transcript?.edits ?? []) edits.push({ ...edit, agent: agent.label })
      return agent
    })
  // No record yet: still running, or stopped with agents that never returned.
  const status = record?.status ?? (agents.some((a) => a.running) ? 'running' : agents.some((a) => a.result === undefined) ? 'interrupted' : 'ended')
  const declared = (record?.phases ?? []).map((p: any) => p.title).filter(Boolean)
  return {
    runId,
    name: record?.workflowName ?? launch?.name ?? metaName(launch?.script) ?? (launch?.scriptPath ? basename(launch.scriptPath).replace(/\.\w+$/, '') : runId),
    status,
    start: record?.startTime ? new Date(record.startTime).toISOString() : (launch?.at ?? agents[0]?.start),
    durationMs: record?.durationMs,
    phases: declared.length ? declared : phasesOf(scriptOf(launch)),
    current: agents.at(-1)?.phase,
    agents,
  }
}

export interface Detail extends Transcript {
  errors: Step[]
  loops: { tool: string; target: string; count: number }[]
  runs: Run[]
  agents: Agent[] // subagents outside any workflow
}

// The same call failing at least three times in a row.
export function loopsOf(steps: Step[]) {
  const loops: Detail['loops'] = []
  let run: Step[] = []
  for (const s of [...steps, { at: '', tool: '', target: '' }]) {
    const same = run.length && s.error && s.tool === run[0].tool && s.target === run[0].target
    if (same) {
      run.push(s)
      continue
    }
    if (run.length >= 3) loops.push({ tool: run[0].tool, target: run[0].target, count: run.length })
    run = s.error ? [s] : []
  }
  return loops
}

export function detail(id: string, cwd?: string): Detail | null {
  const file = transcriptOf(id, cwd)
  if (!file) return null
  const t = readTranscript(file)
  const sessionDir = file.replace(/\.jsonl$/, '')
  const edits = [...t.edits]
  const byRun = new Map(t.launches.filter((l) => l.runId).map((l) => [l.runId!, l]))
  const runIds = [...new Set([...byRun.keys(), ...list(join(sessionDir, 'subagents', 'workflows'))])]
  const runs = runIds.map((r) => runOf(sessionDir, r, byRun.get(r), edits)).sort((a, b) => String(a.start).localeCompare(String(b.start)))
  const agents = list(join(sessionDir, 'subagents'))
    .filter((f) => /^agent-[\w]+\.meta\.json$/.test(f))
    .map((f) => {
      const { agent, transcript } = agentOf(join(sessionDir, 'subagents'), f.slice('agent-'.length, -'.meta.json'.length))
      for (const edit of transcript?.edits ?? []) edits.push({ ...edit, agent: agent.label })
      return agent
    })
  return {
    ...t,
    prompts: t.prompts.slice(-100),
    steps: t.steps.slice(-300),
    edits: edits.sort((a, b) => a.at.localeCompare(b.at)).slice(-100),
    errors: t.steps.filter((s) => s.error).slice(-50),
    loops: loopsOf(t.steps),
    runs,
    agents,
  }
}
