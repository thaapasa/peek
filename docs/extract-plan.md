# Extract feature — plan

Save a sub-item out of a container file (animation frame, archive entry, ISO
entry) to disk or stdout. Designed so the same selection mechanism doubles as
the entry point for **recursive peek** later: `peek archive.zip --extract
foo.txt --print` views the inner file.

## North stars

1. One uniform shape: `trait Extract` on container types. Extract returns an
   `InputSource`, not opaque bytes — the result feeds straight back into the
   normal peek pipeline (info gather, hex view, line indexing).
2. Streaming preserved. ISO + uncompressed archive entries extract as a
   zero-copy `FileRange` view over the backing source — no decompress, no
   temp file, no full read. Compressed entries spool (Phase 2).
3. Read→write boundary stays narrow. Extract logic isolated in
   `src/extract/`. Per-type impls live next to their existing modules.

## Trait

```rust
// src/extract/mod.rs
pub struct Extracted {
    pub suggested_name: String,
    pub source: InputSource,
}

pub enum ExtractError {
    NotFound(String),
    InvalidKey(String),
    UnsafePath(String),
    Io(anyhow::Error),
}

pub trait Extract {
    fn extract(&self, key: &str) -> Result<Extracted, ExtractError>;
}
```

- `key` is a string. Each impl parses its own format (frame index for anim,
  entry path for archive/ISO).
- `Extracted.source` carries one of the new `InputSource` variants — caller
  doesn't care which.
- Trait is implemented only on types with inner items: animation, archive,
  ISO. Text/hex/markdown/etc. don't get it.

## InputSource changes

Switch in-memory backing to `bytes::Bytes` so slicing is zero-copy
(refcount bump, not memcpy). Collapse Stdin into a generic Memory variant.
Add a disk-file range variant for offset+limit views.

```rust
pub enum InputSource {
    File(PathBuf),
    Memory { bytes: Bytes, name: String },        // covers stdin + extracted memory
    FileRange { base: PathBuf, offset: u64, len: u64, name: String },
}
```

**Invariants**

- Range only over disk file. Slicing Memory uses `Bytes::slice` and stays a
  Memory variant (don't wrap in Range).
- Slicing a Range collapses offsets at construction (no nested Ranges).

**ByteSource impls**

- `File(path)` → existing `FileByteSource`.
- `Memory { bytes }` → `BytesByteSource(Bytes)`. Replaces `SliceByteSource`.
- `FileRange { base, offset, len }` → `RangeByteSource` wrapping a
  `FileByteSource`, translating offsets and clamping length.

**Migration**

- `input::stdin::build_source` → `InputSource::Memory { bytes, name: "<stdin>" }`.
- All `match InputSource::Stdin { data }` arms swap to Memory.
- `read_text` / `read_bytes` work uniformly across all three variants
  (FileRange reads via its ByteSource, no special-case needed).

## CLI

```text
peek <FILE> [--extract KEY] [-o OUTPUT | --output OUTPUT] [--print]
```

| Combo | Behavior |
|---|---|
| `peek anim.gif --extract 5` | Save frame 5 as PNG to suggested name |
| `peek anim.gif --extract 5 -o foo.png` | Save frame 5 to `foo.png` |
| `peek anim.gif --extract 5 --print` | Render frame 5 to stdout (ascii art) — recursive peek |
| `peek archive.zip --extract foo.txt` | Save inner entry to `foo.txt` |
| `peek archive.zip --extract foo.txt -o bar.txt` | Save to `bar.txt` |
| `peek archive.zip --extract foo.txt --print` | Recursive peek inner file |
| `peek iso.iso --extract /docs/big.txt --print` | Recursive peek over `FileRange` — no buffering |
| Pipe stdout (`peek ... --extract X | ...`) | Stream raw bytes to stdout |

`--extract` = "drill into sub-item". `--print` = "render to stdout".
`--output` = "write to this file". Three orthogonal axes.

`--frame` not added — `--extract N` covers it for animations.

## Per-extractor backing

| Extractor | Backing | Notes |
|---|---|---|
| Animation frame | `Memory(Bytes)` | Re-encode `DynamicImage` as PNG via `image` crate |
| ISO entry | `FileRange` over base | LBA × block_size + size, pure offset math |
| Zip stored | `FileRange` over zip | Phase 2 — needs `data_start()` exposure |
| Zip deflated | `Memory(Bytes)` (Phase 1) | Decompress to memory, < 64 MB threshold |
| Tar uncompressed | `FileRange` over tar | Phase 2 — entry header offset + size |
| Tar.gz/bz2/zst/xz | `Memory(Bytes)` (Phase 1) | Decompress to memory |
| 7z | `Memory(Bytes)` (Phase 1) | sevenz-rust2 per-entry decompress |

**Phase 1**: animation + ISO get zero-buffer extraction immediately. Archive
entries always spool to memory, regardless of compression. Caps at 256 MB
to prevent OOM on hostile input — error above that.

**Phase 2** (later): zip stored / tar uncompressed → FileRange paths.
Tempfile spool for big compressed entries (>64 MB).

## Path safety (archives)

Archive entries control their own paths. Reject:

- Absolute paths (`/etc/passwd`).
- Path traversal (`..`, including encoded variants).
- Symlinks pointing outside extraction root (when `-o` is a directory in
  Phase 2; Phase 1 only writes to a single explicit file).

Sanitize at the trait impl level — don't rely on caller. `ExtractError::UnsafePath`
on rejection.

Stdin-piped archive: works for in-memory extracts (entire archive already
in `Memory.bytes`). Stored entries → `Memory(bytes.slice(...))` zero-copy.

## Module layout

```
src/extract/
  mod.rs           — Extract trait, Extracted, ExtractError
  dispatch.rs      — extract_from_source: match detected type → call right impl
  write.rs         — write_extracted: source + output path | stdout
src/types/image/extract.rs       — Extract for GIF/WebP/SVG anim
src/types/archive/extract.rs     — Extract for zip/tar/7z (Phase 1: spool)
src/types/disk_image/extract.rs  — Extract for ISO (FileRange)
```

Each per-type extract module re-uses the type's existing readers (decoder,
backend list, walker). No duplicated parse logic.

## Phasing

- **Phase 1** (this branch): Memory + FileRange variants, Extract trait,
  animation frame impl (Memory), ISO impl (FileRange), archive impls
  (Memory spool), CLI flags, tests including recursive extract.
- **Phase 2** (future): zip stored / tar uncompressed → FileRange.
  Tempfile spool for big compressed entries. Interactive `s` keybinding
  in viewer modes (currently CLI-only).

## Open questions

- Animation frame format default: PNG (lossless, universal). User wants
  GIF? Phase 1 = PNG only.
- Archive entry suggested name: last path segment vs full inner path?
  Phase 1 = last segment.
- Frame index 0-based or 1-based? Match user-visible frame counter in viewer
  (1-based).
