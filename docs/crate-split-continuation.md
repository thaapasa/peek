# Crate split continuation — reader/viewer layer

> **Status: in progress.** Extends the completed io/detect split
> ([archived/crate-split-plan.md](archived/crate-split-plan.md)). Goal: hoist the
> reader/viewer layer into `peek-foundation` + `peek-types` crates below the `peek`
> binary, so type modules (which parse untrusted file bytes) are barred by Cargo from
> reaching the event loop / terminal / process control — the same hardening rationale
> that justified `peek-detect`.

## Why

1. **Untrusted-input isolation.** Readers parse hostile bytes (fonts, PDFs, archives,
   disk images). A `peek-types` crate that *cannot* reach the event loop or process
   control contains a parser bug to a crate with no I/O-control surface. Cargo enforces
   it, same as it bars `peek-detect` from the reader layer.
2. **Isolated new types.** Adding a file type becomes a `peek-detect` entry + one
   `peek-types` module — no reach into the bin.
3. **Compile parallelism.** `types/` is ~38k lines; splitting lets it build parallel to
   the viewer infra.

## Target layering

```
peek-theme    leaf (terminal styling)            ← NEW, zero untangle
peek-io       leaf                                ✅ done
peek-detect   → io                                ✅ done
peek-foundation  info-base (InfoExtras trait + FileInfo + render entry + helpers)
                 + viewer toolkit (Mode trait, RenderCtx, shared modes:
                 content/hex/pretty/table/listing/paged, ui: wrap/Action/search/
                 ScreenBuffer/cell_size) + render vocab (image config/geometry +
                 ImageMode trait)
                 → peek-theme, peek-io                                   ← NEW
peek-types    per-type readers/info/view-mode → foundation, detect, io, theme   ← NEW
peek (bin)    compose_modes dispatch + info gather hub + event loop + main → types  ← stays
```

`peek-theme` sits parallel to `peek-io` (both leaves). `peek-foundation` depends on both.

## Baseline coupling (measured on `0e46e0e`, post-FileExtras→trait keystone)

```
info(prod) → types:  56 edges, ALL in gather/mod.rs   (the gather hub — moves UP to bin)
info/mod.rs, render/: 0                                 (clean — the trait keystone did this)
theme → crate::*: 0                                     (clean leaf)

types → viewer: ~140  (modes 48, listing 34, ui 17, paged 17, table 16…)  = shared toolkit, moves DOWN
viewer → types (real, non-test/doc):
   mod.rs 25 + ui/state.rs 1            → bin (compose_modes, ViewerState)   — fine, goes up
   paged.rs 15 + cell_size.rs 2         → foundation back-edge: render vocab  — KEYSTONE A
   modes/pretty_view.rs 2               → foundation back-edge: structured    — KEYSTONE B
   table/rows_mode.rs 3                 → test-only, relocate with the split
   table/row_source.rs 1, listing/row.rs 1 → doc comments, free
```

**Only three foundation-resident back-edges block the split: paged, cell_size, pretty_view.**
Nothing hidden — confirmed by measurement, not guessed.

### Key insight — the render vocab is genuinely shared

The image vocab paged/cell_size need (`TermSize, ImageConfig, FitMode, Background,
ImageMode, ZoomLevel, ScrollBounds, ViewBounds, ZoomPanState`) is imported by **seven**
type modules: comic, ebook, eps, font, image, pdf, svg — every image-rendering-backed
type — plus the shared `PagedImageMode`. It is cross-cutting render infrastructure, not
image-type-specific. Extracting it to foundation is its natural home, not a hack.

## Sequence (each step mergeable alone; boundary drawn last)

### Step 0 — `peek-theme` (do first)

Cleanest crate in the codebase; zero untangling, zero call-site churn.

- theme reaches `crate::*` **0 times** — true leaf. External deps only: syntect, two-face,
  crossterm.
- Assets embed via **relative** `include_str!("../../themes/…")` (not `CARGO_MANIFEST_DIR`),
  so moving `themes/` → `crates/peek-theme/themes/` keeps the path resolving identically.
  Themes are compiled into the binary — build-time only, invisible to release/install.

**Recipe:**
1. `crates/peek-theme/` + Cargo.toml (syntect, two-face, crossterm; inherit `rust-version`,
   edition, etc. from `[workspace.package]`).
2. Move `src/theme/*` → `crates/peek-theme/src/`, `themes/` → `crates/peek-theme/themes/`.
3. Workspace: add member + `peek-theme = { path = "crates/peek-theme" }` in
   `[workspace.dependencies]`; add `peek-theme = { workspace = true }` to the bin deps.
4. Root crate: replace `mod theme;` with `pub use peek_theme as theme;` → all 107
   `crate::theme::` call sites unchanged (mirrors the `src/input/` façade over
   peek-io/peek-detect).
5. Docs: update the `themes/` line + workspace note in `architecture-map.md`; CLAUDE.md
   workspace block.

**Risk:** trivial. **Verify:** `cargo build` + `cargo test` green, `crate::theme::` paths
still resolve through the alias.

### Step B — `pretty_view` → structured inversion (warmup)

`viewer/modes/pretty_view.rs` calls `crate::types::structured::pretty::pretty_print` +
`info::format_name` directly (2 edges). Invert: pass the pretty-print fn in as a
closure/trait param when the structured type composes its `PrettyView`, OR move the
structured pretty-printer into foundation. Prefer the closure — keeps the printer in
`types/structured`.

**Risk:** trivial, 1 file. **Verify:** structured pretty-print still works (existing tests).

### Step A — render vocab extraction (structural heart, highest risk)

Move the shared render vocabulary into a foundation-side module; the image engine imports
it back.

- **Scope:** `TermSize, ImageConfig, FitMode, Background, ImageMode` (in
  `types/image/pipeline/render.rs`, 766 lines — mixed vocab + engine), `ZoomLevel`
  (`zoom.rs`, 364), `ScrollBounds` (`scroll.rs`), `ViewBounds, ZoomPanState`
  (`zoom_pan.rs`). Separate the value types / `ImageMode` trait (→ foundation) from the
  rasterization engine (stays in `types/image`).
- **Verify before starting:** that `ImageMode`'s method signatures don't drag engine
  types into the vocab (if `render()` returns a rasterized-buffer type, that type comes
  along too — check and pull the boundary accordingly).
- Update the seven consuming type modules (comic, ebook, eps, font, image, pdf, svg) +
  `paged.rs` + `cell_size.rs` to import from the new foundation location.
- Relocate the `#[cfg(test)]` cross-boundary uses in `paged.rs` / `rows_mode.rs`.

**Risk:** highest — large module, generic plumbing. Do while context is fresh.
**Verify:** image/comic/pdf/font/eps/svg/ebook render + the paged image tests.

### Step C — hub relocation

- **Up to bin:** `info/gather/mod.rs` dispatch (56 edges — the `FileType → types::X::gather`
  match), `viewer/mod.rs::compose_modes`, the interactive event loop, `ViewerState`.
- **Down to foundation:** info base (InfoExtras trait, FileInfo, CompressionInfo, `render`
  entry, push_field/section helpers, time fmt), viewer shared modes + ui toolkit
  (wrap/Action/search/ScreenBuffer/cell_size/append_universal_modes).
- The per-type gather submodules stay in `types`; only the dispatch hub moves up.

**Risk:** medium, mostly mechanical once A/B land. **Verify:** full suite; info view +
mode composition for every type.

### Step D — draw the crate boundary

Split the foundation cluster + `types/` into `peek-foundation` + `peek-types` crates.
Edges already point one way after A–C, so this is near-mechanical: create the crates,
move modules, add a `src/` façade re-exporting under historical paths where churn would
otherwise be large (mirror `src/input/`).

**Risk:** low. **Verify:** `cargo build` + `cargo test`; confirm Cargo bars `peek-types`
from naming the bin (no event-loop/process-control symbols reachable).

## Notes

- Per CLAUDE.md docs hygiene: when this lands, either archive to `docs/archived/` with a
  completed-status blockquote (if the design rationale has lasting value) or delete.
- The `planned.md` "Type-support plugin trait" item already notes the info-render axis
  collapsed; the compose + detection axes there remain a *separate*, still-parked idea —
  this plan does not pursue trait-dispatch for those, only the crate boundary.
