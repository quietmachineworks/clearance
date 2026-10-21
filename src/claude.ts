// The one module that reads Claude Code itself: its formats may change between versions.
import { execFile } from 'node:child_process'
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
