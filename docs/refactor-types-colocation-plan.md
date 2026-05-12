# Types colocation refactor (planned)

Plan for tightening the three-layer split (input / display / type-support) so that
adding a new file type is a localized change: code goes into `types/xxx/`, with a
small number of one-line additions at central wiring sites.

This is **structural cleanup only** — no new traits, no plugin indirection.
Dispatch stays hard-coded so jumping from "type X handled here" to "type X's
implementation" remains a single IDE-jump. The plugin trait variant is parked
as a follow-up in [planned.md](planned.md).

## Goals

1. **Type-specific code lives in `types/xxx/`.** When a file type owns a struct,
   a parser, a stats shape, a render section, or a view mode that only it uses,
   that code sits in its module.
2. **Cross-type infrastructure lives in its layer.** Viewer-layer abstractions
   used by many types (listing TOC, generic modes) belong in `viewer/`. Input-layer
   primitives (source, lines, MIME, decompression, detection orchestrator) belong
   in `input/`.
3. **Wiring sites are short and exhaustive.** `Registry::compose_modes`,
   `info::FileExtras`, `FileType`, and the detection orchestrator stay as
   single-file overviews — the body of each arm can live next to its type, but
   the per-type entry remains a one-liner you can scan top-to-bottom.
4. **Adding a new file type is mostly additive.** New code under `types/xxx/`,
   plus one-line wiring touches in the central sites above. No edits scattered
   across many files.

## Out of scope

- `TypeSupport` / plugin trait dispatch. Browsing one match arm to its
  implementation is fine; replacing the match with trait iteration loses the
  single-file dispatch overview. Park for later.
- Re-architecting `Mode` trait, `InputSource`, theme manager. Those abstractions
  hold up; the refactor only relocates code.
- `extract/` dispatch. Already split correctly (dispatcher centralized,
  per-type extractors in types/).

## Step 1 — Stats colocation

**Status:** half-converted. Newer types own a stats struct
(`Ebook(EbookStats)`, `Comic(ComicStats)`, `Document(DocumentStats)`,
`Pdf(PdfStats)`, `Audio(AudioStats)`). Older types still inline their shape
in `info/mod.rs::FileExtras` (Image / Text / Svg / Markdown / Sql / Binary /
Archive / DiskImage / Directory).

**Target:** every `FileExtras` variant wraps a single type-owned struct.

```rust
pub enum FileExtras {
    Image(ImageStats),
    Text(TextStats),
    Svg(SvgStats),
    Structured(StructuredInfo),
    Markdown(MarkdownInfo),
    Sql(SqlInfo),
    Binary(BinaryInfo),
    Archive(ArchiveStats),
    DiskImage(DiskImageInfo),
    Directory(DirectoryStats),
    Ebook(EbookStats),
    Comic(ComicStats),
    Document(DocumentStats),
    Pdf(PdfStats),
    Audio(AudioStats),
}
```

Each struct lives in `types/<type>/info.rs`. Free types currently used
across files (`TextStats`, `LineEndings`, `IndentStyle`, `Encoding`,
`AnimationStats`, `LoopCount`, `SvgAnimationStats`, `IsoVolumeMeta`,
`DmgMeta`, `MbrTable`, `IsoDateTime`, `StructuredStats`, `TopLevelKind`,
`MarkdownStats`, `FrontmatterKind`, `SqlStats`, `SqlDialect`) move with
their owning type. Types that consume `TextStats` (Markdown, SQL, SVG)
import it from `types/text/info.rs`.

**Why first:** mechanical, no behavior change, no new abstractions. Each
move is a `git mv` plus import fixes. Unlocks downstream cleanup because
once stats live with types, `info/mod.rs` shrinks to enum + permission
helper.

**Estimated touch surface:** ~10 files moved, imports updated in ~20.

## Step 2 — Listing → viewer

`types/listing/` is reused by archive, comic (CBZ), ebook (EPUB), document
(DOCX/ODT/RTF embeds), pdf (embeds), audio (embeds), disk_image (ISO),
directory. Eight callers; not a file type.

**Target:** `viewer/listing/` (or `viewer/modes/listing/`). Re-exports:
`Entry`, `EntryKind`, `EntryMtime`, `FlatEntry`, `Stats`, `ListingMode`,
`from_flat_paths`, `time_from_epoch_secs`.

**Why second:** pure relocation, no semantic change. Mostly `git mv` plus
import rewrite. Frees `types/` of its biggest miscategorized module.

`types/directory/` stays in `types/` — peeking a filesystem directory is
a real "type" the user invokes; DirectoryMode is type-specific (synthetic
`..` row, follows symlinks).

## Step 3 — Format enums + detection down to types

**Current:** `input/detect.rs` holds `FileType` (the union) plus every
per-type format enum: `ArchiveFormat`, `StructuredFormat`, `EbookFormat`,
`DocumentFormat`, `ComicFormat`, `DiskImageFormat`, `AudioFormat`. Adding
a new type means editing detect.rs to add the format enum, the detection
hints (magic bytes table, extension table), and a `FileType` variant.

**Target:**

- Per-type format enums move to `types/<x>/format.rs`. detect.rs imports
  them.
- Per-type detection contribution moves to `types/<x>/detect.rs`. Each
  exposes:
  ```rust
  pub fn detect_by_extension(ext: &str) -> Option<FileType> { … }
  pub fn detect_by_magic(head: &[u8]) -> Option<FileType> { … }
  ```
  Returning `FileType` (not the type's local enum) keeps the central
  enum exhaustive and lets detection be ordered globally.
- `input/detect.rs` becomes the orchestrator: priority chain, stdin
  sniffing, name-vs-magic mismatch reporting, transparent compression
  unwrap. The per-type lookup tables are gone — it calls each type's
  contribution.
- `FileType` itself stays central. Variants reference type-owned format
  enums (`FileType::Archive(types::archive::ArchiveFormat)` etc.). One
  enum, one place to look up.

**Why third:** removes the cycle where types must reach back into
`input/detect.rs` to name their own format variants. After this, the
dependency is `input → types/*/{format,detect}` only — types own their
identity. Detection orchestration stays in `input/` because priority and
fallback are global concerns.

**Caveats:**

- Order matters for ambiguous prefixes (e.g. ZIP magic for DOCX vs EPUB
  vs CBZ vs raw ZIP). The orchestrator must encode priority; per-type
  detectors must not race. Keep an explicit ordered list in detect.rs:
  ```rust
  static DETECTORS: &[DetectFn] = &[
      types::ebook::detect_by_magic,   // DOCX/EPUB-style ZIPs first
      types::document::detect_by_magic,
      types::comic::detect_by_magic,
      types::archive::detect_by_magic, // raw ZIP last
      …
  ];
  ```
- Magic-byte tests sometimes need full-file context (e.g. central
  directory walk to distinguish EPUB from DOCX from CBZ). Keep those
  expensive contributions opt-in via a separate slower-path phase if
  the cheap header sniff is ambiguous.

## Step 4 — Compose dispatch bodies down to types

**Current:** `Registry::compose_modes` is a ~350-line match in
`viewer/mod.rs`. Each arm parses, lists, probes, and constructs modes
inline.

**Target:** the match stays — single-file overview is the win — but each
arm body shrinks to a function call:

```rust
match file_type {
    FileType::Image          => types::image::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Svg            => types::svg::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Html           => types::html::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Ebook(_)       => types::ebook::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Document(_)    => types::document::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Pdf            => types::pdf::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Comic(_)       => types::comic::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Archive(_)     => types::archive::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::DiskImage(_)   => types::disk_image::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Audio(_)       => types::audio::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::Directory      => types::directory::compose(source, detected, args, &ctx, &mut modes)?,
    FileType::SourceCode { .. }
    | FileType::Structured(_) => /* text_content_mode — generic */
    …
}
```

Each `types::<x>::compose` lives at `types/<x>/compose.rs`. The body that
used to sit in the giant match arm moves there with no other change —
same eager parses, same warning composition, same mode pushes. Shared
context (theme manager, image config, peek_theme) passes through a
`ComposeCtx` struct so call sites stay short.

Universal tail (Hex/Info/About/Help dedupe) stays in `Registry::compose_modes`
after the match.

**Why fourth:** depends on Steps 1–3 settling first. Composition is the
biggest mechanical move but the easiest to land safely once the
underlying shapes (stats, listing, format enums) are stable. With this
done, adding a new type is roughly:

1. Create `types/xxx/` with `format.rs`, `detect.rs`, `info.rs`,
   `info_gather.rs`, `info_render.rs`, `compose.rs`, plus any parser /
   mode files.
2. Add a `FileType::Xxx` variant in `input/detect.rs::FileType`.
3. Add the `XxxStats` variant to `info::FileExtras`.
4. Add the type's detector to the ordered `DETECTORS` list in
   `input/detect.rs`.
5. Add a `FileType::Xxx => types::xxx::compose(…)` arm in
   `Registry::compose_modes`.
6. Add a render dispatch arm in `info::render`.

Six wiring lines. Everything else under `types/xxx/`.

## Step order rationale

Order = increasing risk + dependency:

1. **Stats** — mechanical, no abstractions, unlocks #4.
2. **Listing move** — pure relocation.
3. **Format enums + detection** — bigger churn; breaks the back-import
   cycle so step 4's `ComposeCtx` doesn't have to thread `input::detect`
   types in awkward shapes.
4. **Compose bodies** — biggest move but lowest design risk once the
   shapes are settled.

Each step lands as its own PR / commit series so the diff stays
reviewable.

## Risks

- **`FileType` enum growth pressure.** Each new type still requires
  editing `FileType`. Acceptable: one line, and exhaustive matching is
  the feature, not the bug.
- **Detection priority ordering.** Currently implicit in detect.rs flow;
  becomes explicit in `DETECTORS` ordering. Document the ordering rule
  next to the list.
- **`ComposeCtx` shape.** Don't over-build it. Start with what the
  largest existing arm needs (theme manager, image config, peek_theme,
  args clone) and stop. Pass `&InputSource` and `&Detected` as direct
  parameters.
- **Test fixtures.** `info/gather/tests.rs` references stats shapes
  directly. Moves with the structs; rerun tests after each step.

## Follow-up

See [planned.md](planned.md) → "Type-support plugin trait" for the
optional next step where `Registry::compose_modes`'s match becomes a
trait-dispatch loop. Trade-off recorded there: lose single-file
dispatch overview, gain registration-list pluggability.
