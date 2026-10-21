import { test } from 'node:test'
import assert from 'node:assert/strict'
import { Board, respond } from '../src/board.ts'

const S = 'sess-orchard'
const permission = (command: string, extra = {}) => ({ session_id: S, tool_name: 'Bash', tool_input: { command }, permission_mode: 'default', ...extra })
const ask = (labels: string[]) => ({
  session_id: S,
  tool_name: 'AskUserQuestion',
  tool_input: { questions: [{ question: 'Ship the plan?', header: 'Plan', multiSelect: false, options: labels.map((label) => ({ label })) }] },
})

test('requests are sorted by risk, then age', () => {
  const b = new Board()
  b.hook('PermissionRequest', permission('npm test'), 1)
  b.hook('PermissionRequest', permission('ls'), 2)
  b.hook('PermissionRequest', permission('git push origin main'), 3)
  b.hook('PermissionRequest', permission('npm run build'), 4)
  assert.deepEqual(b.snapshot().requests.map((r) => r.input.command), ['git push origin main', 'npm test', 'npm run build', 'ls'])
})

test('a request leaves once its session stops waiting, not before it was seen waiting', () => {
  const b = new Board()
  const { request } = b.hook('PermissionRequest', permission('npm test'))
  assert.deepEqual(b.sync([{ sessionId: S, status: 'busy' }]), []) // the hook arrives before the poll shows it
  assert.deepEqual(b.sync([{ sessionId: S, status: 'waiting', waitingFor: 'permission prompt' }]), [])
  assert.deepEqual(b.sync([{ sessionId: S, status: 'busy' }]), [request!.id]) // answered in the terminal
  assert.equal(b.requests.size, 0)
})

test('Stop ends the session requests and marks it done until read or a new prompt', () => {
  const b = new Board()
  const { request } = b.hook('PermissionRequest', permission('npm test'))
  assert.deepEqual(b.hook('Stop', { session_id: S, last_assistant_message: 'All green.' }).ended, [request!.id])
  assert.equal(b.done.get(S)?.message, 'All green.')
  b.hook('UserPromptSubmit', { session_id: S, prompt: 'next' })
  assert.equal(b.done.size, 0)
  b.hook('Stop', { session_id: S, last_assistant_message: 'Done again.' })
  b.read(S)
  assert.equal(b.done.size, 0)
})

test('subagents are counted, internal ones ignored', () => {
  const b = new Board()
  b.hook('SubagentStart', { session_id: S, agent_id: 'a1', agent_type: 'reviewer' })
  b.hook('SubagentStart', { session_id: S, agent_id: 'a2', agent_type: '' })
  b.sync([{ sessionId: S, status: 'busy' }])
  assert.equal(b.snapshot().sessions[0].agents, 1)
  b.hook('SubagentStop', { session_id: S, agent_id: 'a1' })
  assert.equal(b.snapshot().sessions[0].agents, 0)
})

test('permission answers', () => {
  const b = new Board()
  const suggestion = { type: 'addRules', rules: [{ toolName: 'Bash', ruleContent: 'npm test:*' }], behavior: 'allow', destination: 'localSettings' }
  const r = b.hook('PermissionRequest', permission('npm test', { permission_suggestions: [suggestion] })).request!
  assert.deepEqual(respond(r, { id: r.id, action: 'allow' }), { hookSpecificOutput: { hookEventName: 'PermissionRequest', decision: { behavior: 'allow' } } })
  assert.deepEqual((respond(r, { id: r.id, action: 'rule', mode: 'acceptEdits' }) as any).hookSpecificOutput.decision.updatedPermissions, [
    suggestion,
    { type: 'setMode', mode: 'acceptEdits', destination: 'session' },
  ])
  assert.equal((respond(r, { id: r.id, action: 'deny', reason: 'use the test script' }) as any).hookSpecificOutput.decision.message, 'use the test script')
  assert.deepEqual(respond(r, { id: r.id, action: 'dismiss' }), {})
})

test('an infrastructure write needs GO typed and never becomes a rule', () => {
  const b = new Board()
  const r = b.hook('PermissionRequest', permission('git push --force', { permission_suggestions: [{ type: 'addRules' }] })).request!
  assert.throws(() => respond(r, { id: r.id, action: 'rule', typed: 'GO' }), /no rule/)
  assert.throws(() => respond(r, { id: r.id, action: 'allow' }), /type GO/)
  assert.equal((respond(r, { id: r.id, action: 'allow', typed: 'GO' }) as any).hookSpecificOutput.decision.behavior, 'allow')
  assert.doesNotThrow(() => respond(r, { id: r.id, action: 'deny' }))
})

test('questions take option labels, a free answer goes back as a deny message', () => {
  const b = new Board()
  const r = b.hook('PermissionRequest', ask(['Red', 'Blue'])).request!
  assert.equal(r.kind, 'question')
  const allow = respond(r, { id: r.id, action: 'answer', answers: { 'Ship the plan?': 'Blue' } }) as any
  assert.equal(allow.hookSpecificOutput.decision.behavior, 'allow')
  assert.deepEqual(allow.hookSpecificOutput.decision.updatedInput.answers, { 'Ship the plan?': 'Blue' })
  assert.throws(() => respond(r, { id: r.id, action: 'answer', answers: { 'Ship the plan?': 'Green' } }), /no valid answer/)
  const free = respond(r, { id: r.id, action: 'answer', text: 'Green, please' }) as any
  assert.equal(free.hookSpecificOutput.decision.behavior, 'deny')
  assert.match(free.hookSpecificOutput.decision.message, /Green, please/)
})

test('a plan needs GO typed', () => {
  const b = new Board()
  const r = b.hook('PermissionRequest', ask(['GO', 'No'])).request!
  assert.equal(r.kind, 'plan')
  assert.equal(b.snapshot().requests[0].risk, 'infra')
  const go = { id: r.id, action: 'answer' as const, answers: { 'Ship the plan?': 'GO' } }
  assert.throws(() => respond(r, go), /type GO/)
  assert.throws(() => respond(r, { ...go, typed: 'go' }), /type GO/)
  assert.equal((respond(r, { ...go, typed: 'GO' }) as any).hookSpecificOutput.decision.behavior, 'allow')
  assert.doesNotThrow(() => respond(r, { id: r.id, action: 'answer', answers: { 'Ship the plan?': 'No' } }))
})

test('auto mode denials can be retried', () => {
  const b = new Board()
  const r = b.hook('PermissionDenied', { session_id: S, tool_name: 'Bash', tool_input: { command: 'git push --force' }, reason: '[Git Destructive]' }).request!
  assert.equal(r.reason, '[Git Destructive]')
  assert.deepEqual(respond(r, { id: r.id, action: 'retry' }), { hookSpecificOutput: { hookEventName: 'PermissionDenied', retry: true } })
  assert.throws(() => respond(r, { id: r.id, action: 'allow' }))
})

test('companion mod: usage per session, freshest quota, instructions delivered once', () => {
  const b = new Board()
  b.sync([{ sessionId: S, status: 'busy' }, { sessionId: 'sess-quarry', status: 'idle' }])
  b.report(S, { context: { window: 200_000, percent: 91 }, rateLimits: [{ kind: 'five_hour', percentUsed: 40 }], cost: { usd: 1.5 } }, 100)
  b.report('sess-quarry', { context: { window: 200_000, percent: 10 }, rateLimits: [{ kind: 'five_hour', percentUsed: 42 }] }, 200)
  const snap = b.snapshot()
  assert.equal(snap.sessions.find((s) => s.sessionId === S)?.usage?.context?.percent, 91)
  assert.deepEqual(snap.quota, [{ kind: 'five_hour', percentUsed: 42 }])
  assert.equal(b.take(S), undefined)
  b.instruct(S, 'Run the linter too.', 300)
  assert.equal(b.take(S, 310), 'Run the linter too.')
  assert.equal(b.take(S, 320), undefined)
  assert.equal(b.snapshot().sessions.find((s) => s.sessionId === S)?.instruction?.delivered, 310)
  b.sync([{ sessionId: 'sess-quarry', status: 'idle' }]) // the session ended
  assert.equal(b.usage.has(S), false)
  assert.equal(b.inbox.has(S), false)
})

test('a turn ending or starting is read off the session status', () => {
  const b = new Board()
  b.sync([{ sessionId: S, status: 'idle' }], 1) // first sight: nothing to say yet
  assert.equal(b.done.size, 0)
  b.sync([{ sessionId: S, status: 'busy' }], 2)
  const { request } = b.hook('PermissionRequest', permission('npm test'))
  const ended = b.sync([{ sessionId: S, status: 'idle', lastText: 'All green.' }], 3)
  assert.deepEqual(ended, [request!.id])
  assert.deepEqual(b.done.get(S), { session: S, cwd: undefined, at: 3, message: 'All green.' })
  assert.equal('lastText' in b.snapshot().sessions[0], false) // not sent to the page with every update
  b.sync([{ sessionId: S, status: 'busy' }], 4) // the user typed something
  assert.equal(b.done.size, 0)
})
