// Windows Terminal: start a session in a real tab, bring a session's tab to the front (ADR-0004).
// Windows-specific, kept in this one module.
import { execFile } from 'node:child_process'
import { existsSync, statSync } from 'node:fs'

export const MODELS = ['', 'opus', 'sonnet', 'haiku']
export const MODES = ['', 'default', 'acceptEdits', 'plan', 'auto']

export interface Start {
  cwd: string
  name: string
  model?: string
  mode?: string
  connect?: boolean // pass clearance's hooks and companion mod
}

// The `wt` arguments for a new tab running claude, or why the request is refused. The tab keeps its
// title (the session's name), so that it can be found again.
// ponytail: no first prompt, since `wt` splits its command line on ";"; send one as an instruction.
export function startArgs(s: Start, clearance: { settings: string; mod: string }): string[] | string {
  if (typeof s.cwd !== 'string' || !existsSync(s.cwd) || !statSync(s.cwd).isDirectory()) return 'That folder does not exist.'
  if (!/^[\w.-]{1,40}$/.test(s.name ?? '')) return 'A name is 1 to 40 letters, digits, dots, dashes or underscores.'
  if (!MODELS.includes(s.model ?? '')) return 'Unknown model.'
  if (!MODES.includes(s.mode ?? '')) return 'Unknown mode.'
  const args = [
    '-w', '0', 'new-tab', '--title', s.name, '--suppressApplicationTitle', '-d', s.cwd,
    'claude', '-n', s.name,
    ...(s.model ? ['--model', s.model] : []),
    ...(s.mode ? ['--permission-mode', s.mode] : []),
    ...(s.connect === false ? [] : ['--settings', clearance.settings, '--plugin-dir', clearance.mod]),
  ]
  return args.some((a) => a.includes(';')) ? 'A path with ";" cannot be passed to Windows Terminal.' : args
}

// What a running Claude Code session sets for the processes it starts. A new tab inherits the
// environment of whoever called `wt`: with these, clearance started from inside a session would open
// child sessions (no transcript, not listed by `claude agents`) holding that session's messaging token.
const SESSION_MARKERS = new Set([
  'CLAUDECODE', 'CLAUDE_CODE_CHILD_SESSION', 'CLAUDE_CODE_ENTRYPOINT', 'CLAUDE_CODE_EXECPATH', 'CLAUDE_CODE_MESSAGING_SOCKET',
  'CLAUDE_CODE_MESSAGING_TOKEN', 'CLAUDE_CODE_SESSION_ATTENDED', 'CLAUDE_CODE_SESSION_ID', 'CLAUDE_EFFORT', 'CLAUDE_PID',
])
export const tabEnv = (env: NodeJS.ProcessEnv = process.env) => Object.fromEntries(Object.entries(env).filter(([k]) => !SESSION_MARKERS.has(k)))

export function start(args: string[]): Promise<string | null> {
  return new Promise((resolve) => execFile('wt.exe', args, { windowsHide: true, timeout: 15_000, env: tabEnv() }, (err) => resolve(err ? err.message : null)))
}

// UI Automation through Windows PowerShell: every tab of every Windows Terminal window, its name
// without the status glyph Claude Code puts in front. The titles come in through the environment,
// never into the script text. Inline, no file, no execution policy change.
const FOCUS = String.raw`
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$titles = $env:CLEARANCE_TITLES -split [char]10
$A = [System.Windows.Automation.AutomationElement]
$scope = [System.Windows.Automation.TreeScope]
$windows = New-Object System.Windows.Automation.PropertyCondition($A::ClassNameProperty, 'CASCADIA_HOSTING_WINDOW_CLASS')
$tabs = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
foreach ($w in $A::RootElement.FindAll($scope::Children, $windows)) {
  foreach ($t in $w.FindAll($scope::Descendants, $tabs)) {
    $name = ($t.Current.Name -replace '^[^\p{L}\p{N}]+', '').Trim()
    if ($titles -contains $name) {
      $t.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
      try { $w.SetFocus() } catch {}
      'found'
      exit 0
    }
  }
}
'not found'
`

// One title per line: Windows PowerShell 5.1's ConvertFrom-Json would not unroll a JSON array.
export function focus(titles: string[]): Promise<boolean> {
  const env = { ...process.env, CLEARANCE_TITLES: titles.map((t) => t.replace(/[\r\n]+/g, ' ')).join('\n') }
  return new Promise((resolve) =>
    execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', FOCUS], { windowsHide: true, timeout: 15_000, env }, (err, out) =>
      resolve(!err && String(out).trim() === 'found'),
    ),
  )
}
