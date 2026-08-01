---
name: spec
description: Author and maintain blkit's module-aligned living specs in specs/. Use when creating or updating a spec, before changing the behaviour or public API of any module that has a spec, when marking a spec superseded, or when checking spec/code synchronization.
---

# blkit spec workflow

The format is defined in
[specs/project/spec-format.spec.md](../../../specs/project/spec-format.spec.md) —
read it first. Specs are living documents aligned to code modules, not one-off
feature documents. This skill covers the workflow around them.

## Finding the spec for code (and vice versa)

- Grep the frontmatter `code:` entries under `specs/` for the package or
  directory you are touching (paths are repo-root-relative).
- Every Go package or module is covered by exactly one spec. Backends under
  `brokers/<name>/` and `stores/<name>/` map 1:1 to `specs/message-brokers/`
  and `specs/state-stores/`.
- A spec's role is inferred, not declared: `<family>/overview.spec.md` holds the
  family's contract; a spec with `implements:` is a backend spec; `specs/project/*`
  are project conventions; everything else is a module spec.
- If no spec covers the code you are changing, create one before implementing.

## Before changing code

1. Read the covering spec AND anything it `implements:` or links as normative.
2. If the spec's `status` is `superseded`, follow `superseded-by:` — never
   implement against a superseded spec. If it is `draft`, the spec is not yet
   authoritative — confirm direction with the stakeholder before building on it.
3. If the change alters behaviour, public API, or a design decision: update the
   spec first (or in the same change), then implement. The spec is the
   requirement; the code follows it.

## After changing code

- Sync the spec in the same commit/PR (commit type `spec` per COMMITS.md when
  the change is spec-only).
- New behaviour discovered during implementation goes into Behaviour / Edge Cases.
- Update Verification links when tests are added or moved.
- If the code now matches an `agreed` spec, flip `status: implemented`.

## Creating a spec

1. Copy [template.spec.md](template.spec.md); the format spec's section matrix
   says which sections the spec's role requires — delete the rest.
2. For a backend spec: set `implements:`, answer any section list the contract
   spec prescribes, and do NOT restate the contract's normative behaviour —
   document only the mapping onto the backend's primitives and any divergences.
3. Wire cross-links both ways: the family overview lists the new spec; the new
   spec links its contract and siblings.
4. New spec, no code yet → `status: agreed` (or `draft` while still being
   shaped), with `code:` naming the intended location.

## Superseding a spec

1. Set `status: superseded` and `superseded-by:` on the old spec.
2. Move any still-authoritative content into the successor **before** flagging —
   a superseded spec must not be the only home of a live fact.
3. Update inbound links (grep `specs/` for the filename).

## Validation checklist (run before finishing any spec change)

- [ ] Frontmatter complete; `status` is a valid value.
- [ ] Every `code:` path resolves from the repo root (`ls` it) — unless
      `status: draft` or `agreed` and the code intentionally doesn't exist yet.
- [ ] Every relative link resolves to a real file/anchor.
- [ ] Required sections for the spec's role are present, in canonical order.
- [ ] No normative statement duplicated from another spec — link instead.
- [ ] Go-idiomatic language only: `(T, error)`, `nil` — no `raises`, no `None`.
- [ ] `status` matches reality (implemented code ⇒ not `draft`; missing code ⇒
      not `implemented`).
