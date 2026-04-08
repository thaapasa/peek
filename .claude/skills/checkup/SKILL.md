---
name: checkup
description: Periodic codebase health check — scan for architectural issues, duplication, code smells, and convention violations
disable-model-invocation: true
context: fork
agent: Explore
---

# Codebase Review

Perform an architectural review of the peek codebase. Read the project's design
documents first, then scan the source code for issues.

## Step 1: Load context

Read these files to understand the intended architecture and conventions:

- `docs/architecture.md` — design principles, data flow, key abstractions
- `docs/conventions.md` — coding rules and patterns
- `CLAUDE.md` — file map

## Step 2: Scan for issues

Explore the `src/` directory and check for:

**Architecture violations:**
- Colored output that bypasses `PeekTheme::paint()` (raw ANSI escapes outside
  `ui.rs` status line composition)
- Event handling logic that should be in `ViewerState::handle_key()` but is
  duplicated in individual viewers
- Image rendering that doesn't follow the resize-before-composite order
- File type handling that bypasses the `Viewer` trait / `Registry` dispatch

**Code quality:**
- Functions over 100 lines that could be broken up
- Duplicated logic across modules (same pattern in 2+ places)
- Parameter lists that should be a struct (4+ related params threaded together)
- `unwrap()` or `expect()` in non-test code
- Dead code, unused imports, or stale comments

**Consistency:**
- Inconsistent error handling (some paths return `Result`, similar paths don't)
- Naming inconsistencies across similar constructs
- Public items that should be `pub(crate)` or private

**Missing pieces:**
- New file types or viewers that lack info screen metadata in `info.rs`
- Interactive viewers missing standard key bindings (scroll, view switching)
- Features documented in `docs/features.md` whose status is out of date

## Step 3: Report

Produce a concise report grouped by severity:

- **High** — bugs, architectural violations, correctness issues
- **Medium** — duplication, code smells, inconsistencies
- **Low** — style nits, minor improvements

For each issue, include the file path, line number, and a brief description of
what's wrong and how to fix it. Skip anything that's working fine — only report
actual issues found.

If the codebase is clean, say so.
