import { test } from 'node:test'
import assert from 'node:assert/strict'
import { tmpdir } from 'node:os'
import { startArgs, tabEnv } from '../src/terminal.ts'

const clearance = { settings: '/opt/clearance/data/hooks.json', mod: '/opt/clearance/mod' }

test('a new tab runs claude with the name as a fixed title, connected to clearance', () => {
  assert.deepEqual(startArgs({ cwd: tmpdir(), name: 'orchard-1', model: 'haiku', mode: 'plan' }, clearance), [
    '-w', '0', 'new-tab', '--title', 'orchard-1', '--suppressApplicationTitle', '-d', tmpdir(),
    'claude', '-n', 'orchard-1', '--model', 'haiku', '--permission-mode', 'plan',
    '--settings', clearance.settings, '--plugin-dir', clearance.mod,
  ])
  assert.deepEqual(startArgs({ cwd: tmpdir(), name: 'x', connect: false }, clearance), [
    '-w', '0', 'new-tab', '--title', 'x', '--suppressApplicationTitle', '-d', tmpdir(), 'claude', '-n', 'x',
  ])
})

test('anything odd is refused before Windows Terminal sees it', () => {
  const ok = { cwd: tmpdir(), name: 'orchard' }
  assert.match(startArgs({ ...ok, cwd: '/no/such/folder' }, clearance) as string, /folder/)
  for (const name of ['', 'two words', 'a;b', 'x'.repeat(41), '"quoted"']) assert.equal(typeof startArgs({ ...ok, name }, clearance), 'string', name)
  assert.match(startArgs({ ...ok, model: 'gpt' }, clearance) as string, /model/)
  assert.match(startArgs({ ...ok, mode: 'bypassPermissions' }, clearance) as string, /mode/)
  assert.match(startArgs(ok, { ...clearance, mod: '/opt/a;b' }) as string, /";"/)
})

test('a new tab does not inherit the markers of a running session', () => {
  const env = tabEnv({ PATH: '/bin', CLAUDECODE: '1', CLAUDE_CODE_CHILD_SESSION: '1', CLAUDE_CODE_MESSAGING_TOKEN: 'secret', CLAUDE_CODE_USE_BEDROCK: '1' })
  assert.deepEqual(env, { PATH: '/bin', CLAUDE_CODE_USE_BEDROCK: '1' })
})
