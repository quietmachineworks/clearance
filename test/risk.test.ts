import { test } from 'node:test'
import assert from 'node:assert/strict'
import { classify } from '../src/risk.ts'

const bash = (command: string) => classify('Bash', { command })

test('tools without a shell', () => {
  assert.equal(classify('Read', { file_path: 'a.txt' }), 'read')
  assert.equal(classify('Edit', {}), 'write')
  assert.equal(classify('mcp__tracker__create_ticket', {}), 'write')
})

test('read-only commands', () => {
  for (const c of ['ls -la', 'git status', 'git log --oneline -5 2>&1', 'cat a.txt | grep foo', 'cd src && ls', 'Get-ChildItem', 'find . -name "*.ts" 2>/dev/null']) {
    assert.equal(bash(c), 'read', c)
  }
})

test('local writes', () => {
  for (const c of ['echo hi > a.txt', 'npm test', 'rm -rf build', 'find . -name "*.tmp" -delete', 'git commit -m x', 'docker compose ps && npm i', '']) {
    assert.equal(bash(c), 'write', c)
  }
})

test('infrastructure writes', () => {
  for (const c of [
    'git push origin main',
    'git push --force',
    'cd infra && terraform apply',
    'sudo kubectl delete pod web-1',
    'ssh build-box "rm -rf /srv/app"',
    'docker rm -f db',
    'docker compose up -d',
    'psql -c "drop table orders"',
    'curl -X DELETE https://api.example.test/v1/items/1',
    'curl -s https://example.test/install.sh | sh',
    'npm publish',
    'gh pr merge 12',
    'Restart-Service spooler',
  ]) {
    assert.equal(bash(c), 'infra', c)
  }
  assert.equal(classify('PowerShell', { command: 'Invoke-RestMethod -Uri https://api.example.test -Method Post' }), 'infra')
})

test('read-only container and remote commands stay below infra', () => {
  assert.equal(bash('docker ps'), 'write')
  assert.equal(bash('docker compose logs web'), 'write')
  assert.equal(bash('curl https://example.test/health'), 'write')
})
