---
name: phased-implement
description: Coordinator workflow for multi-phase implementation across workmux worktrees. Generates or loads a master plan, dispatches phase agents using presets, verifies sentinels, merges serially, and performs integration verification.
allowed-tools: Bash, Read, Write, Glob, Grep
---

Coordinate multi-phase implementation across workmux worktrees. The coordinator owns the master plan, phase dispatch, merge order, ancestry checks, final integration validation, and final summary. The coordinator does not edit source files for feature work.

Do not use Claude Code built-in worktree features. Use `workmux` for worktree orchestration.

## Argument handling

Arguments are `$ARGUMENTS`.

Parse these flags before starting:

- `--plan <path>`: load an existing master plan instead of generating one.
- `--integration-branch <branch>`: branch that receives phase merges. Default: current branch.
- `--preset light|standard|design|strict`: default preset for generated phases.
- `--validation <command>`: final integration validation command.
- `--reviewer <selector>`: consult-llm selector for optional integration review. Repeatable.
- `--reviewers <selector,selector>`: comma-separated reviewer selectors.
- `--integration-review none|auto|full`: final integration review policy. Default: `auto`.

Everything else is the requested multi-phase implementation.

Preset compilation:

```yaml
light:
  planning: note
  plan_review: none
  executor: self
  verification: light
standard:
  planning: rich
  plan_review: narrow
  executor: sideagent
  verification: light
design:
  planning: consult-first
  plan_review: full
  executor: sideagent
  verification: light
strict:
  planning: rich
  plan_review: full
  executor: sideagent
  verification: full
```

Advanced master plans may override compiled fields per phase. Apply overrides after resolving the phase preset.

## Required artifacts

Write coordinator artifacts under `history/` using today's date prefix.

Required:

- master phased plan
- per-phase plan or note, written by each phase through `/implement`
- per-phase result sentinel
- final summary

Optional:

- phase prompt captures
- external review capture
- debug notes

Do not require ADRs or feedback ledgers.

## Phase A: snapshot and plan

1. Record the integration branch and start commit:

   ```bash
   git branch --show-current
   git rev-parse HEAD
   git status --short
   ```

2. Stop if unrelated uncommitted changes make orchestration unsafe.

3. If `--plan <path>` is provided, read the plan and validate it.

4. If no plan is provided, generate a master phased plan under `history/`. Gather source context first with Glob, Grep, and Read. The master plan should be scheduling-focused, not code-heavy. Assign phase presets by risk:

   - `light` for routine mechanical phases from a clear master plan
   - `standard` for non-trivial phases with known approach
   - `design` for uncertain, cross-module, or approach-heavy phases
   - `strict` for public contracts, data formats, security-sensitive work, migrations, or high regression risk

Generated plans should mark non-trivial phases as at least `standard`.

## Master plan schema

The master plan must contain one YAML block and one brief per phase.

````markdown
# <Feature> Master Plan

**Goal:** <one sentence>
**Integration branch:** <branch>
**Start commit:** <sha>
**Final validation:** `<command>`

```yaml
phases:
  - id: api-contract
    description: Define the public API contract for X.
    depends_on: []
    paths:
      - "src/api/**"
      - "tests/api/**"
    preset: design
    acceptance:
      - "Given a valid request, when the API is called, then it returns the new response shape."
    planning: consult-first
    plan_review: full
    executor: sideagent
    verification: light
```

## Phase briefs

### api-contract

**Intent:** why this phase exists
**Current problem:** what is wrong now
**Desired shape:** target behavior and boundaries
**Preserve:** behavior that must not change
**Avoid:** overreach and later-phase ownership
**Dependencies:** previous phase outputs
````

Required YAML fields per phase:

- `id`: stable slug, unique across phases
- `description`: one sentence
- `depends_on`: list of phase ids
- `paths`: expected owned paths or globs
- `preset`: `light`, `standard`, `design`, or `strict`
- `acceptance`: list of acceptance criteria

Optional compiled-field overrides:

- `planning`: `note`, `rich`, or `consult-first`
- `plan_review`: `none`, `narrow`, or `full`
- `executor`: `self` or `sideagent`
- `verification`: `light` or `full`
- `validation`: phase-specific validation command

## DAG validation

Before dispatch, validate the plan:

- Every phase id is unique.
- Every dependency names an existing phase.
- The graph has no cycles.
- Every phase has a preset.
- Every preset resolves to compiled fields.
- Overrides are valid enum values.
- Every phase has acceptance criteria.
- Phase briefs exist for every phase.
- Phase path ownership is specific enough to detect obvious overlap.
- Overlapping paths are allowed only when dependencies serialize those phases or the brief explains the boundary.
- A final validation command exists or can be inferred.

If validation fails, update the master plan directly and rerun validation.

## Phase B: dispatch loop

Dispatch phases whose dependencies have succeeded and merged. Parallel phases may run at the same time when their dependencies are satisfied and path ownership is safe.

For each ready phase:

1. Resolve preset and compiled fields.
2. Write a phase prompt under `history/`.
3. Create a workmux worktree with a descriptive phase name.
4. Send the prompt to the worktree agent.
5. Wait for completion.
6. Capture status and output.
7. Read and verify the result sentinel.
8. Merge successful phases serially into the integration branch.

Use workmux at a high level like this:

```bash
workmux add <phase-id>
workmux status
workmux wait <phase-id>
workmux capture <phase-id>
```

Use the exact local workmux command syntax when running commands in this repo. Do not switch to Claude Code worktrees.

## Phase prompt template

Write one prompt file per phase:

```markdown
# Phase Agent Prompt: <phase-id>

You are implementing one phase in a workmux worktree.

## Hard rules

- Work only on this phase.
- Do not modify the master plan.
- Do not use plan mode.
- Do not use em dashes.
- Do not overwrite user changes.
- Invoke `/implement` for this phase with the resolved preset and phase context.
- Preserve the phase boundaries, acceptance criteria, and dependencies below.
- Commit successful changes in the phase worktree.
- Write the result sentinel exactly as requested.

## Invoke

Run this implementation workflow:

`/implement --preset <preset> --planning <planning> --plan-review <plan_review> --executor <executor> --verification <verification> --parent-plan <master-plan-path> --validation '<phase-validation-command>' <phase description and acceptance>`

If the local command interface does not accept compiled-field flags, include the compiled fields in the implementation request and keep the preset as the primary interface.

## Phase context

- Phase id: `<phase-id>`
- Description: <description>
- Paths: <paths>
- Preset: `<preset>`
- Compiled fields: planning=<planning>, plan_review=<plan_review>, executor=<executor>, verification=<verification>
- Master plan: `<path>`
- Dependencies: <dependencies>

## Acceptance criteria

- <criterion>

## Phase brief

<brief from master plan>

## Result sentinel

Write `history/<date>-<phase-id>-result.md`:

```markdown
# Phase Result: <phase-id>

status: success | blocked | failed
phase_id: <phase-id>
preset: light | standard | design | strict
planning: note | rich | consult-first
plan_review: none | narrow | full
executor: self | sideagent
verification: light | full
worktree: <workmux worktree name>
base_commit: <sha>
head_commit: <sha>
commit: <sha>
plan_or_note: <path>
validation: <command>
validation_status: passed | failed | skipped

## Summary

- <what changed>

## Acceptance

- <criterion>: met | not met | unknown

## Files changed

- <path>

## Blockers

- <blocker or none>
```
```

The phase agent should treat a missing sentinel as failure.

## Phase C: result verification

For each completed phase, the coordinator verifies:

- The result sentinel exists.
- `status` is `success`.
- `head_commit` and `commit` are present.
- The phase committed its work.
- Validation passed or any skipped validation is justified.
- Files changed are within phase scope or explained by the brief.
- Acceptance criteria are marked met or have clear explanation.
- Blockers are `none`.

If verification fails, do not merge that phase. Capture the workmux output and summarize the blocker.

## Phase D: merge workflow

Merge successful phases serially into the integration branch. Before each merge, ensure the integration branch is checked out and current.

Use the existing merge skill in the phase worktree:

```text
/merge --keep
```

At a high level:

1. Capture the phase tip from the sentinel before merge: `head_commit`.
2. Send `/merge --keep` to the phase worktree agent with workmux.
3. Wait for merge completion.
4. Check the integration branch history.
5. Verify the phase tip is an ancestor of the integration branch:

   ```bash
   git merge-base --is-ancestor <phase-head-commit> <integration-branch>
   ```

6. If ancestry verification passes, remove the workmux worktree:

   ```bash
   workmux remove <phase-id>
   ```

7. If ancestry verification fails, stop merging and report the phase as blocked.

Do not use `workmux remove` before the merge and ancestry verification succeed.

## Phase E: continue the DAG

After each successful merge:

- Mark the phase as merged.
- Recompute ready phases.
- Dispatch newly unblocked phases.
- Continue until every phase is merged or no progress can be made.

If a phase fails, keep unrelated ready phases running only when their dependencies and paths are unaffected. Otherwise stop and summarize.

## Phase F: final integration validation

When all phases merge:

1. Run the final integration validation command from the master plan or `--validation`.
2. Inspect the integration diff and phase summaries.
3. Confirm that acceptance criteria across phases are covered.
4. Confirm there is no obvious cross-phase drift or conflict.

Final integration review policy:

- `none`: skip external integration review.
- `auto`: run full external integration review when any phase used `design` or `strict`, or when multiple phases changed shared contracts. Otherwise skip.
- `full`: always run full external integration review.

For final integration review, load the `consult-llm` skill before calling consult-llm. Attach the master plan, phase sentinels, and relevant diffs or files. Use `--task review`, supplied reviewer selectors if present, quoted heredoc terminator `__CONSULT_LLM_END__`, and Bash timeout `600000`.

Prompt shape:

```text
Review the integrated result of this multi-phase implementation.

Check whether the merged phases satisfy the master plan acceptance criteria. Focus on cross-phase compatibility, public contracts, regression risk, edge cases, validation adequacy, and security where relevant.

Return only actionable must-fix or should-fix issues. If the result is acceptable, say so directly.
```

Only auto-fix localized must-fix issues with one clear answer. If review finds design or scope issues, stop and summarize them.

## Final summary

Write a final summary under `history/` and print it:

```markdown
# Phased Implementation Summary: <feature>

status: success | blocked | failed
integration_branch: <branch>
start_commit: <sha>
end_commit: <sha>
master_plan: <path>
final_validation: <command>
final_validation_status: passed | failed | skipped
integration_review: none | skipped | passed | failed

## Phases

| Phase | Preset | Status | Commit | Sentinel |
| --- | --- | --- | --- | --- |
| <id> | <preset> | merged | <sha> | <path> |

## Acceptance

- <criterion>: met | not met | unknown

## Merges

- <phase>: merged and ancestry verified against `<phase-head-commit>`

## Blockers

- <blocker or none>

## Next steps

- <only if needed>
```

Report the final validation result, integration review result, merged phase commits, and blockers. If all phases merged and validation passed, state success plainly.
