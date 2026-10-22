# ADR-0012 - The environment of an infrastructure write

Date: 2026-10-22. Status: accepted. Builds on BRIEF section 6 (risk classification).

## Context

The risk rules tell that a command reaches another machine, not which one. The question that matters
before an "Allow" is whether it hits production. Nothing in a command says so reliably: it depends on
each team's naming.

## Decision

- The user declares patterns in `~/.clearance/environments.json`, for example
  `{ "prod": ["-prd", "db-prod", "--context prod"], "staging": ["-stg", "-n staging"] }`. clearance
  looks for them in the command and in the inputs of other tools; the first environment with a matching
  pattern wins, and the card says which pattern matched.
- **Nothing matched means unknown, handled like production**: `GO` to type, no "always allow" rule.
- A request that stays on the machine (not an infrastructure write) shows as local.
- The repository ships no pattern.
