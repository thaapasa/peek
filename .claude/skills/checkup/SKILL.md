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

**Precision over recall.** A short report of real issues beats a long
report padded with speculation. Every finding the user must dismiss as
"already considered" or "not actually a problem" is a tax on the
review's value. If you are not confident a finding is a real problem
*right now*, do not report it. Better to ship five solid findings than
twenty mixed ones.

Be skeptical of the code — and equally skeptical of your own findings
before they go in the report.

## Step 1: Load context

Read these to understand the *intended* design — then judge the code
against it, and judge the design itself against reality:

- `docs/architecture.md` — design principles, data flow, key abstractions
- `docs/conventions.md` — coding rules and patterns
- `CLAUDE.md` — file map and the three north stars (clean architecture,
  stream don't load, low cognitive load)
- `docs/checkup-findings.md` — open findings and **wontfix records** from
  prior rounds. Do not re-report anything recorded there: open items are
  already known, wontfix records are settled decisions. The records are
  not immunity, though — if the cited situation has changed (the code
  moved, the declining rationale no longer holds, a stated reopen trigger
  fired), it is your responsibility to revalidate and surface it as a
  *new* finding that says explicitly what changed since the record.
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

**Check existing rationale before reporting a dedup candidate.** If
the duplicated sites carry comments explaining *why* they are not
unified (e.g. "kept separate because X differs in subtle way Y", "tried
to merge in commit Z, reverted because…"), read them and judge whether
the rationale still holds against the current code. If it does, drop
the finding silently — do not report it just to make the user
re-litigate a settled decision. Only report it if the rationale is
clearly stale or wrong, and say *why* it's stale.

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

## Step 5: Self-validate before reporting

Before each candidate finding goes in the report, run it through this
filter. Drop anything that fails — do not pad the report with weak
items.

- **Read the surrounding context.** Open the file at the cited lines
  and read enough around them to understand what the code is doing and
  why. Many "issues" evaporate once the local context is clear.
- **Look for explanatory comments or doc blocks.** If a comment, doc
  comment, or commit message explains why the code is shaped this way
  (intentional duplication, deliberate non-abstraction, a workaround
  for a known constraint), assess whether the rationale still applies.
  If it does, drop the finding. If it doesn't, the finding must say
  *why* the rationale is stale.
- **Confirm the problem exists today.** "This could be a problem if X"
  is not a finding unless X is real. Speculative or hypothetical issues
  ("might cause a race if called concurrently" — is it?) belong in the
  report only when you have confirmed the precondition holds.
- **Check that the fix is actually better.** If the proposed
  refactoring would trade one form of complexity for another of roughly
  equal weight, drop it. The fix must produce a clearly better
  cognitive-load / surface-area / correctness outcome.
- **Beware of style-only nits dressed up as findings.** Renames,
  reorderings, or "I'd write it differently" are not findings.

If after this filter a severity group is empty, leave it empty. Empty
is an honest result; padded is not.

## Step 6: Report

Lead with **what's most worth changing** — the highest-leverage
findings, architecture and duplication first. Then the rest, grouped:

- **High** — bugs, correctness issues, architectural violations
- **Medium** — architectural debt, duplication, abstractions that no
  longer earn their place, refactor candidates
- **Low** — convention nits, minor cleanups

**Number every finding** with a severity-class ID so it's easy to refer
to later: `H` / `M` / `L` + a sequential number within that class (`H1`,
`H2`, `M1`, `L1`, …), numbered from 1 per class in report order. The IDs
are local to this report — a handle for the user, not stable tracker IDs.

Each finding: `**H1** `path:line` — what's wrong, *why it matters now*,
concrete fix or direction`. For refactor candidates, sketch the target
shape. If you considered and rejected a related finding (e.g. a
dedup candidate where the existing rationale still holds), do **not**
mention it — silence is the right outcome.

Rules:

- No praise. Don't restate what the code does well. The report is a list
  of things to change.
- No quotas. Do not invent or inflate findings to fill a section. A
  short, sharp report is the goal; a long one with mixed signal is a
  failure mode.
- Omit a severity group when it is empty. Do not synthesise a finding
  just because the group looks bare.
