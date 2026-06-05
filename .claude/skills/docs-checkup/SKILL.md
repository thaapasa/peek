---
name: docs-checkup
description: Periodic documentation review — checks docs/ hygiene, verifies features.md / planned.md / architecture.md / README.md / CLAUDE.md match implementation, and audits the mdbook manual against actual CLI flags, keybindings, and shipped file-type support.
disable-model-invocation: true
context: fork
agent: general-purpose
---

# Documentation Review

A critical pass over peek's documentation. The goal is **not** to confirm
docs exist — they do. The goal is to find where docs lie about the code,
where they document planned features as if they shipped, where
references rot after refactors, and where the hygiene rules in
`CLAUDE.md` have been violated.

Be skeptical. Docs drift faster than anything else in the repo because
nothing fails to compile when a manual chapter is wrong. A review that
concludes "all good" almost certainly didn't cross-check against the
source.

## Step 1: Load context

Read these to understand what the docs are *supposed* to look like and
what the code actually does:

- `CLAUDE.md` — top-level architecture overview + the **Docs hygiene**
  rules under "Documentation". These rules are the floor for Step 2.
- `docs/architecture.md` — design, key abstractions, "how to add a new
  file type" reference.
- `docs/architecture-map.md` — full file/module breakdown.
- `docs/features.md` — currently shipped features (✅ + ◐).
- `docs/planned.md` — planned features / open ideas (☐ + ❓).
- `docs/conventions.md`, `docs/release.md` — supporting reference.
- `docs/archived/` — completed plans kept for design rationale.
- `README.md` — outward-facing feature summary + usage examples.
- `manual/src/SUMMARY.md` + every chapter under `manual/src/`.
- `src/cli.rs` — clap Args, the authoritative CLI flag list.
- `crates/peek-foundation/src/viewer/ui/keys.rs` — authoritative keybinding map.
- `crates/peek-foundation/src/viewer/ui/help.rs` — in-app help screen text.
- `crates/peek-detect/src/detect.rs` — `FileType` enum + format enums, the
  authoritative file-type list.
- `crates/peek-types/src/types/*/` — per-type modules, the authoritative source of "what
  peek actually does for type X".
- `git log --oneline -30` — recent direction; what likely drifted.

### Self-check: validate this SKILL.md's own references

Before trusting the map above, verify it. The skill itself rots: file
splits and module renames silently invalidate paths the procedure
points at, and an audit running against a stale map will miss the
drift it's supposed to find.

For every path / module / symbol named in this SKILL.md (the bulleted
sources of truth in Step 1, plus the file-citation examples in Steps 3
and 4 — `src/cli.rs`, `crates/peek-foundation/src/viewer/ui/keys.rs`,
`crates/peek-foundation/src/viewer/ui/help.rs`, `crates/peek-detect/src/detect.rs`,
`crates/peek-types/src/types/*/`,
`crates/peek-theme/src/name.rs::PeekThemeName`,
`crates/peek-theme/src/style_mode.rs::StyleMode`,
`crates/peek-foundation/src/viewer/ui/keys.rs::Action`, `src/cli.rs::Args`,
`src/extract/`, every `docs/*.md` filename, `docs/archived/`,
`manual/src/SUMMARY.md`, `manual/src/cli-reference.md`,
`manual/src/keyboard-shortcuts.md`, `manual/src/environment.md`):
confirm the file exists and the symbol resolves. If any reference is
stale, that's a **High** finding in the report: the skill itself
must be fixed before the audit it produces can be trusted.

## Step 2: Docs hygiene sweep

Check the rules CLAUDE.md → Documentation → "Docs hygiene" lays down:

- **`docs/` root holds live reference only.** No "(landed)" / "shipped"
  plans sitting at the root. No dated snapshots. No postmortems.
  Anything that fits one of those shapes belongs in `docs/archived/` or
  should be deleted.
- **Archived files carry the status blockquote.** Every file under
  `docs/archived/` must start (within the first few lines) with
  `> **Status: Completed YYYY-MM-DD.** Archived for reference.` (or
  `Archived` for snapshots). Title stays the same; the blockquote is
  the marker.
- **Cross-references point at the right path.** When a file moves to
  `docs/archived/`, every linker (`docs/planned.md`,
  `docs/architecture-map.md`, `CLAUDE.md`, `README.md`, manual
  chapters, sibling docs) must follow. Grep for stale relative paths.
- **`docs/architecture-map.md` does not list archived files.** Archive
  is a graveyard; the map covers live docs only.
- **Active instructions are not buried inside plans.** "How to add a
  new X" / "the rule for Y" content must live in a general doc
  (`architecture.md`, `conventions.md`, or its own top-level doc), not
  inside a plan file (active or archived) that future readers won't
  know to open. If the only place a reusable instruction lives is
  inside a plan, flag it.
- **`MEMORY.md` (under `.claude/`) is an index, not a memory.**
  Out of scope for this review — that's user-memory territory.

## Step 3: Live-docs accuracy

For each live doc, compare against the code. Note: features.md and
planned.md split on a clean rule — features.md is what's shipped (✅/◐),
planned.md is what isn't (☐/❓). Items must appear in exactly one.

- **`features.md`** — every ✅ / ◐ entry must correspond to working
  code. Spot-check 3–5 randomly chosen entries against `src/types/<x>/`
  to confirm. Look for entries describing capabilities that have since
  been removed, renamed, or split. Look for ◐ entries that have
  actually shipped fully (should be ✅).
- **`planned.md`** — every ☐ / ❓ entry should NOT yet be implemented.
  If a planned item actually shipped, it must move to features.md.
  Cross-check the "Done since previous snapshot"-style risk: items
  that landed quietly without doc updates. Also check that links to
  archived plans resolve.
- **`architecture.md`** — file/module references valid; "Adding a new
  file type" steps still match the actual `compose_modes` / detection /
  `FileExtras` shape; example code blocks compile mentally against the
  current API.
- **`architecture-map.md`** — every entry under `src/` matches a real
  file/module; every real top-level module under `src/` is mentioned.
  The map is the most rot-prone doc — file splits, renames, and new
  modules all silently invalidate entries.
- **`README.md`** — feature summary matches `features.md` (no version
  skew); usage examples still work (CLI flag spelling, behaviour);
  install / build instructions match `release.md` and `install.sh`.
- **`CLAUDE.md`** — top-level architecture map under `src/` matches
  reality (it's a condensed version of architecture-map.md, but
  condensation rots independently). North stars and workflow rules
  still reflect collaboration norms.

## Step 4: Manual accuracy

The mdbook manual under `manual/src/` is the user-facing reference. It
must document what peek **actually does today**, not what's planned.
Aspirational documentation is a bug — a user trying a documented feature
that doesn't exist is the worst failure mode.

For each chapter, cross-check against the source of truth:

- **`cli-reference.md`** vs `src/cli.rs`. Every flag in `Args` must be
  documented; every flag documented must exist. Defaults, value
  enums, short/long forms, and help text wording should match. Hidden
  flags (`hide_short_help`) should still appear in the full reference.
- **`keyboard-shortcuts.md`** + per-chapter keybinding notes vs
  `crates/peek-foundation/src/viewer/ui/keys.rs` and
  `crates/peek-foundation/src/viewer/ui/help.rs`. Every key the
  app binds must be in the reference (or explicitly per-chapter);
  every key the manual claims must actually be bound. Watch for
  per-mode bindings (paged image `n`/`p`, listing `e`, search `/`
  `n` `N`) that depend on which mode is active.
- **`environment.md`** vs env vars actually read in the code. Grep
  `std::env::var` / `env!` across `src/` and `crates/` for completeness.
- **`file-types/*.md`** — every chapter must describe a file type that
  ships today (i.e., has a `FileType` variant + `types/<x>/compose.rs`
  / equivalent). The set of chapters should cover every shipped file
  type — gaps are as bad as ghost chapters. Per-chapter content:
  describe view modes that exist (cycle keys, paged vs listing vs
  rendered vs hex), info-screen fields actually surfaced
  (`types/<x>/info_render.rs`), extract behaviour for archive-like
  types. Do not document planned-but-unshipped enhancements as if
  they're live.
- **`viewer/*.md`** (info screen, themes, color modes, line numbers /
  wrap, extraction) vs the corresponding `crates/peek-foundation/src/viewer/` and
  `crates/peek-theme/src/` modules. Theme list must match `PeekThemeName` variants
  in `crates/peek-theme/src/name.rs`. Color-mode list must match `StyleMode`
  variants in `crates/peek-theme/src/style_mode.rs`. Extraction behaviour
  (`--extract`, `e` in viewer) must match `src/extract/` reality.
- **`SUMMARY.md`** — every chapter file referenced exists; every
  chapter file under `manual/src/` is referenced. mdbook silently
  drops orphans.

When in doubt, the source code wins. The manual is wrong, not the code.

## Step 5: Report

Lead with **what's most worth fixing** — the highest-impact lies first
(manual documenting a feature that doesn't exist, planned item described
as shipped in README). Then group:

- **High** — false claims (docs describe behaviour that doesn't exist),
  missing CLI flags / keybindings / file types from the manual, broken
  cross-references after archive moves, hygiene-rule violations
  (orphaned landed plans in `docs/` root).
- **Medium** — stale module paths and line numbers, ◐ entries that
  should be ✅ (or vice versa), planned items that shipped without
  promotion to features.md, manual chapters missing capabilities that
  were added.
- **Low** — wording drift, outdated screenshots / examples, minor
  consistency nits between `architecture-map.md` and `CLAUDE.md`
  condensed map.

Each finding: `path:line — what's wrong, why it matters, concrete
fix`. For accuracy findings, cite both the doc claim and the code
ground truth so the fix is unambiguous.

Rules:

- No praise. The report is a list of things to change.
- Don't manufacture issues. But docs in a project the size of peek
  always carry drift — if a section is empty, you under-looked.
- Omit a severity group only if it's genuinely empty after a real look.
- This skill does not edit docs. It reports; the human (or a follow-up
  pass) makes the changes.
