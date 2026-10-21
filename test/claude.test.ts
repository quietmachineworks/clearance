import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// An invented Claude Code home: one project, one session that ran a two-phase workflow.
const home = mkdtempSync(join(tmpdir(), 'clearance-test-'))
process.env.CLAUDE_CONFIG_DIR = home
const { detail, loopsOf, modelOf, phasesOf, target, transcriptOf } = await import('../src/claude.ts')

const ID = '0a1b2c3d-0000-4000-8000-000000000001'
const CWD = '/work/orchard'
const project = join(home, 'projects', '-work-orchard')
const session = join(project, ID)
const run = join(session, 'subagents', 'workflows', 'wf_test-001')
mkdirSync(run, { recursive: true })
const jsonl = (file: string, records: object[]) => writeFileSync(file, records.map((r) => JSON.stringify(r)).join('\n') + '\n')
const at = (min: number) => new Date(Date.now() - (60 - min) * 60_000).toISOString()
const use = (id: string, name: string, input: object, min: number, model = 'claude-sonnet-5-5') => ({ type: 'assistant', timestamp: at(min), gitBranch: 'feature/tide', message: { model, content: [{ type: 'tool_use', id, name, input }] } })
const result = (id: string, min: number, is_error = false, extra = {}) => ({ type: 'user', timestamp: at(min), message: { content: [{ type: 'tool_result', tool_use_id: id, is_error, content: is_error ? 'Exit code 1' : 'ok' }] }, ...extra })

jsonl(join(project, `${ID}.jsonl`), [
  { type: 'permission-mode', permissionMode: 'auto' },
  { type: 'user', timestamp: at(0), message: { content: '<system-reminder>context</system-reminder>' } },
  { type: 'user', timestamp: at(1), isMeta: true, message: { content: 'injected' } },
  { type: 'user', timestamp: at(2), message: { content: 'Ship the tide tables.' } },
  { type: 'ai-title', aiTitle: 'Tide tables' },
  use('t1', 'Bash', { command: 'npm test' }, 3),
  result('t1', 4, true),
  use('t2', 'Bash', { command: 'npm test' }, 5),
  result('t2', 6, true),
  use('t3', 'Bash', { command: 'npm test' }, 7),
  result('t3', 8, true),
  use('t4', 'MultiEdit', { file_path: 'src/tide.ts', edits: [{ old_string: 'a', new_string: 'b' }, { old_string: 'c', new_string: 'd' }] }, 9),
  result('t4', 10),
  use('t5', 'Workflow', { scriptPath: '/nowhere/ship.js', args: {} }, 11),
  result('t5', 12, false, { toolUseResult: { status: 'async_launched', runId: 'wf_test-001' } }),
])
mkdirSync(join(session, 'workflows'), { recursive: true })
writeFileSync(join(session, 'workflows', 'wf_test-001.json'), JSON.stringify({ workflowName: 'ship', status: 'completed', startTime: Date.now() - 50 * 60_000, durationMs: 600_000, phases: [{ title: 'Build' }, { title: 'Check' }] }))
jsonl(join(run, 'journal.jsonl'), [
  { type: 'launched' },
  { type: 'started', agentId: 'a1', label: 'build', phase: 'Build' },
  { type: 'result', agentId: 'a1', result: { ok: true } },
  { type: 'started', agentId: 'a2', label: 'check', phase: 'Check' },
])
writeFileSync(join(run, 'agent-a1.meta.json'), JSON.stringify({ agentType: 'builder', workflowPhase: 'Build' }))
jsonl(join(run, 'agent-a1.jsonl'), [use('b1', 'Write', { file_path: 'dist/tide.json', content: '{}' }, 20, 'claude-haiku-5-5'), result('b1', 21)])
writeFileSync(join(run, 'agent-a2.meta.json'), JSON.stringify({ agentType: 'checker', workflowPhase: 'Check' }))
jsonl(join(run, 'agent-a2.jsonl'), [use('c1', 'Read', { file_path: 'dist/tide.json' }, 59, 'claude-opus-5-5')])
writeFileSync(join(session, 'subagents', 'agent-x1.meta.json'), JSON.stringify({ agentType: 'Explore', description: 'find tide code', finished: true }))
jsonl(join(session, 'subagents', 'agent-x1.jsonl'), [use('x', 'Grep', { pattern: 'tide' }, 30, 'claude-haiku-5-5'), result('x', 31)])

test('finds a session from its folder, and refuses odd ids', () => {
  assert.equal(transcriptOf(ID, CWD), join(project, `${ID}.jsonl`))
  assert.equal(transcriptOf(ID), join(project, `${ID}.jsonl`))
  assert.equal(transcriptOf('../etc'), null)
  assert.equal(modelOf(join(project, `${ID}.jsonl`)), 'claude-sonnet-5-5')
})

test('session header, prompts, errors, loops and edits', () => {
  const d = detail(ID, CWD)!
  assert.equal(d.title, 'Tide tables')
  assert.equal(d.branch, 'feature/tide')
  assert.equal(d.mode, 'auto')
  assert.deepEqual(d.prompts.map((p) => p.text), ['Ship the tide tables.'])
  assert.equal(d.errors.length, 3)
  assert.deepEqual(d.loops, [{ tool: 'Bash', target: 'npm test', count: 3 }])
  assert.deepEqual(d.edits.map((e) => `${e.agent ?? 'session'}:${e.file}:${e.after}`), ['session:src/tide.ts:b', 'session:src/tide.ts:d', 'build:dist/tide.json:{}'])
})

test('workflow run: declared phases, agents, current phase', () => {
  const [r] = detail(ID, CWD)!.runs
  assert.equal(r.name, 'ship')
  assert.equal(r.status, 'completed')
  assert.deepEqual(r.phases, ['Build', 'Check'])
  assert.equal(r.current, 'Check')
  assert.deepEqual(r.agents.map((a) => [a.label, a.type, a.model, a.tools, a.running, a.result]), [
    ['build', 'builder', 'claude-haiku-5-5', 1, false, '{"ok":true}'],
    ['check', 'checker', 'claude-opus-5-5', 1, true, undefined],
  ])
})

test('subagents outside workflows', () => {
  assert.deepEqual(detail(ID, CWD)!.agents.map((a) => [a.label, a.type, a.running]), [['find tide code', 'Explore', false]])
})

test('loops need three failures of the same call in a row', () => {
  const s = (target: string, error?: string) => ({ at: '', tool: 'Bash', target, error })
  assert.deepEqual(loopsOf([s('a', 'x'), s('a', 'x'), s('b', 'x'), s('a', 'x')]), [])
  assert.deepEqual(loopsOf([s('a', 'x'), s('a', 'x'), s('a', 'x'), s('a'), s('a', 'x')]), [{ tool: 'Bash', target: 'a', count: 3 }])
})

test('phases and name declared in a workflow script', () => {
  const script = "export const meta = {\n  name: 'audit',\n  phases: [{ title: 'Review' }, { title: \"Verify\", detail: 'all' }],\n}\n"
  assert.deepEqual(phasesOf(script), ['Review', 'Verify'])
  assert.deepEqual(phasesOf('no meta'), [])
  assert.equal(target({ script }), 'audit')
  assert.equal(target({ command: 'npm   test\n --watch' }), 'npm test --watch')
})
