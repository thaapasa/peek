---
name: analyze
description: Analyze a target — a crate, a module within a crate, or a single file — for its surface within peek, its place in the architecture, its API, its correctness, and its efficiency. Produces an overview followed by findings. Catches obvious bugs, not a deep edge-case bug hunt.
disable-model-invocation: true
context: fork
agent: general-purpose
---

# Target Analysis

A focused analysis of **one target** inside peek. The target is given as
input (`$ARGUMENTS`) and is one of:

- a **crate** — e.g. `peek-io`, `peek-detect`, `peek-foundation`
- a **module within a crate** — e.g. `peek-foundation/viewer/listing`,
  `peek-types::types::pdf`, `peek-io` codecs
- a **single file** — e.g. `crates/peek-io/src/line_source.rs`

If the input is missing or genuinely ambiguous (matches several crates /
modules with no obvious winner), ask which target before analysing —
don't guess and burn the pass on the wrong code.

The deliverable is an **overview of the target plus findings about it**.
This is an understand-and-assess pass, not a checklist audit and not an
exhaustive bug hunt — catch *obvious* bugs, but do not chase every edge
case.

**Precision over recall.** A short report of real findings beats a long
one padded with speculation. Every finding the user dismisses as "not
actually a problem" is a tax on the analysis. If you are not confident a
finding is real *right now*, drop it. Be skeptical of the code — and
equally skeptical of your own findings before they go in.

## Step 1: Resolve the target and load context

1. **Resolve the target** to a concrete set of files. A crate → its
   `src/` tree (note the `lib.rs` façade and the module layout). A module
   → that module's file(s). A file → that file. Use `Glob` / `Grep` to
   pin down the exact paths; state the resolved file set in one line at
   the top of the report.
2. **Read the target's own docs first.** Per-file `//!` module
   doc-comments are the design intent — read the header of every file in
   scope before judging the code. They say *what* the module does and
   *why*.
3. **Load the surrounding architecture** so "place in the architecture"
   is grounded, not guessed:
   - `CLAUDE.md` — the file map and the three north stars (clean
     architecture, stream don't load, low cognitive load). The crate
     descriptions in the file map state each crate's intended
     responsibility and dependency direction.
   - `docs/architecture.md` — design, data flow, key abstractions.
   - `docs/conventions.md` — coding rules, when correctness/efficiency
     findings need a convention to cite.
   - For an I/O- or size-sensitive target, `docs/memory-streaming.md` —
     the budget classes and the rule every whole-file read must follow.

## Step 2: Analyse the five axes

Work the target across these five axes. Read the actual code — don't
infer behaviour from names or doc-comments alone; the doc may have
drifted from the code.

### 2a. Surface within peek

What the rest of peek sees and uses from this target.

- What is `pub` (crate-public vs fully public)? What's the façade —
  the `lib.rs` re-exports, the trait(s), the entry-point functions?
- **Who depends on it, and how widely?** Grep the workspace for the
  target's public names. Is the surface broad (many callers, many types)
  or narrow (one trait, a couple of entry points)? A wide surface on a
  low-level crate is a coupling signal.
- Is anything `pub` that has no external caller and should be
  `pub(crate)` / private? Is anything reached around the façade (callers
  naming internal paths the façade was meant to hide)?

### 2b. Place in the architecture

Where the target sits in the layering and whether it stays in its lane.

- Which layer is it (per the CLAUDE.md map), and what is it *allowed* to
  depend on? peek's layering is Cargo-enforced — detection and the parser
  layer are barred from the bin's session layer. Confirm the target's
  actual `use` / dependency set matches its stated place: does a
  low-level crate reach upward, does a leaf crate pull in a sibling it
  shouldn't?
- Does it have **one narrow responsibility**, or has it accreted
  unrelated helpers into a grab-bag?
- Does its existence still earn its place — does it reduce surface area /
  ease extension, or is it indirection that only forwards?

### 2c. API

The shape and ergonomics of the public interface.

- Is the API coherent — consistent naming, consistent error types,
  predictable ownership (`Bytes` over `Vec<u8>` for read-only buffers per
  convention)? Does it follow `bytes::Bytes`, builder/option-struct, and
  the other repo idioms?
- Is it easy to misuse — footguns, required call ordering not encoded in
  types, init sequencing the caller must remember, leaking of the
  mechanism behind the abstraction?
- Are errors propagated consistently (not `Result` on one path, swallowed
  on its twin)? Are panics (`unwrap` / `expect` / `panic!`) confined to
  paths where input is statically guaranteed, or can hostile/EOF/empty
  input reach them?

### 2d. Correctness

Does the code do what its doc-comment says, with obvious bugs called out.

- Logic matches stated intent; the **obvious** edge cases are handled —
  empty input, EOF, zero-length, off-by-one on window/anchor/offset math,
  integer overflow on size arithmetic, error paths.
- For a streaming / seeking / decompression target especially: boundary
  handling at chunk edges, partial reads, seek-into-window correctness,
  spill-to-tempfile paths.
- Flag obvious bugs with a concrete reason they fire. This is **not** a
  deep all-edge-cases hunt — depth here is "a careful reader would catch
  this", not "fuzzing found it".

### 2e. Efficiency

Whether the target honours "stream, don't load" and avoids waste.

- **Whole-file reads where streaming/seeking would do** — the cardinal
  sin here. Does the target slurp a multi-GB input into memory where a
  `ByteSource` random read or chunked iteration would serve? Is any
  whole-file read gated behind a budget (per `docs/memory-streaming.md`)?
- Redundant work — re-reading, re-decoding, re-allocating on a hot path;
  eager computation that should be lazy.
- Allocation in tight loops / render paths; needless `clone()` of large
  buffers where `Bytes` refcount-clone or a borrow would do.

## Step 3: Self-validate before reporting

Run every candidate finding through this filter; drop anything that
fails. Do not pad.

- **Read the surrounding context** at the cited lines. Many "issues"
  evaporate once the local context is clear.
- **Look for explanatory comments / commit messages.** If the code is
  shaped this way on purpose (a documented workaround, a deliberate
  non-abstraction, a known constraint), assess whether the rationale
  still holds. If it does, drop the finding. If not, say *why* it's
  stale.
- **Confirm the problem exists today.** "Could be a problem if X" is not
  a finding unless X is real and you confirmed it.
- **Check the fix is actually better** — no trading equal complexity for
  equal complexity, no style-only nits dressed as findings.

## Step 4: Report

Two parts, in this order.

**1. Overview** — a short prose picture of the target so a reader who's
never opened it understands it:

- One-line resolved file set (the exact paths analysed).
- What the target is and does, in a few sentences.
- Its surface and its place in the architecture (axes 2a–2b distilled):
  what it exposes, who uses it, which layer it sits in, what it depends
  on.
- A one-line health read on API / correctness / efficiency — the shape of
  things before the specific findings.

**2. Findings** — lead with what's most worth fixing, grouped:

- **High** — bugs, correctness issues, architectural-boundary violations
  (reaching across a layer the layering bars), whole-file loads on large
  inputs.
- **Medium** — API footguns, abstractions that no longer earn their
  place, efficiency waste on warm paths, convention violations.
- **Low** — surface that should be `pub(crate)`, minor cleanups, naming.

**Number every finding** with a severity-class ID so it's easy to refer
to later — same scheme as `docs/checkup-findings.md`: `H` / `M` / `L` +
a sequential number within that class (`H1`, `H2`, `M1`, `L1`, …).
Number from 1 per class, in report order. These IDs are local to the
report (not the checkup tracker's stable IDs) — they just give the user
a handle per finding.

Each finding: `**H1** `path:line` — what's wrong, why it matters now,
concrete fix`.

Rules:

- No praise. The overview describes; the findings list things to change.
- No quotas. A short sharp report beats a long mixed one. Omit a
  severity group when it's genuinely empty — empty is an honest result.
- This skill reports; it does not edit. The user (or a follow-up pass)
  makes the changes.
