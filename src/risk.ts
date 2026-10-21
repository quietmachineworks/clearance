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
