# Checkup findings

Live tracker for `/checkup` findings. Keeping it up to date:

- **IDs are stable.** Fixed findings are deleted; remaining IDs keep their numbers so commit / PR
  references stay valid. New IDs go at the end of a section — don't renumber.
- **Active findings first**, grouped by severity. A finding decided against moves to *Wontfix
  records* at the bottom, compressed to ~5 lines: what was flagged, why declined, what would
  reopen it.
- **Durable design rationale lives in `docs/architecture.md`** (or a comment at the site the
  decision guards), not here — the record links to it. This file only carries the dedup stub.
- `/checkup` consults the records to avoid re-reporting settled findings. The records are not
  immunity: if the cited situation has changed — code moved, the declining rationale no longer
  holds, a reopen trigger fired — checkup must revalidate and surface a fresh finding saying what
  changed.

No active findings.

## Wontfix records

### M6. Per-type dispatch hubs are a `match file_type` family

`FileTypeRegistry` declined after the 6th-type trigger fired (Notebook, 2026-05-30): detection
*produces* `FileType` so it can't join a registry; format sub-enums break a 1:1 type→impl map; the
explicit matches are compiler-enforced completeness where trait defaults would silently no-op.
Full rationale: `architecture.md` → "Why the dispatch arms stay explicit". Reopen if a dispatcher
can someday silently fall through and ship a bug.

### M12. `--plain` mutates `args.color`

Merging `--plain` into `--color plain` declined: plain mode additionally suppresses pretty-print,
the syntect pipeline, and rendered views — none derivable from `StyleMode::Plain`. Rationale:
`architecture.md` → Registry. Residual wart stays open there too: `main.rs` mutates `args.color`;
compute an `effective_color` instead next time the argument plumbing is touched.

### M18. `render_window`'s `scroll` parameter is dead for owns-scroll modes

`ScrolledMode` / `OwnsScrollMode` trait split declined (2026-06-12): it would bifurcate the mode
vocabulary and `ViewerState` dispatch to remove one ignored argument, and most data modes own
scroll anyway. Contract is documented on the trait (`viewer/modes/mod.rs`) and in
`architecture.md` → Mode trait. Reopen if the dead parameter starts shipping real bugs.

### L4. `status_hints(has_return_target)` read only by `HexMode`

Proposed fix can't compile: Cargo layering bars foundation modes from calling back into the bin's
`ViewerState`, and every workaround (mode-side setter, session-side hex special-casing) costs more
than one defaulted parameter — it is the channel for session context to reach hint rendering.
Rationale: `architecture.md` → Mode trait.

### L13. 16 MB render cap also bounds in-container image payloads

Intentional: alloc-abort safety holds for images too, the degrade path is a soft warning, typical
pages run 1–5 MB. Rationale on `RENDER_MAX_BYTES` (`viewer/modes/rendered_text.rs`). Reopen if a
real >16 MB page/image surfaces; the fix is a second, larger image-payload cap passed into
`read_zip_entry` per call — not gate removal.
