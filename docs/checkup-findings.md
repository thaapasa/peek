# Checkup findings

IDs are stable. Resolved items are deleted but remaining IDs keep their numbers
so commit / PR references stay valid. Add new IDs at the end of each section
(don't renumber).

## Medium

### M6. The per-type dispatch hubs are a `match file_type` family — wontfix, kept as analysis record

Original finding: adding a file type touches ≥5 dispatch tables (detect,
gather, render, compose, extract); flag when a 6th wiring point appears,
then a `trait FileTypeRegistry` registering all of them at one site
beats the explicit dispatches.

**Trigger fired, remedy declined.** The `Notebook` type (2026-05-30) was
the 6th-type proof. One new type touched six sites:
`crates/peek-detect/src/detect.rs` (variant + `classify_by_name` + content
sniff), `crates/peek-detect/src/mime.rs` (mime arm + extensions arm),
`src/compose.rs` `compose_modes`, `src/extract/extract.rs`, `src/gather/mod.rs`,
and `crates/peek-foundation/src/info/render/mod.rs` + the gather `Extras`
plumbing. On inspection the registry does not pay:

1. **Detection can't join.** `detect.rs` *produces* the `FileType` from
   magic / extension / content sniff — there is no `FileType` value to
   dispatch on yet. A `FileType → impl` registry can absorb at most
   4–5 of the 6 sites, never detect.
2. **Format sub-enums break a 1:1 type→impl map.** `Archive(fmt)`,
   `Document(fmt)`, `Audio(fmt)`, `Cert(fmt)` dispatch on the inner
   format too; a uniform per-type trait fits these awkwardly.
3. **`render` dispatched on `Extras`, not `FileType`** — a different
   axis. ~~Folding it in needs `gather` to return a boxed trait object
   instead of the `FileExtras` enum, losing that enum's exhaustiveness.~~
   **Superseded 2026-06-05.** This axis *was* converted: `gather` now
   returns `Extras` (`Box<dyn InfoExtras>`) and `info::render` dispatches
   through the trait, per-type impls collected in `types/info_impls.rs`.
   Exhaustiveness was *not* lost — `render_section` is a *required* trait
   method, so a type with no impl fails to compile (the same guarantee
   the enum gave; contra point 4's "silent no-op" worry, which applies to
   *defaulted* methods, not this one). Done to break the `info → types`
   cycle that blocks hoisting `types/` into its own crate. The other axes
   (detect, compose, extract, the `gather` `FileType` hub) still decline
   the registry for the reasons in 1–2 and 4.
4. **The explicit form's assets outweigh the typing cost.** The
   non-exhaustive `match` in `compose`/`extract`/`gather` is
   compiler-enforced completeness — the `extract.rs` arm was *forced* by
   a compile error during the notebook work, not forgotten. A trait with
   defaulted methods would silently no-op instead. `compose_modes` also
   stays a single-file overview of the whole dispatch table; a wide
   trait with no-op defaults (most types' extract = `Unsupported`)
   trades that map for scattered impls + click-through.

The cost the finding feared (touch 6 sites) is mechanical and
compiler-guided — low cognitive load. The registry would *add* load.
Revisit only if a dispatcher can silently fall through and ship a bug;
today the compiler blocks that on `compose`/`extract`/`gather`, and a
missing `mime`/`render` arm surfaces as a visible `?` / absent Info
section, not a crash.

**On the cited "drift" — not a defect.** `info/render` needs no
`Compressed` case because `Compressed → gather → binary` produces a
`BinaryInfo` payload, which renders through its `InfoExtras` impl like any
binary. `Compressed` is a transparent-resolve pseudo-type that degrades to
Binary by design; the dispatchers enumerating different `FileType` sets is
correct, not inconsistent.

### M12. `--plain` mutates `args.color` — wontfix, kept as analysis record

Original finding suggested either (a) dropping `--plain` as "`--color
plain` + a derived tweak" or (b) deriving `plain_mode` from the resolved
`StyleMode`. Both miss the semantic distinction. Three effects of
`--plain` are *not* derivable from `StyleMode::Plain`:

1. Disables structured pretty-print (`compose.rs` passes the pretty branch,
   suppressed under plain). `--color plain` alone still reformats JSON.
2. Skips the syntect pipeline entirely
   (`viewer/mod.rs:84-88`: `syntax_token = None` when `plain_mode`).
   `--color plain` still parses and styles, just drops the ANSI at
   the encoder.
3. Suppresses rendered views for SVG / HTML / Markdown
   (`svg|html|markdown/compose.rs`). `--color plain` still composes them.

A user passing `--color plain` to get sterile output expects pretty-print
and rendered views to keep working. Conflating the two flags would
break that.

The one real wart is `src/main.rs:28-29` mutating `args.color` to
`StyleMode::Plain` when `args.plain` is set — the mutation is correct
but hides the user's actual `--color` choice. A cleaner shape would
compute `effective_color = if args.plain { Plain } else { args.color }`
at theme construction without mutating args. Tiny win, not worth a
commit on its own; fold in next time `main.rs` argument plumbing gets
touched.

### M17. `ContentMode::set_search` streams the entire file per query

`crates/peek-foundation/src/viewer/modes/content.rs:708-735` (raw-branch
scan at `722-725`). Each `/`-then-Enter pulls every line through
`self.line_source.iter_all().map(...)` into `SearchState::scan`.
`MAX_MATCHES = 100_000` caps match *storage* but the streaming read isn't
capped — `'scan: for ... break 'scan` in `search::SearchState::scan` exits
only after the match cap, so a zero-hit query on a 1 GB log walks the whole
file every search. Violates the spirit of "stream, don't load" even while
streaming — the cost is paid per query, not per session.

Two directions:

- Cheap: cap by *bytes scanned* in addition to match count, so a no-match
  search on a multi-GB file degrades cleanly with a status warning.
- Invasive: move scanning to a background thread that streams matches in.

### M18. `Mode::render_window`'s `_scroll` parameter is dead for owns-scroll modes

Trait signature at `crates/peek-foundation/src/viewer/modes/mod.rs:228`;
`owns_scroll` default `false` at `mod.rs:265-267`. Six modes return
`owns_scroll() = true` and ignore the caller's `scroll`: ContentMode
(`content.rs:596`), HexMode (`hex.rs:96`), TableMode (`table/mode.rs:213`),
RowsTableMode (`table/rows_mode.rs:739`), PagedImageMode (`paged.rs:432`),
ListingMode (`listing/mode.rs:317`). Trait surface still passes `_scroll`;
readers must learn that `owns_scroll`-true modes silently discard it and use
their internal scroll instead.

Two reasonable directions:

- Split `Mode` into `ScrolledMode` / `OwnsScrollMode`, drop the dead
  parameter, add trait-discrimination in `ViewerState`.
- Keep as-is, document the contract more loudly on the trait.

Not urgent. Flag as refactor candidate — every new owns-scroll mode adds
another `_scroll` underscore.

## Low

### L4. `Mode::status_hints(has_return_target)` parameter only read by `HexMode` — wontfix, kept as analysis record

Trait sig + default at `crates/peek-foundation/src/viewer/modes/mod.rs:300`
(default ignores `_has_return_target`). Only reader: HexMode
(returns `x:exit hex`); every other impl ignores it. Caller threads the
bool at `src/viewer_session/state.rs:121-125`.

**Remedy declined: the proposed fix can't compile.** `HexMode` lives in
`peek-foundation`, `ViewerState` in the bin — the Cargo layering bars a
mode from calling back into the session layer, so
`ViewerState::has_return_target_for(mode_id)` is unreachable from the
impl that needs it. The alternatives are all worse than one defaulted
parameter: a `set_return_target` setter on the mode means hand-synced
state at every mode switch; appending the hint session-side means the
session layer carrying hex-specific knowledge. The parameter *is* the
clean channel for session context to reach hint rendering; most modes
never override `status_hints` at all, so the cost is one `_`-prefixed
name in the trait default.

### L13. 16 MB render cap also bounds in-container image payloads — wontfix, kept as analysis record

`RENDER_MAX_BYTES` was sized for text payloads ("a 16 MB
`document.xml`"), but the shared `read_zip_entry` gate now also bounds
CBZ page and EPUB image reads
(`crates/peek-types/src/types/comic/cbz/package.rs:67`,
`crates/peek-types/src/types/ebook/epub/read_mode.rs:562`). A legitimate >16 MB
archival scan refuses to render inside the container while the
identical file opened standalone renders fine (the image type reads
`read_bytes()` uncapped). Not a defect — the safety rationale
(alloc-abort is uncatchable, unlike a render `Err`) holds for images
too, the degrade path is soft (warning line; TOC / Info / hex / extract
all still work), and typical pages run 1–5 MB. Act only if a real
oversize page/image surfaces; the fix is a second, larger image-payload
cap passed into `read_zip_entry` per call — not gate removal.
