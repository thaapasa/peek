# Memory retention audit

Snapshot of where peek holds bytes in memory vs streams them. Captured
2026-05-16, refreshed 2026-05-17 to reflect the `Bytes`-everywhere
refactor and the archive-extract tempfile spool.

North star #2 from `CLAUDE.md`: *stream, don't load*. This doc tracks
every place that violates the rule, whether the violation is bounded,
and what fixing it would look like.

## Streaming / safe

These do the right thing already — small constant or sublinear memory.

| Site                              | File                            | Notes                                                                                                                                                              |
|-----------------------------------|---------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Hex mode                          | `src/viewer/modes/hex.rs:57-72` | Viewport-only `read_range`. ~4 KiB.                                                                                                                                |
| Line index                        | `src/input/lines.rs:31-43`      | Anchor every 1024 lines, only byte offsets. ~16 KiB per 16 GiB. Backward scroll re-reads from nearest anchor.                                                      |
| Archive TOC                       | `src/types/archive/reader.rs`   | Metadata tree, never bodies.                                                                                                                                       |
| `InputSource::File` / `FileRange` | `src/input/source.rs`           | Random-access `ByteSource`, zero-copy slices for ranges.                                                                                                           |
| `InputSource::TempFile`           | `src/input/source.rs`           | Spooled large archive entries land here. `Arc<NamedTempFile>` shared with `TempFileByteSource`, RAII unlink on last drop. Reads via `FileByteSource` over the path. |
| Directory listing                 | `src/types/directory/read.rs`   | Single `fs::read_dir`, one level.                                                                                                                                  |

## Bounded buffers (hard cap)

Fully materialised but capped. Refuse / fall back above cap.

| Site                                               | File:line                                    | Cap                                              | Behaviour above cap                                                                                                                                                                                                          |
|----------------------------------------------------|----------------------------------------------|--------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Decompression                                      | `src/input/compression.rs:33`                | `MAX_DECOMPRESS_BYTES = 256 MiB`                 | Reject input. Streaming decoders lack bounded read API, so full materialise is unavoidable; resulting `Bytes` dropped after `resolve_transparent`.                                                                           |
| Archive entry extract (in-memory fallback)         | `src/types/archive/extract.rs:29`            | `MAX_EXTRACT_BYTES = 256 MiB` (memory path only) | Only applies when the spool path is skipped (entry < `SPOOL_THRESHOLD = 16 MiB`, or tempfile create failed and we fell back, or `--no-tempfile` was set). With `--no-tempfile` the cap is intentionally dropped — user opted in. |
| Pretty-print structured (JSON / YAML / TOML / XML) | `src/viewer/modes/content.rs:25`             | `PRETTY_MAX_BYTES = 16 MiB`                      | Fall back to raw streaming + warning. Below cap, holds **two** full vecs per `(theme, style_mode)`: `pretty_raw_lines` + `pretty_highlighted`.                                                                               |
| Stdin                                              | `src/input/stdin.rs` → `InputSource::Memory` | **No cap**                                       | Pipes non-seekable, so full slurp is the only option. Worth flagging.                                                                                                                                                        |

## Unbounded / monotonic growth

User-visible scroll / navigation can drive these to arbitrary size. No
eviction.

### 1. CSV record cache — worst offender

- `src/types/csv/parse.rs:82-85` — `CsvData::records: Vec<Record>` where each
  cell is owned `String`.
- Seed = first 1000 records (parse.rs:46).
- `ensure_record(idx)` (parse.rs:185-201) appends every record requested by
  scroll. Never evicts, never shrinks.
- Scroll-to-end on multi-GiB CSV ⇒ full file in memory as parsed records.
- UTF-16 transcode (parse.rs:328-368) eager, full-file before parse.

### 2. Document render cache — DOCX / ODT / RTF / HTML

- `src/types/document/read_mode.rs:35-44` (and html `RenderedMode`).
- Cache: `Vec<String>` of fully rendered lines, keyed by `(width, style_mode)`.
- No size cap. Source AST also fully materialised (zip + XML parse → `ast::Doc`).
- Resize → re-render → old vec dropped on overwrite, but no proactive eviction
  if multiple style modes have been visited.
- Large DOCX (hundreds of pages, embedded images noted by `[Image: ...]`) ⇒
  large line vec proportional to render output.

### 3. PDF text mode

- `src/types/pdf/text_mode.rs` — width-keyed full-document line cache. Same
  shape as document mode. No cap.

### 4. Paged-render cache — PDF / CBZ / EPUB

- `src/types/pdf/page_mode.rs:43-96`, `src/types/comic/cbz/read_mode.rs:48-70`,
  `src/types/ebook/epub/read_mode.rs:70-104`.
- `Vec<Option<CachedRender>>`, one slot per page / chapter, keyed by viewport
  `(page, cols, rows, style, image_config)`.
- Visited pages never evicted. Resize burns previous-key cache but slot stays
  allocated until next visit overwrites.
- Per-page render small (ASCII art ~5-50 KiB, EPUB chapter ~50 KiB), so
  practical ceiling is modest. 1000-page PDF × frequent resize ≈ 50 MiB worst
  case.

### 5. Audio visuals + lyrics

- `src/types/audio/package.rs:42-56` — `visuals: Vec<EmbedVisual>` raw image
  `Bytes`, `lyrics: Option<String>` joined across USLT/SYLT/LYRICS.
- No per-visual or total cap. Re-probed per call (no caching layer — see
  package.rs:10 comment).

## Re-buffering on `TempFile` sources (recursive descent)

The archive extract spool puts large entries in `InputSource::TempFile`,
which random-access reads handle without slurping. But the *outer*
archive reader for two of the five formats still slurps before walking:

- `extract_tar` (`src/types/archive/extract.rs`) calls `source.read_bytes()`
  on the outer source before iterating tar entries. Descending into a
  multi-GiB `outer.tar.gz` that itself lives in a spooled `TempFile`
  reloads the whole outer entry into `Bytes` for the walk. Inner-entry
  spool still works; peak memory during the walk is outer-entry size.
- `extract_cpio` — same pattern.

zip / 7z / ar walk the source via `open_seekable(&InputSource)` which
opens the `TempFile`'s on-disk path directly, so those three stay
disk-only across recursion. Documented as a planned improvement in
`docs/planned.md`.

## Notes on lifetime

- *Per-mode lifetime* = freed when user pops the mode off `ViewerState`'s
  stack. Mode-internal caches go with it.
- *Per-frame lifetime* (new): each `SessionFrame` holds its own
  `Arc<NamedTempFile>` via `InputSource::TempFile`. Popping the frame
  drops the Arc; last drop unlinks the file. Multiple nested spools each
  scope independently.
- *Per-process lifetime* — none currently. No global cache.
- Theme/style cycling rebuilds keyed caches but the prior `Vec` is only freed
  when the slot is overwritten with the new key.

## Suggested fixes by priority

| Priority | Site                                 | Fix                                                                                                                                        |
|----------|--------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| High     | CSV `records` Vec                    | Sliding window / ring around viewport; drop records far from current scroll. Alternative: hard ceiling + "showing first N records" notice. |
| High     | DOCX / ODT / HTML / RTF render cache | Cap analogous to `PRETTY_MAX_BYTES`; above cap → "too large for rendered view, raw source only".                                           |
| Medium   | EPUB + PDF + CBZ paged cache         | LRU cap (last N renders) keyed by viewport.                                                                                                |
| Medium   | tar / cpio re-buffer on TempFile     | Switch the outer walk to `open_byte_source()` so nested big-on-big stays disk-only.                                                        |
| Medium   | Audio visuals                        | Per-visual byte cap; reject oversized cover art early.                                                                                     |
| Low      | Pretty-print double-buffer           | Share raw vec between pretty and highlighter to halve footprint.                                                                           |
| Low      | Stdin slurp                          | Document the limit; consider spill-to-tempfile for huge stdin streams (mirror the archive extract path).                                   |

## Done since the previous snapshot

- **`Bytes` everywhere on read-only buffer APIs.** `InputSource::read_bytes`,
  `decompress_bytes`, `read_file_range`, archive `decompress_tar` /
  `in_memory_extract`, cpio `read_body`, `epub::read_entry`,
  `cbz::read_page`, audio `EmbedVisual.data` / `read_embed` all return
  `bytes::Bytes` instead of `Vec<u8>`. Kills the silent `.to_vec()` copy
  on the in-memory arm and makes "this is a copy" syntactically visible
  (callers must spell `.to_vec()`).
- **Archive extract tempfile spool.** Entries ≥ `SPOOL_THRESHOLD =
  16 MiB` (or with unknown declared size) land in
  `InputSource::TempFile` instead of a `Bytes` buffer. Removes the
  former 256 MiB hard cap for the common case and unblocks hex-dump /
  random-access viewers over multi-GiB archive entries.

## Methodology

Read-only audit across `src/input`, `src/types`, `src/viewer/modes`,
`src/extract`. Categories: decompression, format readers, view-mode caches,
extract spool. For each site: bounded? eviction? worst-case memory? Lifetime?
