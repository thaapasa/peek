---
name: checkup
description: Periodic architectural review — critically re-examines existing design choices, hunts duplication, flags refactor candidates, checks conventions
disable-model-invocation: true
context: fork
agent: general-purpose
---

# Codebase Review

A critical architectural review of the peek codebase. The goal is **not**
to confirm the code follows its own conventions — it usually does. The
goal is to find where the architecture itself has drifted, where past
decisions no longer fit, and where the code has grown duplication or
debt that the conventions don't catch.

Be skeptical. A review that concludes "all good" has almost always
failed to look hard enough at the seams. Conformance to conventions is
the floor, not the finding.

## Step 1: Load context

Read these to understand the *intended* design — then judge the code
against it, and judge the design itself against reality:

- `docs/architecture.md` — design principles, data flow, key abstractions
- `docs/conventions.md` — coding rules and patterns
- `CLAUDE.md` — file map and the three north stars (clean architecture,
  stream don't load, low cognitive load)
- `git log --oneline -30` — recent direction; what's been churning

The three north stars are the lens. But also question them: a north
star can be violated, and a past decision that honoured one can fall out
of step as the codebase grows.

## Step 2: Challenge the architecture

This is the core of the review. For each significant abstraction —
trait, central dispatch table, shared manager, "shared" helper module,
per-type module shape — ask hard questions:

- **Does it still earn its place?** An abstraction justifies itself by
  reducing total surface area or making extension easier. A trait with
  one implementor, a generic helper called from one site, a layer that
  only forwards — these are indirection without payoff.
- **Has it been outgrown?** A module that started with one narrow
  responsibility and accreted unrelated helpers is now a grab-bag.
  A `match file_type` chain re-growing in a place `compose_modes` was
  meant to eliminate. A parameter list that crossed into "should be a
  struct" three params ago.
- **Is it leaking?** Callers that have to know the mechanism behind an
  abstraction (ordering constraints, init sequencing, hidden coupling)
  mean the abstraction failed at its job.
- **Did reality move?** A choice that fit when files were small but
  conflicts with "stream don't load" now that multi-GB inputs are
  first-class. Eager work that should be lazy. A cache or buffer sized
  for assumptions that no longer hold.
- **Best-practice drift:** `unwrap()` / `expect()` / `panic!` on
  non-test paths, blocking whole-file reads where streaming is the norm,
  allocation in render hot paths, error handling that's `Result` on one
  path and swallowed on its twin.

Name the trade-off concretely. "This was reasonable when X; now Y, so it
costs Z." Propose a direction, even if rough.

## Step 3: Hunt duplication

Actively search for repetition the conventions don't flag:

- **Same logic in 2+ places** — exact copies, or near-copies that
  drifted slightly (the dangerous kind: they'll diverge as bugs).
- **Parallel structures** — per-type modules that copy-paste the same
  shape. Colocation is intentional in this codebase, so distinguish
  "healthy parallel structure" from "this shape should be a shared
  helper / trait / macro and isn't".
- **Boilerplate** that a helper, trait default method, or macro would
  erase.

For each cluster: list **every** site, then either propose the unifying
abstraction *or* argue why the duplication should stay. The project
values low cognitive load over dogmatic DRY — three short copies a
reader can hold in their head can beat one abstraction they must chase
across four files. Make that call explicitly; don't just demand DRY.

## Step 4: Convention & correctness sweep

The mechanical checks — quicker, lower-value, but still worth a pass:

- Colored output bypassing `PeekTheme::paint()` (raw ANSI outside the
  `ui.rs` status-line composition)
- File-type handling bypassing `Registry` / `compose_modes` dispatch
- Functions long enough to obscure their own structure
- Dead code, unused imports, stale comments, public items that should be
  `pub(crate)` or private
- New file types missing info-screen metadata, or interactive viewers
  missing standard key bindings
- `docs/features.md` status out of date vs. the actual code

## Step 5: Report

Lead with **what's most worth changing** — the 3-5 highest-leverage
findings, architecture and duplication first. Then the rest, grouped:

- **High** — bugs, correctness issues, architectural violations
- **Medium** — architectural debt, duplication, abstractions that no
  longer earn their place, refactor candidates
- **Low** — convention nits, minor cleanups

Each finding: file path + line, what's wrong, *why it matters now*, and a
concrete fix or direction. For refactor candidates, sketch the target
shape.

Rules:

- No praise. Don't restate what the code does well. The report is a list
  of things to change.
- Don't manufacture issues to hit a quota. But a non-trivial codebase
  always carries trade-offs worth naming — if a section is empty, it's
  more likely you under-looked than that the area is perfect. Surface
  the weakest parts and the trade-off each represents even when none is
  an outright bug.
- Omit a severity group only if it is genuinely empty after a real look.
