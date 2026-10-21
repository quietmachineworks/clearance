// Risk of a permission request: what an "Allow" would let run (BRIEF section 6).
// ponytail: regex heuristics on the command text, not a shell parser. Errors lean towards the higher
// class, and the card always shows the full command whatever the class.
export type Risk = 'read' | 'write' | 'infra'

export const RANK: Record<Risk, number> = { read: 0, write: 1, infra: 2 }

const READ_TOOLS = new Set(['Read', 'Glob', 'Grep', 'WebFetch', 'WebSearch'])
const SHELL_TOOLS = new Set(['Bash', 'PowerShell'])

// Start of a command word anywhere in the line, so `sudo kubectl` or `time terraform` still match.
const W = String.raw`(?:^|[\s;&|(])`
// Reaches another machine, a service, a remote or a registry: never approved without the full
// command, never turned into a rule.
const INFRA = [
  new RegExp(W + String.raw`(ssh|scp|sftp|rsync|kubectl|helm|terraform|tofu|pulumi|ansible(-playbook)?|aws|az|gcloud|sqlcmd|psql|mysql|mongosh|redis-cli|systemctl|shutdown|reboot)(\s|$)`),
  new RegExp(W + String.raw`(docker|podman)\s+(?:compose\s+)?(?!(?:compose|ps|images|logs|inspect|version|info)\b)\S`),
  new RegExp(W + String.raw`git\s+push\b`),
  new RegExp(W + String.raw`(npm|pnpm|yarn)\s+publish\b`),
  new RegExp(W + String.raw`(gh|glab)\s+(pr\s+(create|merge|close)|mr\s+(create|merge|close)|release|repo\s+(create|delete|edit)|api\s.*-X\s*(POST|PUT|PATCH|DELETE))`, 'i'),
  new RegExp(W + String.raw`(curl|wget)\s.*(-X\s*(POST|PUT|PATCH|DELETE)|--request\s+(POST|PUT|PATCH|DELETE)|\s-d\s|--data)`, 'i'),
  /Invoke-(RestMethod|WebRequest)\b.*-Method\s+(Post|Put|Patch|Delete)/i,
  /\b(Restart|Stop|Remove)-(Service|Computer|VM)\b/i,
  /\|\s*(sh|bash|zsh|iex|Invoke-Expression)\b/i,
]

const READ_COMMAND = /^(ls|dir|cat|head|tail|less|grep|rg|find|pwd|cd|wc|which|where|echo|git\s+(status|log|diff|show|branch|remote|rev-parse)|Get-\w+)(\s|$)/i
// Redirections that write nothing: 2>&1, >/dev/null, 2>$null.
const QUIET = /\d?>&\d|\d?>\s*(\/dev\/null|\$null|NUL)\b/gi

// ---- suggested rules (BRIEF section 3: fewer interruptions) ----

export interface Approval {
  tool: string
  input: any
  at: string
  session: string
}

export interface Suggestion {
  rule: string // as written in permissions.allow
  count: number
  sessions: number
  last: string
  example: string
}

// A rule allows every command that starts with its prefix, not only those approved. So a prefix is
// offered only for programs whose second word is a subcommand, or for programs that only read.
// ponytail: two allowlists; anything else gets no suggestion rather than a broad one.
const SUBCOMMANDS = /^(git|npm|pnpm|yarn|npx|bun|deno|cargo|go|dotnet|mvn|gradle|make|docker|gh|glab|claude|pip|uv|poetry|winget|choco)$/
const READERS = /^(ls|dir|cat|head|tail|grep|rg|wc|pwd|which|where|tree|sort|uniq|diff|Get-\w+|Select-String|Test-Path|Measure-Object|Format-\w+)$/i
// Subcommands that run anything (`npm run`, `npx`-like `exec`, `gh api`): the next word must be named too.
const OPEN_ENDED = /^(run|exec|x|dlx|api)$/
// Subcommands that discard or rewrite local work: a rule for them is never offered.
const DESTRUCTIVE = /^(checkout|reset|clean|restore|rm|rebase|branch|stash|config|uninstall|remove|prune)$/
// An MCP tool that writes to the outside world, or runs code, gets no rule; names are split into
// words (`create_issue`, `addComment`) so that `job_runs` is not mistaken for `run`.
const MCP_WRITE = new Set(['create', 'delete', 'update', 'send', 'post', 'put', 'patch', 'set', 'add', 'remove', 'edit', 'write', 'transition', 'move', 'rename', 'upload', 'trash', 'modify', 'publish', 'deploy', 'execute', 'run', 'start', 'stop', 'restart', 'javascript', 'computer', 'click', 'type', 'press', 'form'])
const words = (name: string) => name.split(/_|-|(?=[A-Z])/).map((w) => w.toLowerCase())
const NO_RULE = new Set(['AskUserQuestion', 'ExitPlanMode', 'Workflow', 'Skill', 'Agent', 'Task'])

export function ruleOf(tool: string, input: any = {}): string | null {
  if (NO_RULE.has(tool) || classify(tool, input) === 'infra') return null
  if (tool.startsWith('mcp__')) return words(tool.split('__').slice(2).join('_')).some((w) => MCP_WRITE.has(w)) ? null : tool
  if (tool === 'WebFetch') {
    try {
      return `WebFetch(domain:${new URL(input.url).hostname})`
    } catch {
      return null
    }
  }
  if (!SHELL_TOOLS.has(tool)) return null // Read, Edit, Write: a rule by path is the user's call
  // A chain (`a && b; c`) was approved as a whole, not as its first command: it counts for nothing.
  const command = String(input.command ?? '').trim()
  if (/&&|\|\||;|\n/.test(command)) return null
  const [program, sub, third] = command.split('|')[0].trim().split(/\s+/)
  const word = (w?: string) => /^[a-z][\w-]*$/i.test(w ?? '')
  const prefix = !SUBCOMMANDS.test(program) ? (READERS.test(program) ? program : null)
    : !word(sub) || DESTRUCTIVE.test(sub) ? null
    : !OPEN_ENDED.test(sub) ? `${program} ${sub}`
    : word(third) ? `${program} ${sub} ${third}` : null
  if (!prefix || classify(tool, { command: prefix }) === 'infra') return null
  return `${tool}(${prefix}:*)`
}

// Approvals given by hand at least `min` times for the same rule, most frequent first.
export function suggest(approvals: Approval[], min = 3): Suggestion[] {
  const groups = new Map<string, Approval[]>()
  for (const a of approvals) {
    const rule = ruleOf(a.tool, a.input)
    if (rule) groups.set(rule, [...(groups.get(rule) ?? []), a])
  }
  return [...groups]
    .filter(([, list]) => list.length >= min)
    .map(([rule, list]) => {
      const last = list.reduce((x, y) => (y.at > x.at ? y : x))
      return { rule, count: list.length, sessions: new Set(list.map((a) => a.session)).size, last: last.at, example: String(last.input.command ?? last.input.url ?? last.tool).slice(0, 200) }
    })
    .sort((a, b) => b.count - a.count)
}

export function classify(tool: string, input: any = {}): Risk {
  if (READ_TOOLS.has(tool)) return 'read'
  if (!SHELL_TOOLS.has(tool)) return 'write'
  const cmd = String(input.command ?? '')
  if (INFRA.some((re) => re.test(cmd))) return 'infra'
  const quiet = cmd.replace(QUIET, '')
  const parts = quiet.split(/&&|\|\||[;|\n]/).map((s) => s.trim()).filter(Boolean)
  const reads = parts.length > 0 && !quiet.includes('>') && parts.every((p) => READ_COMMAND.test(p) && !/\s-(delete|exec)\b/.test(p))
  return reads ? 'read' : 'write'
}
