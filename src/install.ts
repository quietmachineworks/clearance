// Installs clearance's hooks in the user's Claude Code settings, or removes them. An explicit step
// (BRIEF section 9): a backup first, the user's own hooks left as they are, and only entries that point
// at clearance are ever replaced or removed.
//   node src/install.ts [--dry-run] [--remove] [--settings <file>]
// The companion mod installs apart: claude plugin marketplace add <this folder>, then
// claude plugin install clearance@clearance.
import { copyFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

const EVENTS = 'PermissionRequest|PermissionDenied|Stop|StopFailure|UserPromptSubmit|SubagentStart|SubagentStop'
const CLEARANCE_URL = new RegExp(String.raw`^http://127\.0\.0\.1:\d+/hook/(${EVENTS})$`)
const ours = (group: any) => (group?.hooks ?? []).some((h: any) => h.type === 'http' && CLEARANCE_URL.test(h.url ?? ''))

export function withoutHooks(settings: any) {
  const out = structuredClone(settings)
  for (const [event, groups] of Object.entries<any[]>(out.hooks ?? {})) {
    const kept = groups.filter((g) => !ours(g))
    if (kept.length) out.hooks[event] = kept
    else delete out.hooks[event]
  }
  if (out.hooks && Object.keys(out.hooks).length === 0) delete out.hooks
  return out
}

// A new install replaces the previous one (new token, new port), next to the user's own hooks.
export function withHooks(settings: any, hooks: Record<string, any[]>) {
  const out = withoutHooks(settings)
  out.hooks ??= {}
  for (const [event, groups] of Object.entries(hooks)) out.hooks[event] = [...(out.hooks[event] ?? []), ...groups]
  return out
}

// Which events gain or lose a clearance entry, for the summary; never the token.
const count = (settings: any) => Object.fromEntries(Object.entries<any[]>(settings.hooks ?? {}).map(([e, g]) => [e, g.filter(ours).length]).filter(([, n]) => n))

if (import.meta.main) {
  const args = process.argv.slice(2)
  const file = args.includes('--settings') ? args[args.indexOf('--settings') + 1] : join(homedir(), '.claude', 'settings.json')
  const remove = args.includes('--remove')
  const hooksFile = new URL('../data/hooks.json', import.meta.url)
  if (!remove && !existsSync(hooksFile)) {
    console.error('Start clearance once first: node src/server.ts writes data/hooks.json.')
    process.exit(1)
  }
  const settings = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : {}
  const next = remove ? withoutHooks(settings) : withHooks(settings, JSON.parse(readFileSync(hooksFile, 'utf8')).hooks)
  console.log(`settings: ${file}`)
  console.log(`clearance hooks before: ${JSON.stringify(count(settings))}`)
  console.log(`clearance hooks after:  ${JSON.stringify(count(next))}`)
  console.log(`other hooks kept as they are: ${Object.entries<any[]>(settings.hooks ?? {}).reduce((n, [, g]) => n + g.filter((x) => !ours(x)).length, 0)}`)
  if (args.includes('--dry-run')) process.exit(0)
  if (existsSync(file)) {
    const backup = `${file}.bak-clearance-${new Date().toISOString().replace(/[:.]/g, '-')}`
    copyFileSync(file, backup)
    console.log(`backup: ${backup}`)
  }
  writeFileSync(file, JSON.stringify(next, null, 2) + '\n')
  console.log(remove ? 'clearance hooks removed.' : 'clearance hooks installed; sessions started from now on use them.')
}
