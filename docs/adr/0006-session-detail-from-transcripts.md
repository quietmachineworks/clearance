# ADR-0006 - The session detail, read from transcripts

Date: 2026-10-21. Status: accepted.

## Context

M2: unfold a session down to its subagents and workflow, with errors, loops and changes. Exit criterion:
a multi-agent workflow followed end to end. Claude Code 2.1.304.

## Findings

- A session's files live in `~/.claude/projects/<cwd with every non-alphanumeric character as "-">/`:
  `<session>.jsonl` (the transcript), `<session>/subagents/agent-<id>.jsonl` and `.meta.json`,
  `<session>/subagents/workflows/<run>/journal.jsonl` with its agents, `<session>/workflows/<run>.json`.
- The transcript links a `Workflow` call to its run: the tool result record carries
  `toolUseResult.runId`. The call's input holds the script, or a `scriptPath`, whose `meta.phases` gives
  the declared phases while the run is still going.
- `<session>/workflows/<run>.json` appears when the run ends: `workflowName`, `status`, `startTime`,
  `durationMs`, `phases`.
- The journal records `launched`, `started` (`agentId`, `label`, `phase`) and `result`. An agent's
  `.meta.json` gives `agentType` and `workflowPhase`; its transcript gives the model, tool calls,
  errors (`tool_result.is_error`) and times.
- Replayed agents of a resumed run report the model `<synthetic>`.
- A `Workflow` call asks for permission like any tool: the card must show the whole script.

Followed live on a two-phase run (two parallel agents, then one): the page showed the first phase
current, the next one dashed, each agent with its tool calls and result, then the run closed.

## Decision

- Every read of these files stays in `src/claude.ts`. A transcript is parsed whole, on demand, and kept
  until its size changes; the board reads only the model, from the last 256 KB.
- The "diff" is the Edit, Write, MultiEdit and NotebookEdit calls of the session and its agents, from
  their transcripts. No `git` call: it works when a workflow changes another repository, and on
  sessions that are over.
- An agent is running when it has no result and its transcript moved in the last 10 minutes. A run with
  no record is running, or interrupted when its agents stopped without a result.
- Loops: the same tool with the same target failing at least three times in a row.
- The model's accessory (BRIEF section 4) comes from the transcript: glasses, headphones, cowlick on the
  manga pack; lenses, headphones, antenna on the robots pack.
- No "stop the workflow" action: there is no documented way to stop one run without stopping the whole
  session. Sending an instruction comes with the companion mod (M3), going to the terminal with M4.
