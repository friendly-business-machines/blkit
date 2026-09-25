# Agent workflow

- **OpenSpec** owns requirements and change artifacts. For non-trivial work,
  start with `openspec` (in Pi, `/opsx-propose`) and treat the approved change
  in `openspec/changes/` as the planning source of truth.
- **Superpowers** owns the engineering method: clarification, design review,
  TDD, and implementation discipline. Do not create a second competing plan
  when OpenSpec already has one; use its artifacts as the input.
- **pi-subagents** provides optional delegation. Delegate bounded research,
  implementation, or review tasks only when useful, and keep OpenSpec artifacts
  and final decisions in the parent session. Prefer a fresh reviewer after
  implementation.
- **Ponytail** owns implementation economy. After understanding the task,
  prefer no change, existing code, standard-library or native features,
  installed dependencies, and finally the smallest correct diff. It may
  challenge unapproved scope, but must not override approved OpenSpec
  requirements, required artifact/output contracts, or Superpowers' TDD,
  verification, safety, and review discipline.
- Do not use or reintroduce `pi-plan`; it has been removed. Avoid overlapping
  planning/task systems unless a tool is explicitly needed by the active
  workflow.
