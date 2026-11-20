# Memory and streaming strategy

This doc explains how peek treats multi-GB files as first-class input. It's the single overview of
the guards on size and streaming. It covers the threat model, the mechanisms, and the rule that
every new read path must follow. The byte numbers for each class live in
[`peek-io/src/limits.rs`](../crates/peek-io/src/limits.rs). The table of members is in
[architecture.md → Memory budgets](architecture.md#memory-budgets). This doc gives the _why_ and the
_decision rule_. That table is the _index_.

The north stars from [CLAUDE.md](../CLAUDE.md) are **stream, don't load** and **multi-GB
first-class**. Opening a single path must never load a whole multi-GB file into RAM only to show the
first screen.

## Two threats

Size alone isn't the deciding factor. What really tells the cases apart is whether the operation is
bounded in memory and whether the source has random access:

|            | Memory threat                                                                                                                                                                          | Latency threat                                                                                                                                                                                            |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **What**   | Unbounded materialization. Reading, parsing, or transforming the whole file makes RAM grow with the input size.                                                                        | A streaming operation that is bounded in memory but _slow_, because the source isn't seekable. Examples are a full decompression pass to list a `.tar.xz`, or a deep seek into a compressed stream.       |
| **Guard**  | A **hard cap** that refuses or degrades, or a **spill to a temporary file** that bounds RAM by a threshold and puts the rest on disk. No path around it can reach out of memory (OOM). | A **confirmation prompt** before the expensive operation, in interactive use. The user can choose to proceed.                                                                                             |
| **Status** | Handled by the budget classes and the spill paths described in this doc.                                                                                                               | Handled by load prompts for decompression and for the table of contents (TOC) of a compressed tar, and by the spill ceiling. See [planned.md → Large file safeguards](planned.md#large-file-safeguards-). |

A seekable, uncompressed format, such as ISO, plain tar, or zip, is neither threat. Random access
keeps both memory and latency bounded, so it needs no guard.

## Budget classes

The three classes name the _shape of consumption_, not the feature. Every hard-cap site aliases one
of them and keeps its own constant named for its domain. The classes are defined in `limits.rs`:

- **`WHOLE_DOC_BYTES` (32 MB)** is for code that reads the whole input **and transforms** it into a
  larger form in memory, such as a parse tree or styled lines. The expansion is 5–20×, and the UI
  blocks while it parses and highlights. This is the smallest class. _Members:_ rendered
  whole-document views, structured pretty-print, and notebook JSON.
- **`SIDECAR_PARSE_BYTES` (64 MB)** is for code that reads the whole input and derives something
  small from it, such as text statistics or the fields of a header. The read cost is the same, but
  nothing expands, so this class tolerates more. _Members:_ sidecar text parsers and the EPS header.
- **`BULK_WALK_BYTES` (256 MB)** is for code that makes one bounded pass over untrusted or unbounded
  data and keeps nothing proportional to it. _Members:_ the batch decompression helper, archive
  extraction for each entry, the search scan, and the object file parse. Real `.a` and `.so` files
  reach hundreds of MB.

A limit belongs to a class because of its _rationale_, not its number. A limit that guards a
different shape stays local to its site and is **not** a class member. Examples are caps for each
record, pixel ceilings, and count caps.

## Mechanisms

### 1. Stream (preferred)

Consume a window or seek. Never consume the whole file. `InputSource` exposes these methods:

- `open_byte_source()` returns a random-access `ByteSource`, which seeks for each read.
- `open_stream()` returns a sequential `ByteStream`. It's an `io::Read`, so it works with
  `io::copy`, `BufReader`, and `.take()`.
- `open_line_source()` returns an anchor-indexed `LineSource` for windows of lines.

`ContentMode`, for plain text and source code, and `HexMode` render from these, so a multi-GB text
or binary file is never read whole into memory. The LMDB reader walks B+tree pages through a
`ByteSource` in the same way. Info, rows, and value extracts read single pages, never the whole
file. Streaming is the default. Use a cap only when streaming is truly impossible.

### 2. Capped whole read (hard refuse or degrade)

Some parses need the whole slice in hand with random access, such as `object`, the binary header of
an EPS file, or the JSON tree of a notebook. They have no streaming option. Every whole-file read
names a [`Budget`](../crates/peek-io/src/limits.rs). A cheap `byte_len` stat refuses the read
_before_ any allocation, and the caller can then degrade:

- `InputSource::read_bytes(Budget)` and `read_text(Budget)` live in `peek-io/source.rs`. The capped
  budgets, `Budget::WholeDoc`, `Sidecar`, and `BulkWalk`, alias the three classes by shape of
  consumption. Each carries a `what` label for the message shown over the cap. Over the cap, the
  caller degrades. It can show only Info, show a streaming Source view, or drop a section.

There is no bare `read_bytes()`. The `Budget` argument is required, so you can't express an
unguarded whole-file read. The one escape is `Budget::Unbounded("why-safe")`, which you use only
when the source is bounded by construction. Its `&str` names _why_ the read is safe, and you can
find it with grep.

### 3. Spill to a temporary file (bound RAM, disk takes the rest)

This mechanism is for a one-pass transform of unbounded data, where the _output_ can be large but
must still open. Decompression and archive extraction are examples. The transform inflates into RAM
up to a threshold and then spills the rest to a `NamedTempFile`. The threshold bounds RAM, whatever
the size of the output. The resulting `InputSource` is then seekable from disk.

- Transparent decompression goes through `compression::decompress_to_source` and spills past
  `DECOMPRESS_SPOOL_THRESHOLD` (16 MB). This is why a bare `bigdb.sqlite.xz` of any size opens the
  same way as the identical entry inside a `.tar.xz`.
- Archive entry extraction goes through `materialise` in `archive/extract.rs` and spills past
  `SPOOL_THRESHOLD` (16 MB).

**Disk ceiling.** Both spill paths cap the bytes written to disk at `compression::MAX_SPILL_BYTES`
(8 GiB) and fail cleanly past it. A decompression bomb then gets refused, instead of filling the
temporary directory (`ENOSPC`). The ceiling is generous, so large legitimate files open. It's
_always_ enforced, including on non-interactive paths, such as a pipe or `--print`, where no prompt
can step in.

In interactive use, two prompts add to this. A large extract asks for confirmation before it spools
anything (`EXTRACT_PROMPT_BYTES`, 256 MB). A big transparent decompression waits behind a load
prompt (`LATENCY_PROMPT_BYTES`, 50 MB compressed). For details, see
[the safeguards plan](archived/large-file-safeguards-plan.md).

### 4. Soft-degrade render cap (placeholder, not error)

Whole-document renders for HTML, DOCX, RTF, Markdown, and notebooks build the entire styled document
in memory. None of these renderers streams. Over the cap, they don't fail. They show a one-line
placeholder that points at the raw source view or the hex view, so the file still opens. This path
differs from the hard-refusing capped read. It degrades the **view**, not the whole frame:

- `render_cap_placeholder(len, what, &mut warning)`, `render_cap_exceeded`, and
  `ensure_under_render_cap` live in `viewer/modes/rendered_text.rs`.

### Other gate helpers

- `read_zip_entry` (`archive/reader.rs`) reads the payload of a zip entry behind a gate. It checks
  both the declared size _and_ the actual size.
- `gather_capped_text` (`text/info_gather.rs`) is a capped whole-text read for sidecar parsers.
- `SearchState::scan_capped` (`viewer/search.rs`) scans a streaming source within a byte budget.
- The image decode and sniff code has non-File arms (`image/pipeline/`, `image/info_gather.rs`).
  These call `ensure_under_render_cap` or `render_cap_exceeded` on `byte_len` before they read the
  buffer. The File arm streams from the path without a cap. A non-File source must be buffered whole
  to decode, and a spilled `TempFile` or an archive entry can be GB in size, so the render cap gates
  it. This asymmetry is by design.
- The AVIF decode hook (`image/avif.rs`) buffers on every arm, File included, because AV1 needs the
  whole file. `render_cap_exceeded` on the stream length gates it first.

## The decision rule

If you have an `InputSource` to consume, ask these questions in order:

1. **Can you stream it?** Streaming covers rendering a window, seeking to an offset, and counting
   lines. If you can, use `open_byte_source`, `open_stream`, or `open_line_source`, and stop here.
   This is the answer for any view that scrolls.
2. **Must you read the whole thing into memory**, because the parse needs random access over the
   full slice? If so, use `read_bytes(Budget)` or `read_text(Budget)` with the budget class that
   matches the _shape_:
   - For a transform, use `Budget::WholeDoc`.
   - For deriving something small, use `Budget::Sidecar`.
   - For a single pass, use `Budget::BulkWalk`.

   Degrade over the cap.

3. **Is it a one-pass transform of unbounded data** whose output must still open? If so, spill to a
   temporary file past a 16 MB threshold.
4. **Is it a whole-document render that can show a placeholder?** If so, use
   `render_cap_placeholder`.

`Budget::Unbounded("why")` is allowed only when the source is _already bounded by construction_.
Examples are a small extracted entry under the spool threshold, an in-memory frame you built, a
non-File source whose other arm streams, and a read that an earlier `byte_len` check already gates.
The `&str` names which case applies. It _is_ the one-line justification, and you can find it with
grep.

## Enforcement

The type system enforces the rule. `read_bytes` and `read_text` take a required
[`Budget`](../crates/peek-io/src/limits.rs), so an unguarded whole-file read doesn't compile. The
only escape is the explicit `Budget::Unbounded("why-safe")`, which grep can find. To audit the
escapes, run one grep:

```sh
grep -rn 'Budget::Unbounded' crates src
```

The `&str` reason of every hit must show that the read is bounded by construction. The original
audit of about 44 sites that introduced this rule is done. New whole-file reads get the gate by
construction.
