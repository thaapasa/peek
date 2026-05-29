---
name: scoped-review
description: Scoped code review — determines the change scope (explicit, discussion, or git), picks a source of truth (discussion, new docs, or code), then reviews the changes for correctness, doc agreement, doc coverage, convention adherence, duplicated shapes that want a shared abstraction, hand-rolled complex logic that wants a named/shared function, and unit-test coverage of complex logic.
disable-model-invocation: true
context: fork
agent: general-purpose
---

# Scoped Code Review

A focused review of a *specific set of changes* — not the whole codebase.
The checkup skill audits the whole architecture; this skill reviews a
bounded diff against a chosen source of truth.

**Precision over recall.** A short report of real problems beats a long
one padded with speculation. Every finding the user dismisses as "not
actually a problem" is a tax on the review. If you are not confident a
finding is a real problem *right now*, drop it. Be skeptical of the code
— and equally skeptical of your own findings before they go in.

## Step 1: Determine scope

What changes are under review. Resolve in this order; stop at the first
that applies:

1. **Explicit ask.** If the user named a scope (a path, a commit range,
   a PR, "the parser changes", "since v0.2.6"), that is the scope. Use it
   verbatim.
2. **Discussion context.** If this conversation has been working on a
   specific change, that work is the scope — even if not restated as a
   review target.
3. **Git, in this order:**
   - On a feature branch (not `main`): scope is the branch —
     `git diff main...HEAD` plus any uncommitted changes.
   - Otherwise, if there are uncommitted changes (`git status
     --porcelain` non-empty): scope is the working tree —
     `git diff HEAD` (staged + unstaged), including untracked files.
   - Otherwise: scope is the last commit — `git show HEAD` /
     `git diff HEAD~1..HEAD`.

State the resolved scope in one line at the top of the report, with the
exact diff command, so the user can confirm you reviewed what they meant.
If scope is genuinely ambiguous (e.g. branch *and* a big pile of
unrelated uncommitted work), ask before reviewing rather than guess.

## Step 2: Determine the source of truth

What the changes are measured *against*. The review's job is to check the
change agrees with its source of truth — so picking the right one matters.
Resolve in this order:

1. **Discussion context.** If the conversation laid out what the change
   should do (a spec, an agreed design, acceptance criteria), that is the
   source of truth. Code and docs are judged against it.
2. **New / changed documentation that clearly defines a feature.** If the
   diff includes documentation that *specifies* new behaviour — a new
   manual chapter, a `features.md` / `planned.md` entry, an
   `architecture.md` section describing intent — treat that doc as the
   spec. The code must implement what the doc promises.
3. **Code, with docs judged against it.** If the diff is mostly code with
   only scattered, incidental doc edits (not a coherent spec), the **code
   is the source of truth** and the documentation is reviewed *against the
   code* — stale, contradictory, or missing doc updates are findings, not
   the spec.

State the chosen source of truth in one line in the report. When it's
docs or discussion, the bar is "does the code do what was promised". When
it's code, the bar is "do the docs (and conventions, and tests) keep up
with what the code now does".

Load the context needed to judge against the source of truth:

- `docs/conventions.md` — coding rules and patterns.
- `docs/architecture.md` — design principles, key abstractions, "how to
  add a new file type".
- `CLAUDE.md` — file map and the three north stars (clean architecture,
  stream don't load, low cognitive load).
- Whatever docs/spec the source-of-truth decision points at.

## Step 3: Review the changes

Read the full diff for the scope. For non-trivial changes, also open the
surrounding code at each change site — a diff read in isolation hides
bugs that are obvious in context. Check each of the following.

### 3a. Correctness — the changes work, no bugs introduced

- Logic does what it intends; edge cases handled (empty input, EOF,
  zero-length, overflow, off-by-one, error paths).
- No `unwrap()` / `expect()` / `panic!` on non-test paths where input is
  not statically guaranteed.
- Errors propagate consistently (not `Result` on one path, swallowed on
  its twin).
- No regressions in adjacent behaviour the change touches.
- Honours the north stars where relevant: streaming not whole-file loads
  on large inputs; no allocation in render hot paths; lazy where eager
  would waste work.

### 3b. Agreement with the source of truth

- Code matches what the spec / docs / discussion promised. Flag every
  divergence — both "code does less than promised" and "code does
  something the spec didn't ask for" (scope creep is a finding too).

### 3c. All relevant docs updated

For the behaviour the change introduces or alters, check that **every
doc that should reflect it does**. A user-visible or structural change
with no matching doc update is a finding. Relevant docs, when applicable:

- **README.md** — outward feature summary / usage examples.
- **CLAUDE.md** — top-level architecture map (when top-level structure
  changed) and the workflow/north-star norms.
- **docs/architecture.md** — design, data flow, key abstractions, "how to
  extend".
- **docs/architecture-map.md** — full file/module breakdown (when files /
  modules added, moved, removed).
- **docs/features.md** — shipped features (✅ / ◐). New behaviour that
  shipped belongs here.
- **docs/planned.md** — planned items (☐ / ❓). An item that shipped must
  move out of planned and into features.
- **manual/src/** — the user-facing mdbook chapter for any user-visible
  feature (new flag, keybinding, file type, view mode).

Judge *appropriateness* — not every change touches every doc. A pure
internal refactor may need only `architecture-map.md`; a new CLI flag
needs `cli-reference.md` + README. Flag the docs that genuinely should
have changed and didn't.

### 3d. Conventions adhered to

Against `docs/conventions.md` and the codebase norms:

- Colored output goes through `PeekTheme::paint()` (no raw ANSI outside
  the `ui.rs` status-line composition).
- File-type handling goes through `Registry` / `compose_modes` dispatch,
  not a re-grown `match file_type` chain.
- `bytes::Bytes` over `Vec<u8>` for read-only buffers.
- Naming, error types, module placement, `pub(crate)` vs `pub` match the
  surrounding code. `cargo fmt` clean.

### 3e. Duplicated shapes — refactor signal (important)

Check whether the new functionality introduces code that **duplicates a
shape already present elsewhere** in the repo. This is the key signal that
a common shape should be lifted into a generic implementation.

- Search the repo for the same logic / structure the change adds — exact
  copies and near-copies that drifted slightly (the dangerous kind: they
  diverge into bugs).
- If the change copy-pastes a per-type module shape, a parsing routine, a
  rendering pattern, or a helper that already exists, list **every** site
  and propose the unifying abstraction (shared helper, trait default,
  macro, generic over the varying part).
- Weigh against low cognitive load: three short copies a reader holds in
  their head can beat one abstraction chased across four files. Make that
  call explicitly — but a genuine duplicated shape with real divergence
  risk is a finding, not a style nit.
- Check for existing rationale comments explaining why sites are kept
  separate; if the rationale still holds, drop the finding silently.

### 3f. Hand-rolled complex logic — name it / share it (important)

Check that complex calculation, escaping, encoding, parsing, or similar
fiddly logic is **not inlined raw**.

- Even at a single use site, this kind of logic deserves a named function
  at minimum — both for readability and so it can be unit-tested in
  isolation.
- If the functionality is generic (an escaper, a size/offset calculation,
  a format encoder/decoder, a geometry computation) and there are other
  sites in the repo that do the same thing — or the logic is general
  enough to belong there — it should be promoted to shared utils, not
  left local.
- Flag inline blocks of bit-twiddling, manual escaping, or multi-step
  arithmetic with no name and no test.

### 3g. Test coverage of complex logic

- Every complex calculation / piece of fiddly logic the change adds must
  be covered by unit tests, so regressions are caught.
- Check the diff (or adjacent test modules) for tests exercising the new
  logic's edge cases — not just a happy-path smoke test.
- Untested complex logic is a finding even if the code looks correct;
  "looks correct" is exactly what regresses silently.

## Step 4: Self-validate before reporting

Run every candidate finding through this filter; drop anything that
fails. Do not pad the report.

- **Read the surrounding context** at the cited lines. Many "issues"
  evaporate once the local context is clear.
- **Look for explanatory comments / commit messages.** If the code is
  shaped this way on purpose (intentional duplication, deliberate
  non-abstraction, a documented workaround), assess whether the rationale
  still holds. If it does, drop the finding. If not, the finding must say
  *why* it's stale.
- **Confirm the problem exists today.** "Could be a problem if X" is not
  a finding unless X is real and you confirmed it.
- **Check the fix is actually better.** If a proposed refactor trades one
  complexity for an equal one, drop it.
- **No style-only nits dressed as findings.** Renames / reorderings /
  "I'd write it differently" are not findings.

## Step 5: Report

Open with two lines: the **resolved scope** (with the exact diff command)
and the **chosen source of truth**. Then lead with what's most worth
fixing, grouped:

- **High** — bugs, correctness issues, code that disagrees with the
  source of truth, missing docs for a user-visible change, untested
  complex logic.
- **Medium** — duplicated shapes that should be unified, hand-rolled
  complex logic that should be named/shared, convention violations, stale
  docs that contradict the code.
- **Low** — minor doc gaps, naming, small cleanups.

Each finding: `path:line — what's wrong, why it matters now, concrete
fix`. For duplication / extraction findings, list every site and sketch
the target shape. For source-of-truth divergences, cite both the promise
and the code.

Rules:

- No praise. The report is a list of things to change.
- No quotas. A short sharp report beats a long mixed one. Omit a
  severity group when it's genuinely empty.
- This skill reports; it does not edit. The user (or a follow-up pass)
  makes the changes.
