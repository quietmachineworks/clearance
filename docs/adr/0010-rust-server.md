# ADR-0010 - The server in Rust, standard library only

Date: 2026-10-22. Status: accepted. Replaces the Node 24 server of ADR-0001.

## Context

clearance must run on macOS as well as Windows, as one program to install. Measured on the Node server:
47 MB of memory and 0.3 % of one core over 22 hours; the visible cost is `claude agents --json`, about
480 ms per call, every 2 seconds. A Rust binary built on the maintainer's managed workstation, unsigned,
launched `claude`, `wt.exe` and an inline Windows PowerShell script without being blocked by the endpoint
security tool. The page and the companion mod stay in JavaScript: a browser and Claude Code run them.

## Decision

- **Rust, stable toolchain, edition 2024, no crate.** What a crate would give is written here, small and
  tested: an HTTP/1.1 subset for `127.0.0.1` (request line, headers, bodies with `Content-Length`,
  server-sent events), a JSON value that keeps key order (the user's `settings.json` is rewritten in
  place), the risk rules without a regex engine, ISO 8601 dates, and the operating system's random
  numbers (`BCryptGenRandom` on Windows, `/dev/urandom` elsewhere). A crate still needs an ADR.
- **One binary**: `clearance` serves the page and the hooks; `clearance install [--dry-run] [--remove]
  [--settings <file>]` replaces `src/install.ts`.
- **Data in `~/.clearance/`** (or `CLEARANCE_HOME`): token, `hooks.json`, `endpoint.json`, journal,
  preferences. The token and journal of the Node layout (`data/`) are copied there once, so that running
  sessions keep working.
- **The page is built into the binary**, and read from the repository instead while that file exists, so
  that editing it needs no rebuild.
- **One thread per connection.** A held hook call waits on a channel and checks every second whether
  Claude Code closed it.
- **`claude agents` every 2 s while a page is open, every 10 s otherwise.**
- **macOS**: Terminal.app through `osascript`; every value goes in as an AppleScript argument, never into
  the script text.
- The Node version stays in the repository until the Rust one does everything it does, then goes.

## Consequences

More code of our own (JSON, HTTP), each part with its tests (`cargo test`). A build step (`cargo build
--release`), and binaries to produce for each system.
