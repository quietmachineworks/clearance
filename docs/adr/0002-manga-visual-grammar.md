# ADR-0002 - Manga visual grammar

Date: 2026-10-20. Status: accepted.

## Context

Successive trials on the design canvas: dark radar, paper, tilted bright paper, aircraft liveries, an
airport theme. Feedback: the tilt is decoration without meaning, the airport theme does not speak to
the user. Explicit ask: something fun and animated, where every element means something.

## Decision

Manga style, original chibi avatars. Manga already has a code for emotions (sweat drop, anger vein,
sparkles, Zzz): we use it for state, so no legend is needed. Hair colour carries the project, the
accessory carries the model, subagents are clones. Full table: section 4 of the brief.

The session detail reuses the manga page: one row of panels per step, panels side by side for agents
run in parallel.

## Consequences

- Meaning layer and costume are separate: a pack (Halloween...) only touches the costume.
- Original drawings only, or CC0 assets; fonts under the SIL Open Font License. No character, name or
  visual from an existing work: the tool must be publishable without any rights issue.
