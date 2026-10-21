import { test } from 'node:test'
import assert from 'node:assert/strict'
import { withHooks, withoutHooks } from '../src/install.ts'

const theirs = {
  UserPromptSubmit: [{ hooks: [{ type: 'command', command: 'python stamp.py' }] }],
  PreToolUse: [{ matcher: 'Bash', hooks: [{ type: 'command', command: 'rewrite-bash' }] }],
  Stop: [{ hooks: [{ type: 'http', url: 'http://127.0.0.1:9000/notify' }] }], // someone else's local http hook
}
const settings = { model: 'opus', permissions: { defaultMode: 'auto' }, hooks: theirs }
const http = (event: string, port = 47800) => [{ hooks: [{ type: 'http', url: `http://127.0.0.1:${port}/hook/${event}`, headers: { Authorization: 'Bearer t' }, timeout: 5 }] }]
const ours = { PermissionRequest: http('PermissionRequest'), Stop: http('Stop'), UserPromptSubmit: http('UserPromptSubmit') }

test('install adds clearance next to the hooks already there', () => {
  const out = withHooks(settings, ours)
  assert.equal(out.model, 'opus')
  assert.deepEqual(out.hooks.PreToolUse, theirs.PreToolUse)
  assert.deepEqual(out.hooks.UserPromptSubmit, [...theirs.UserPromptSubmit, ...ours.UserPromptSubmit])
  assert.deepEqual(out.hooks.Stop, [...theirs.Stop, ...ours.Stop])
  assert.deepEqual(out.hooks.PermissionRequest, ours.PermissionRequest)
  assert.deepEqual(settings.hooks, theirs) // the input is left untouched
})

test('installing again replaces the previous clearance entries only', () => {
  const twice = withHooks(withHooks(settings, ours), { ...ours, Stop: http('Stop', 47900) })
  assert.equal(twice.hooks.Stop.length, 2)
  assert.equal(twice.hooks.Stop[1].hooks[0].url, 'http://127.0.0.1:47900/hook/Stop')
  assert.equal(twice.hooks.PermissionRequest.length, 1)
})

test('remove takes out clearance and nothing else', () => {
  assert.deepEqual(withoutHooks(withHooks(settings, ours)), settings)
  assert.deepEqual(withoutHooks({ hooks: ours }), {})
})
