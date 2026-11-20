# Checkup findings

This file is the live tracker for `/checkup` findings. To keep it up to date, follow these rules:

- **IDs are stable.** When a finding is fixed, delete it. The remaining findings keep their IDs, so
  references in commits and pull requests stay valid. Add new IDs at the end of a section, and don't
  renumber.
- **Active findings come first**, grouped by severity. If you decide against a finding, move it to
  _Won't-fix records_ at the bottom and compress it to about 5 lines. The record says what was
  flagged, why it was declined, and what would reopen it.
- **Durable design rationale lives in `docs/architecture.md`**, or in a comment at the site that the
  decision guards. It doesn't live here. The record links to the rationale, and this file carries
  only the stub that prevents duplicate reports.
- `/checkup` reads the records so that it doesn't report settled findings again. The records don't
  grant immunity. If the situation that a record cites has changed, `/checkup` must check the
  finding again and report a fresh finding that says what changed. For example, the code may have
  moved, the reason for declining may no longer hold, or a reopen trigger may have fired.

## Low

### L14. Fuzzing covers only `peek-detect`, and the hand-rolled `peek-types` parsers have no fuzz coverage

The security audit on 2026-06-21 surfaced this finding. The stable fuzz floor
(`peek-detect/tests/fuzz_detect.rs`) and the nightly libFuzzer target exercise only the detection
layer.

The hand-rolled binary parsers in `peek-types` have no fuzz coverage. These include the `ds_store`
buddy allocator, WOFF, the disk-image readers (`mish`, `mbr`, `iso_pvd`, and `dmg`), and the `ar`
and `cpio` headers. peek reaches them only _after_ detection routes to them. They rely on
hand-written unit tests instead. The audit found them clean: each file-controlled length is
bounds-checked before allocation, so it found no reachable panic or out-of-bounds access.

As a future item, add a libFuzzer target for each high-value parser. This hardens the floor against
future edits. It isn't urgent, because there is no live bug, and the work is regression insurance.
Reopen this as a real finding only if a parser change lands without matching adversarial tests.

## Won't-fix records

### M6. Per-type dispatch hubs are a `match file_type` family

The trigger for the sixth type fired with Notebook on 2026-05-30, and `FileTypeRegistry` was
declined then. There are three reasons:

- Detection _produces_ `FileType`, so it can't join a registry.
- Format sub-enums break a one-to-one map from type to implementation.
- The explicit matches give completeness that the compiler enforces. Trait defaults would silently
  do nothing instead.

For the full rationale, see "Why the dispatch arms stay explicit" in `architecture.md`. Reopen this
if a dispatcher can someday silently fall through and ship a bug.

### M12. `--plain` kept separate from `--color plain`

Merging `--plain` into `--color plain` was declined. Plain mode also suppresses pretty-printing, the
syntect pipeline, and rendered views. None of these follow from `StyleMode::Plain`. For the
rationale, see the Registry section in `architecture.md`.

The former wart, where `main.rs` mutated `args.color`, is gone. `cli::parse` computes the effective
encoding once, and `args.color_arg` keeps the user's choice.

### M18. The `scroll` parameter of `render_window` is dead for owns-scroll modes

A split into `ScrolledMode` and `OwnsScrollMode` traits was declined on 2026-06-12. It would split
the mode vocabulary and the `ViewerState` dispatch in two just to remove one ignored argument. Most
data modes own their scroll position anyway. The trait (`viewer/modes/mod.rs`) and the Mode trait
section in `architecture.md` document the contract. Reopen this if the dead parameter starts
shipping real bugs.

### L4. `status_hints(has_return_target)` read only by `HexMode`

The proposed fix can't compile. Cargo layering bars foundation modes from calling back into the
`ViewerState` of the bin. Every workaround costs more than one defaulted parameter. The workarounds
include a setter on the mode side and special-casing hex on the session side. The parameter is the
channel that carries session context to hint rendering. For the rationale, see the Mode trait
section in `architecture.md`.

### L13. The whole-document render cap also bounds in-container image payloads

This is intentional. Safety against allocation aborts holds for images too, the degrade path is a
soft warning, and typical pages run 1 to 5 MB. The rationale is on `RENDER_MAX_BYTES`
(`viewer/modes/rendered_text.rs`). Reopen this if a real page or image over the cap surfaces. The
fix is then a second, larger cap for image payloads, passed into `read_zip_entry` on each call. The
fix isn't to remove the gate.
