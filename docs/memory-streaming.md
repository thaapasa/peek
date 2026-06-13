# Memory & streaming strategy

How peek stays multi-GB-first-class. This is the single overview of the
size/streaming guards — the threat model, the mechanisms, and the rule
every new read path must follow. The per-class byte numbers live in
[`peek-io/src/limits.rs`](../crates/peek-io/src/limits.rs); the
member-by-member table is in
[architecture.md → Memory budgets](architecture.md#memory-budgets). This
doc is the *why* and the *decision rule*; that table is the *index*.

North stars (from [CLAUDE.md](../CLAUDE.md)): **stream, don't load** and
**multi-GB first-class**. A single open path must never load a whole
multi-GB file into RAM just to show the first screen.

## Two threats

Size alone isn't the axis. The real discriminator is
`(memory-bounded?) × (random-access?)`:

|            | Memory threat                                                                                                                                       | Latency threat                                                                                                                                                      |
|------------|-----------------------------------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **What**   | unbounded materialization — a whole-file slurp / parse / transform grows RAM with input size                                                        | a streaming, memory-bounded op that is *slow* because the source is non-seekable (a full decompress pass to list a `.tar.xz`, a deep seek into a compressed stream) |
| **Guard**  | a **hard cap** (refuse / degrade) or a **spill to tempfile** (bound RAM by a threshold, disk takes the rest) — never bypassable to the point of OOM | a **confirmation prompt** before the expensive op (interactive); the user may proceed                                                                               |
| **Status** | addressed by the budget classes + spill paths below                                                                                                 | partly open — see [planned.md → Large File Safeguards](planned.md#large-file-safeguards-)                                                                           |

A seekable, uncompressed format (ISO, plain tar, zip) is neither: random
access keeps both bounded. It needs no guard.

## Budget classes

Three classes name the *consumption shape*, not the feature. Every
hard-cap site aliases one (keeping its own domain-named constant). Defined
in `limits.rs`:

- **`WHOLE_DOC_BYTES` (32 MB)** — materialize **and transform** into a
  larger in-memory form (parse tree, styled lines). 5–20× expansion,
  blocks the UI during parse+highlight. Smallest class. *Members:*
  rendered whole-document views, structured pretty-print, notebook JSON.
- **`SIDECAR_PARSE_BYTES` (64 MB)** — materialize whole, derive something
  small (text stats, a header's fields). Same read cost, no expansion, so
  it tolerates more. *Members:* sidecar text parsers, the EPS header.
- **`BULK_WALK_BYTES` (256 MB)** — one bounded pass over untrusted /
  unbounded data, nothing proportional retained. *Members:* the batch
  decompress helper, per-entry archive extraction, the search scan, the
  object-file parse (real `.a`/`.so` run to hundreds of MB).

Membership is by *rationale*, not by number. A limit guarding a different
shape (per-record caps, pixel ceilings, count caps) stays local to its
site — it is **not** a class member.

## Mechanisms

### 1. Stream (preferred)

Consume a window or seek, never the whole file. `InputSource` exposes:

- `open_byte_source()` — random-access `ByteSource` (seek per read).
- `open_stream()` — sequential `ByteStream` (an `io::Read`: `io::copy`,
  `BufReader`, `.take()`).
- `open_line_source()` — anchor-indexed `LineSource` for line windows.

`ContentMode` (plain text / source) and `HexMode` render from these, so a
multi-GB text or binary file never materializes. This is the default —
reach for a cap only when streaming is genuinely impossible.

### 2. Capped whole read (hard refuse / degrade)

When a parse needs the whole slice in hand with random access (`object`,
the EPS binary header, a notebook's JSON tree), there is no streaming
option. Every whole-file read names a [`Budget`](../crates/peek-io/src/limits.rs)
— a cheap `byte_len` stat refuses *before* any allocation, letting the
caller degrade:

- `InputSource::read_bytes(Budget)` / `read_text(Budget)`
  (`peek-io/source.rs`). The capped budgets (`Budget::WholeDoc` /
  `Sidecar` / `BulkWalk`) alias the three classes by consumption shape and
  carry a `what` label for the over-cap message. Over the cap the caller
  degrades (Info-only, a streaming Source view, a dropped section).

There is no bare `read_bytes()` — the `Budget` argument is required, so an
unguarded slurp is not expressible. The one escape is
`Budget::Unbounded("why-safe")`, used only when the source is bounded by
construction; its `&str` names *why*, and is greppable.

### 3. Spill to tempfile (bound RAM, disk takes the rest)

For a one-pass transform of unbounded data where the *output* may be large
but must stay openable (decompression, archive extract), inflate into RAM
up to a threshold, then spill the remainder to a `NamedTempFile`. RAM is
bounded by the threshold regardless of output size; the produced
`InputSource` is then seekable from disk.

- Transparent decompression: `compression::decompress_to_source`, spilling
  past `DECOMPRESS_SPOOL_THRESHOLD` (16 MB). This is why a bare
  `bigdb.sqlite.xz` of any size opens the same way the identical entry
  inside a `.tar.xz` does.
- Archive entry extract: `materialise` in `archive/extract.rs`, spilling
  past `SPOOL_THRESHOLD` (16 MB).

Trade-off: the spilled path has no *disk* ceiling, so a decompression bomb
fills the tempdir (fails on `ENOSPC`) rather than refusing. Bounding that
is the job of the latency prompt — see planned.md.

### 4. Soft-degrade render cap (placeholder, not error)

Whole-document renders (HTML / DOCX / RTF / Markdown / notebook) build the
entire styled document in memory — no renderer streams. Over the cap they
don't error; they show a one-line placeholder pointing at the raw source /
hex view, so the file stays openable. This is a *different* path from the
hard-refusing capped read — it degrades the **view**, not the whole frame:

- `render_cap_placeholder(len, what, &mut warning)` / `render_cap_exceeded`
  / `ensure_under_render_cap` (`viewer/modes/rendered_text.rs`).

### Other gate helpers

- `read_zip_entry` (`archive/reader.rs`) — gated zip-entry payload read
  (declared *and* actual size).
- `gather_capped_text` (`text/info_gather.rs`) — capped whole-text read
  for sidecar parsers.
- `SearchState::scan_capped` (`viewer/search.rs`) — byte-budgeted scan over
  a streaming source.

## The decision rule

When you have an `InputSource` to consume, in order:

1. **Can you stream it?** (render a window, seek to an offset, count
   lines) → use `open_byte_source` / `open_stream` / `open_line_source`.
   Stop here. This is the answer for any view that scrolls.
2. **Must you materialize the whole thing** because the parse needs random
   access over the full slice? → `read_bytes(Budget)` / `read_text(Budget)`
   with the budget class that matches the *shape* (transform →
   `Budget::WholeDoc`, small-derived → `Budget::Sidecar`, one-pass →
   `Budget::BulkWalk`). Degrade over cap.
3. **Is it a one-pass transform of unbounded data** whose output must stay
   openable? → spill to a tempfile past a 16 MB threshold.
4. **Is it a whole-document render that can show a placeholder?** →
   `render_cap_placeholder`.

`Budget::Unbounded("why")` is allowed only when the source is *already
bounded by construction* (a small extracted entry under the spool
threshold, an in-memory frame you built, a non-File source whose other arm
streams, a read already gated by a `byte_len` check above). The `&str`
names which — it *is* the one-line justification, and it is greppable.

## Enforcement

The rule is type-enforced: `read_bytes` / `read_text` take a required
[`Budget`](../crates/peek-io/src/limits.rs), so a bare unguarded slurp does
not compile. The only escape is the explicit, greppable
`Budget::Unbounded("why-safe")`. Auditing the escapes is a single grep:

```sh
grep -rn 'Budget::Unbounded' crates src
```

Every hit must read as bounded-by-construction from its `&str` reason. The
original ~44-site audit that introduced this is done; new whole-file reads
inherit the gate by construction.
