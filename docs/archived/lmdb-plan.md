# LMDB support plan

> **Status: Completed 2026-10-12.** Archived for reference.

Issue #21. View LMDB databases: env info, named sub-databases, key/value rows. Read-only, streaming,
no new dependencies.

## Decisions

- **Hand-written page walker over `ByteSource`, not liblmdb (`heed` / `lmdb-rkv`).** liblmdb mmaps
  the whole map and creates or writes `lock.mdb` / `<file>-lock` even for read-only opens (seen while
  building the fixtures: `mdb_stat` recreated both lock files). That breaks "stream, don't load",
  touches a live app's lock table, and adds C code to the hostile-bytes layer. The on-disk format is
  small: 2 meta pages + a B+tree of branch / leaf / overflow pages.
- **64-bit little-endian, 0.9 and 1.0.** The plan assumed 0.9 only. LMDB 1.0.2 (Aug 2026,
  Homebrew's default) writes data version 3 with a different page layout, and the first
  fixtures came out in it, so the reader carries a `Variant` for the three differences:
  24-byte page header, `lower` / node offsets relative to the header end, node data after an
  even-padded key. 1.0 also stores named-DB names NUL-terminated. 32-bit, big-endian and
  encrypted environments are detected but not walked.
- **Single file in, as always.** `peek data.mdb` or `peek foo.lmdb` (no-subdir file). `peek db/`
  lists the directory and Enter on `data.mdb` descends, which already works. The lock file is never
  opened.

## Format notes

From `lmdb.h` / `mdb.c` (0.9.x), offsets for a 64-bit build:

- Page header (16 B): `pgno u64 · pad u16 · flags u16 · lower u16 · upper u16`. Overflow pages
  reuse `lower+upper` as `pages u32`. Flags: `BRANCH 0x01 · LEAF 0x02 · OVERFLOW 0x04 · META 0x08 ·
  LEAF2 0x20 · SUBP 0x40`. Entry count = `(lower - 16) / 2`; the `u16` node offsets start at 16.
- Meta (pages 0 and 1, body at 16): `magic u32 = 0xBEEFC0DE · version u32 = 1 · addr u64 ·
  mapsize u64 · dbs[FREE, MAIN] (48 B each) · last_pg u64 · txnid u64`. Use the meta page with the
  higher `txnid`; fall back to the other if it fails validation.
- DB record (48 B): `pad u32 · flags u16 · depth u16 · branch_pages · leaf_pages · overflow_pages ·
  entries · root` (all `u64`; `root = !0` means empty). `dbs[FREE].pad` holds the page size, so
  page 1 sits at `psize`, not at a fixed 4 KiB.
- Node: `lo u16 · hi u16 · flags u16 · ksize u16 · key · data`. Branch: child
  `pgno = lo | hi<<16 | flags<<32`. Leaf: `dsize = lo | hi<<16`; `F_BIGDATA 0x01` means the data is
  a `u64` overflow pgno, `F_SUBDATA 0x02` means a 48 B DB record, and `F_DUPDATA 0x04` means a
  dupsort sub-page (inline) or a sub-tree (with `F_SUBDATA`).
- Named DBs are MAIN leaf nodes with `F_SUBDATA` (without `F_DUPDATA`). Their key is the name. MAIN
  can also hold plain keys next to them.
- DB flags: `REVERSEKEY 0x02 · DUPSORT 0x04 · INTEGERKEY 0x08 · DUPFIXED 0x10 · INTEGERDUP 0x20 ·
  REVERSEDUP 0x40`. Custom comparators aren't stored, but in-order traversal doesn't need them.

## Phases

One commit each. Each phase leaves the tree green.

### 1. Detection

- `FileType::Lmdb` in `peek-detect/src/detect.rs`, plus `types/lmdb.rs` with `sniff_magic(head)`:
  page 0 has flags `META`, the magic at offset 16, version 1, and `dbs[FREE].pad` is a power of two
  in 512..=65536. Called from `head_magic_mime` next to `ds_store_detect::sniff_magic`. The 8 KB
  head covers it.
- No extension rule. `.mdb` is also MS Access, and LMDB names are arbitrary, so magic decides.
- Tests: both fixtures; a `.mdb` name over non-LMDB bytes doesn't classify as LMDB; truncated
  heads; the 16 KiB and 4 KiB page-size variants.

### 2. Page walker (`peek-types/src/types/lmdb/reader.rs`)

- `LmdbEnv::open(ByteSource)`: read and validate both metas and pick the active one. Expose `psize`,
  `mapsize`, `last_pg`, `txnid`, the FREE / MAIN records and the file length.
- `read_page(pgno)`: one `read_range(pgno * psize, psize)`, bounds-checked against the file length
  (not `last_pg`, which can point past a short file).
- `named_dbs()`: walk MAIN and collect `F_SUBDATA` nodes as `(name, DbRecord)`.
- `Cursor`: in-order leaf walk with an explicit branch stack. Yields `(key, ValueRef)`, where
  `ValueRef` is `Inline(Bytes)` or `Overflow { pgno, len }`. Dupsort nodes expand into one item per
  dup value (sub-page or sub-tree). `LEAF2` pages yield fixed-size keys.
- Hostile-input guards, same style as `ds_store`: depth cap 32, visited-page set per walk, page
  type checks (branch child must be branch/leaf, not meta/overflow), node offsets inside
  `lower..psize`, overflow run inside the file. Errors become a truncation flag, never a panic.
- Tests: fixture counts match `mdb_stat` (`users` 600, `tags` 30, `blobs` 4, event-log 24); the
  40 KB `blobs` value reads back intact; corrupted copies built in memory (cycle, out-of-range pgno,
  bad offsets) stop cleanly.

### 3. Info section

- `info_gather.rs` + `info_render.rs`, derive mode. Environment block: page size, map size, file
  size, used pages (`last_pg + 1`), last txnid, active meta (0/1). One nested block per DB (MAIN,
  then each named DB): flags as tags, depth, branch / leaf / overflow pages, entries. FREE DB tree
  stats shown as "Free list".
- Wire the `src/gather/mod.rs` arm and the `info_impls.rs` row. `--info --json` comes out of the
  same struct.
- Reads metas plus MAIN's leaves only. No full-tree walk, so cost stays constant on multi-GB files.

### 4. Database list and row table

- `compose.rs`: when named DBs exist, land on a `ListingMode` with one row per DB: `(main)` first,
  then names, each with its entry count. Enter descends to that DB's table, using the same
  descend-handler pattern as `sqlite/compose.rs`. With no named DBs, land straight on MAIN's table.
- `row_set.rs`: `LmdbRowSet: RowSource`. Columns `Key · Value · Size`. `total()` comes from the
  DB record's `entries`, so it is definite without a walk.
- Random access: a lazily grown **leaf index** `Vec<(pgno, first_row)>`, filled as the cursor
  passes each leaf. A jump to row N walks forward from the nearest indexed leaf. For non-dupsort
  DBs a skipped leaf costs a 16 B header read (`lower` gives its count). Dupsort leaves have to be
  read whole to count their dups. A sliding window of rows like `SqliteRowSet`, centred on the
  request.
- Cell format: key and value as text when valid UTF-8 without control characters, else hex
  (`de ad be ef …`, capped). INTEGERKEY keys decode as `u32` / `u64` LE numbers. Overflow values
  preview from their first page only, so a 1 GB value costs one read. Size is the full byte length.
- Print / pipe mode emits the table, streamed through the existing `RowsTableMode` print path.

### 5. Open a value (Enter / `e`)

Shows a JSON value as JSON, a PNG value as an image, and so on through the normal re-detect.

**Blocker:** `RowsTableMode` has no row cursor, so a row can't be selected. `ListingMode` has
selection, but `ListSource` takes `&self` with every row resident, which breaks on large DBs.
Options:

- **(a) Add an optional row cursor to `RowsTableMode`** (recommended). A highlight row, Up/Down
  moving it, `extract_target()` / `extract_size()` delegated to a new defaulted `RowSource` hook.
  It stays off for CSV and SQLite unless their sources opt in. This is a foundation change, so it
  needs its own commit and help-screen entry.
- (b) Defer: phase 4 ships view-only, and values are opened by extracting from a future
  `--extract <db>/<key>`.

With (a), the payload source is `InputSource::subrange` (a `FileRange`, no copy) for overflow values,
since those are contiguous on disk, and `Memory(Bytes)` for inline values. Inner-path key:
`<db>/<key>`, with binary keys hex-encoded behind an `x:` prefix and run through
`sanitize_entry_path`. Wire the `src/extract/extract.rs` arm.

### 6. Docs

`docs/features.md`, a manual chapter section next to SQLite, README feature list, the CLAUDE.md
types list, and `//!` headers on the new files. `docs/memory-streaming.md`: note that LMDB reads
pages only and has no whole-file path.

## Fixtures

- `test-data/catalog.lmdb` (LMDB 1.0) and `catalog-0.9.lmdb` (same data, 0.9): no-subdir
  layout, 16 KiB pages. MAIN has 2 keys plus the named DBs `users` (600 JSON values, depth 2),
  `tags` (dupsort, 5×6), `blobs` (binary, one 40 KB overflow value) and `counters`
  (INTEGERKEY).
- `test-data/event-log.lmdb/data.mdb` (1.0): directory layout, unnamed INTEGERKEY DB, 24
  entries.
- Missing: a **4 KiB-page** fixture. liblmdb uses the OS page size, so it has to be built on
  x86_64 or Linux (`mdb_load` in Docker). Also missing: a `DUPFIXED` sub-database (`LEAF2` pages).
  Generator scripts are currently outside the repo; commit them under `scripts/` if the fixtures
  need to be reproducible.

## Answered questions

1. Dupsort rows flatten: one row per `(key, value)`.
2. With no named DBs, peek lands on MAIN's table.
3. Phase 5 took option (a): an opt-in row cursor in `RowsTableMode`.

## Fixture notes

- `mdb_load` keeps `format=print` across sections, so a later `format=bytevalue` section
  loads hex digits as literal text. Generate every section in `print` format.
- 0.9 `mdb_load` doesn't persist `integerkey` on MAIN; integer keys are covered by the
  `counters` named DB instead.
- `catalog-0.9.lmdb` was built with 0.9.36 tools compiled from `mdb.RE/0.9`.
