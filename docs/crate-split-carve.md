# Crate-split carve — execution playbook (peek-foundation + peek-types)

> **Status: ready to execute.** All design-level inversions are done (branch
> `reader-crate-split`, head `b6a9c92`, PR #16, green). This doc is the mechanical
> playbook for the remaining physical carve — no design decisions left. Companion:
> [crate-split-continuation.md](crate-split-continuation.md) (the why + the inversion history).

## Precondition (already true on the branch)

- `peek-theme`, `peek-io`, `peek-detect` are crates.
- Five inversions done (FileExtras→trait, pretty_view, image vocab, ComposeOpts, `text_content_mode` pretty).
- **Zero** non-hub, non-test `foundation→types` edges. Verify before starting:
  ```sh
  # both must print nothing:
  for f in $(grep -rl 'crate::types::' src/viewer --include='*.rs' | grep -v 'mod.rs'); do
    awk '/#\[cfg\(test\)\]/{t=1} /crate::types::/ && !/^\s*\/\//{ if(!t) print FILENAME": "$0 }' "$f"; done
  grep -rn 'crate::types::' src/info --include='*.rs' | grep -vE 'gather/|test|//'
  ```
- Only types-touching code: the `compose_modes` hub (`viewer/mod.rs`) and the `gather` hub
  (`info/gather/`), plus `extract/` (already bin-level), plus some `#[cfg(test)]` edges.

## Decisions locked in

- **Hubs go in the bin**, not `peek-types`. The three `FileType → types::X::*` dispatch matches
  (`compose_modes`, `gather`, `extract`) are session-orchestration glue. `peek-types` stays *pure
  parsers* — per-type modules with no FileType matching. The per-type `compose` / `extract` /
  `gather_extras` *functions* stay in `peek-types`; only the dispatch matches live in the bin.
- `ComposeCtx`, `ComposeOpts`, `Mode`, `RenderCtx` → foundation (types call them).
- `Registry`, `ViewerState`, the interactive event loop → bin.
- Foundation tests that drive the full pipeline (real types) relocate to the bin.

## Final crate layout

```
peek-theme   (leaf)                         ✅
peek-io      (leaf)                          ✅
peek-detect  → io                            ✅
peek-foundation → theme, io, detect
   info/      mod.rs (InfoExtras/FileInfo/Extras/helpers) + render/ + time.rs   [gather/ leaves]
   viewer/    cell_size, hex, highlight, image_render, listing, modes, paged,
              search, table, ui/ (minus state.rs), wrap_scroll, + mod.rs's
              ComposeCtx / ComposeOpts / Mode re-exports / image_config / is_lossy_pretty
   output/    PrintOutput
   base64.rs, xml.rs   (shared utils types depend on)
   input/     façade re-exporting peek-io + peek-detect (foundation's own copy)
peek-types → foundation, detect, io, theme
   types/     all per-type modules + info_impls.rs
peek (bin) → types, foundation, detect, io
   main.rs, cli.rs, update.rs
   gather/        (moved from info/gather)              — the FileType→gather match
   compose.rs     (Registry + compose_modes, moved from viewer/mod.rs) — the FileType→compose match
   extract/       (stays)                                — the FileType→extract match
   viewer_session: ViewerState (viewer/ui/state.rs) + interactive.rs (event loop)
```

### Three intra-unit splits (the fiddly part)

1. **`viewer/mod.rs`** splits: `ComposeCtx` / `ComposeOpts` / `text_content_mode` /
   `image_config` / `is_lossy_pretty` / `Mode` re-exports / `highlight_lines` re-export →
   stay foundation. `Registry` (struct + `new` + `compose_modes` + `compose_ctx` + `theme_name`
   + `peek_theme`) → bin `compose.rs`. Note `Registry::compose_ctx()` builds a foundation
   `ComposeCtx` — fine, bin→foundation.
2. **`info/`** splits: `mod.rs` + `render/` + `time.rs` → foundation; `gather/` → bin.
   `format_permissions_from_meta` (currently `pub(super)` in `info/mod.rs`) must become
   `pub(crate)`/`pub` so the relocated gather can call it.
3. **`viewer/ui/`** splits: primitives (Action, screen, prompt, status, term-size) → foundation;
   `state.rs` (`ViewerState`) → bin.

## Execution order (each step compiles; commit per step)

### C1 — relocate `gather` hub to bin (in-monolith)
- `git mv src/info/gather src/gather`; add `mod gather;` to `main.rs`; drop `mod gather; pub use gather::gather;` from `info/mod.rs`.
- In `src/gather/mod.rs`: `use super::{...}` → `use crate::info::{...}`; bump `format_permissions_from_meta` visibility.
- Callers `info::gather` / `crate::info::gather` → `crate::gather::gather` (main.rs ×3, state.rs ×4, the two foundation tests — see relocation below).
- Verify: build + test green.

### C2 — relocate `compose_modes` + `Registry` to bin (in-monolith)
- Move `Registry` (struct + impl incl `compose_modes`) from `viewer/mod.rs` → new `src/compose.rs`; add `mod compose;` to `main.rs`. Keep `ComposeCtx`/`ComposeOpts`/`text_content_mode`/`image_config`/`is_lossy_pretty` in `viewer/mod.rs`.
- `Registry::compose_ctx()` moves with Registry; it constructs the foundation `ComposeCtx` (uses `self.theme_manager` etc).
- Callers `viewer::Registry` → `crate::compose::Registry` (main.rs, state.rs).
- After C1+C2: `viewer/mod.rs` and `info/` are 100% types-clean (production). Verify.

### C3 — relocate the bin-bound viewer session pieces (prep, optional pre-D)
- `ViewerState` (`viewer/ui/state.rs`) and `interactive.rs` are bin. Can stay physically until D, but their `#[cfg(test)]` + production edges into `gather`/`compose`/`types` are bin→* (fine).

### D1 — create `peek-foundation` crate
- `crates/peek-foundation/` Cargo.toml (deps: peek-theme, peek-io, peek-detect + syntect, image, crossterm, unicode-width, bytes, anyhow, … — whatever `viewer`/`info`/`output` use). Inherit workspace package + `rust-version`.
- Move foundation modules (per the layout) into `crates/peek-foundation/src/`. `lib.rs` declares them + the **facade aliases** so intra-crate `crate::` paths survive:
  ```rust
  pub use peek_theme as theme;            // crate::theme:: keeps working
  pub mod input { pub use peek_io::*; pub use peek_detect as detect; /* match src/input/mod.rs */ }
  pub mod viewer; pub mod info; pub mod output;
  pub mod base64; pub mod xml;
  ```
- **Visibility:** every item `peek-types` (and the bin) reaches must be `pub` (not `pub(crate)`).
  Grep the ~167 viewer + ~56 info edge targets; bump them. Expect a long but mechanical pass —
  let the compiler enumerate via `error[E0603]` (private item).
- The bin keeps `crate::viewer`/`crate::info`/`crate::output` working via `pub use peek_foundation::{viewer, info, output};` + `pub use peek_theme as theme;` etc. in `main.rs` (mirror the existing `use peek_theme as theme` and `src/input/` facade).
- Verify: build + test.

### D2 — create `peek-types` crate
- `crates/peek-types/` Cargo.toml (deps: peek-foundation, peek-detect, peek-io, peek-theme + every parser dep currently in the bin's Cargo.toml that `types/` uses — object, cafebabe, rusqlite, pdfium-render, calamine, symphonia, ttf-parser, fontdue, mail-parser, x509-parser, … ). This is the big dep move.
- `git mv src/types crates/peek-types/src` (becomes the crate's modules). `lib.rs` facade aliases:
  ```rust
  pub use peek_foundation::{viewer, info, output};
  pub use peek_theme as theme;
  pub mod input { pub use peek_io::*; pub use peek_detect as detect; }
  pub mod types { /* re-export own modules, OR make the crate root == types */ }
  ```
  Simplest: make the crate root the `types` namespace and alias `pub use crate as types;` so
  intra-crate `crate::types::X` keeps resolving. (Confirm this resolves; otherwise add a `types`
  module that re-exports.)
- Bin: `pub use peek_types::types;` (or `as types`) so `crate::types::` works in the bin hubs.
- **Visibility:** bin reaches `peek-types` items (per-type compose/extract/gather_extras, mode
  constructors) — bump to `pub`. Compiler-driven.
- Verify: build + test. Confirm Cargo **bars** `peek-types` from the bin: it must not name
  `compose_modes` / `ViewerState` / `gather` / event loop. `cargo tree -i peek` should show
  peek-types does NOT depend on the bin.

### D3 — relocate foundation tests that drive the full pipeline
These call `gather` (bin) and/or real `types`, so they can't live in foundation:
- `src/viewer/paged.rs` `#[cfg(test)] mod tests` — uses `types::binary::BinaryInfo`, `types::comic::CbzPageRenderer` / `cbz`. Move to a bin-side or peek-types integration test (`tests/` or a bin module).
- `src/viewer/modes/content_tests.rs` — `crate::info::gather` (×2, lines ~46, ~438). Move to bin, or build a `FileInfo` fixture without the full gather.
- `src/viewer/table/rows_mode.rs` `#[cfg(test)]` (~1408) — `crate::info::gather`. Same.
- `src/viewer/ui/state.rs` `#[cfg(test)]` — already bin-bound (ViewerState is bin), travels with it.
- `info/gather/tests.rs` — travels with gather to the bin (C1).

Pattern: foundation keeps unit tests of pure mechanics (use synthetic fixtures / fakes — see the
`PrettyView` synthetic-closure tests + `content_tests::json_pretty` already in tree); full
detect→gather→compose integration tests live bin-side.

## Verification (each step + final)

- `cargo build`, `cargo clippy --all-targets`, `cargo test` green.
- Final boundary checks:
  - `cargo tree -i peek-types` — depends on foundation/detect/io/theme, **not** the bin.
  - `cargo tree -i peek-foundation` — **not** on peek-types.
  - No `peek-types` source names `compose_modes` / `Registry` / `ViewerState` / `interactive`.

## Gotchas seen this session

- **zsh word-split:** `for f in $files` does NOT split on spaces; use `while IFS= read -r f` or `${(f)var}`.
- **`include_str!` is file-relative**, not `CARGO_MANIFEST_DIR` — moving a crate shifts the `../` depth (bit us on `peek-theme`'s `themes/`).
- **`pub use` can't re-export a `pub(crate)` item** (E0365) — the facade re-exports force `pub` on the source.
- **Hidden foundation→types edges surface only at the boundary** — twice this session a method *looked* inverted but a foundation method still named `types` (the `text_content_mode` pretty closure). After the crates exist, the compiler finds the rest; expect a few more.
- **Trait/`Registry` inherent impls can't cross crates** — `compose_modes` must move *with* `Registry` (can't impl a foundation `Registry` from the bin).

## Docs to update when done

- `CLAUDE.md` workspace block (+ peek-foundation, peek-types).
- `docs/architecture-map.md` (crate breakdowns; the bin's `gather`/`compose`/`extract` hubs).
- `docs/architecture.md` "add a new type" (now: peek-detect entry + peek-types module + 3 one-line hub arms in the bin).
- Archive both `crate-split-continuation.md` + this file to `docs/archived/` with a completed-status note.
