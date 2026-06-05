# Crate split continuation — reader/viewer layer

> **Status: in progress — inversions done, carve pending.** Extends the completed io/detect
> split ([archived/crate-split-plan.md](archived/crate-split-plan.md)). Goal: hoist the
> reader/viewer layer into `peek-foundation` + `peek-types` crates below the `peek` binary,
> so type modules (which parse untrusted file bytes) are barred by Cargo from reaching the
> event loop / terminal / process control — the same hardening rationale that justified
> `peek-detect`.
>
> **This doc is the *why* + the inversion record.** The remaining mechanical execution
> (hub relocation + crate manifests) lives in
> [crate-split-carve.md](crate-split-carve.md) — that is the source of truth for the carve;
> do not duplicate execution detail here. Archive this doc (rationale has lasting value)
> when the carve lands.

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

**Done 2026-06-05.** Vocab moved to `viewer/image_render/` (config + image_mode +
zoom/scroll/zoom_pan); `types/image` re-exports it back at the old paths. Also surfaced a
*deeper* back-edge the measurement under-counted: `render_image_window` in `paged.rs`
drove the rasterization engine (`prepare_decoded` / `render_prepared*`), not just vocab.
It's only ever called by the type-side PDF/CBZ/EPS renderers, so it moved to
`types::image::paged_render` (engine-side), taking `PagedRender`/`RenderArgs` back from the
foundation. Result: **zero** production `viewer → types` edges outside the `compose_modes`
hub in `viewer/mod.rs` (step C). Remaining `paged.rs` `types` references are all
`#[cfg(test)]` (real PDF/CBZ renderers in PagedImageMode tests) — those must move to a
bin-side / types-side integration test in step D, since foundation tests can't reach
`types`.

### Step C0 — extract `ComposeOpts` (gates C)

**The blocker the original plan walked past (surfaced in review 2026-06-05).** The 23
`types/*/compose.rs` fns + `compose_modes` take `args: &crate::cli::Args` — a clap-derived,
bin-level struct. Step C moves `compose_modes` into `peek-types`, which would drag `Args` +
clap down into the parser crate. That's wrong: clap is a CLI concern and must stay in the
bin.

Resolution — a config view-model, the same inversion shape as step B. Measurement shows
compose reads exactly **12** plain `Args` fields (no clap behaviour, just values):
`line_numbers, raw, theme, color, width, plain, no_svg_anim, margin, language, image_mode,
edge_density, background`. (The `term/zoom/scroll_x/scroll_y/style_mode` reads in the image
path are `RenderArgs`, unrelated.)

- Define `ComposeOpts` in the foundation holding those 12 fields (plain values:
  `StyleMode` / `PeekThemeName` / numbers / strings — all foundation-reachable).
- The bin builds `ComposeOpts` from clap `Args` (a field copy) and passes `&ComposeOpts`
  into `compose_modes`.
- `compose_modes` + the 23 compose fns take `&ComposeOpts` instead of `&Args`. clap never
  leaves the bin.
- `viewer/mod.rs::image_config(&Args)` moves to `image_config(&ComposeOpts)` too.

**Risk:** low-medium — mechanical signature change across ~25 files, no behaviour change.
**Verify:** every type's compose path; CLI flags still take effect (`--raw`, `--line-numbers`,
`--theme`, `--image-mode`, etc.).

**Done 2026-06-05.** `ComposeOpts` (12 fields) defined in `viewer/mod.rs`; `Args::compose_opts()`
in the bin (`cli.rs`) is the projection seam. The `Registry` now *holds* the `ComposeOpts`, so
`compose_modes` dropped its `args` param entirely (reads `self.opts`) — and the two builder
closures (main + state) no longer capture a cloned `Args`. All 23 `types/*/compose.rs` take
`&ComposeOpts`; `types/` is now fully clap-free (`grep` confirms zero `cli::Args` references).

### Inversions complete (2026-06-05) — tree poised for the carve

All foundation→types *toolkit* edges are now removed. Verified: **zero** non-hub, non-test
`crate::types::` references in `viewer/` (outside the `compose_modes` hub in `mod.rs`) or
`info/` (outside the `gather` hub). The five inversions that got here:

1. `FileExtras` enum → `InfoExtras` trait (on `main`).
2. `pretty_view` → injected closure.
3. image render vocab → `viewer/image_render` + `render_image_window` → `types::image::paged_render`.
4. `Args` → `ComposeOpts` (clap out of compose).
5. `text_content_mode` pretty closure → `types::structured::pretty_view_for` (the gap step B left).

What remains is **purely mechanical**: relocate the two dispatch hubs, then draw the crate
manifests + facade aliases + visibility bumps. No more design work.

### Steps C & D — hub relocation + crate boundary → see the carve doc

The remaining work (relocate the `compose_modes` / `gather` hubs, create
`peek-foundation` + `peek-types`, facade aliases, `pub(crate)→pub` bumps, test
relocation) is mechanical and lives in **[crate-split-carve.md](crate-split-carve.md)** —
the execution source of truth. Not duplicated here to avoid drift.

> **Decision corrected:** an earlier draft of this doc put the three dispatch hubs *in
> `peek-types`*. That was reversed — the hubs are **session-orchestration glue and live in
> the bin**; `peek-types` stays pure parsers (per-type `compose`/`extract`/`gather_extras`
> functions, no `FileType` matching). The carve doc reflects the final decision.

## Notes

- Per CLAUDE.md docs hygiene: when this lands, either archive to `docs/archived/` with a
  completed-status blockquote (if the design rationale has lasting value) or delete.
- The `planned.md` "Type-support plugin trait" item already notes the info-render axis
  collapsed; the compose + detection axes there remain a *separate*, still-parked idea —
  this plan does not pursue trait-dispatch for those, only the crate boundary.
