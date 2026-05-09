# Architecture Decision Records

This directory captures the design decisions that shape `anvil`. Each ADR is a numbered, append-only document. Superseded ADRs are marked, not deleted — the trail of past decisions is the point.

## Format

```markdown
# <NNNN> — <Title>

**Status:** Accepted | Superseded by NNNN | Deprecated
**Date:** YYYY-MM-DD

## Context
What is the situation that calls for a decision?

## Decision
What did we choose?

## Consequences
What follows from this decision — good and bad?

## Alternatives considered
Briefly, what else did we look at, and why didn't we pick it?
```

## Index

| #    | Title                              | Status   |
| ---- | ---------------------------------- | -------- |
| 0001 | [Oxc vs SWC for TS parsing](./0001-oxc-vs-swc.md)            | Accepted |
| 0002 | [Stage-3 decorators only](./0002-stage3-decorators-only.md)  | Accepted |
| 0003 | [No TypeScript type checker](./0003-no-type-checker.md)      | Accepted |
| 0004 | [Per-component output file](./0004-per-component-output-file.md) | Accepted |

## When to write a new one

Write an ADR when a milestone makes a non-obvious architectural choice — particularly one that would be hard to reverse, or one a future contributor might second-guess. Don't write ADRs for routine implementation choices that are obvious from reading the code.
