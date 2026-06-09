# Listing mode generalization

> **Status: Steps 1–6 landed (branch `listing-generalization`), bar one
> deliberately-skipped piece (sqlite, below).** Generic engine + `ListSource`
> seam, directory folded in, email content-type column, object-file symbol →
> Hex jump, spreadsheet sheet source. Ready to archive once merged.

Split `ListingMode` into a generic **navigation engine** and a per-consumer
**row source**, so the listing UI (scroll / paging / selection / search / sticky
breadcrumb) stops dictating row shape and selection semantics. Consumers supply
their own columns and their own select action.

## Why

Two concrete pains today — not hypothetical-future:

1. **Spreadsheet + SQLite abuse the file model.** They build `Entry` values with
   faked file metadata: `size` = row count (not bytes), `.csv` / `.sql` name
   suffix as a kind tag, default `0o755` perms. Then `with_descend_handler`
   overrides the baked-in "select = extract sub-file" with "select = open a
   table view." The file model is already faked and the core selection semantic
   already broken for these consumers.

2. **`DirectoryMode` is a near-duplicate.** It re-implements flat
   selection/scroll and shares only the `row::` paint helpers with
   `ListingMode`. It can't reuse `listing::viewport` (474 lines of generic
   scroll / sticky / paging / reconcile) because that file is welded to
   `TreeRow`'s concrete file fields.

The generalization pays for itself by **folding `DirectoryMode` into the engine**
(reduces surface area — north-star #1). Email part/message listing and binary
symbol listing are the payoff on top, not the justification.

## Current shape (what exists)

- `crates/peek-foundation/src/viewer/listing/`
  - `mode.rs` — `ListingMode` (`TreeRow` rows + `descend_handler` escape hatch).
  - `viewport.rs` — `ListingViewport`: generic scroll/selection/sticky/paging/
    reconcile. **The valuable generic core.** Currently reads `TreeRow` fields
    (`parent_row`, `inner_path`) directly.
  - `entry.rs` — `Entry` / `EntryKind` / `EntryMtime` (file model).
  - `row.rs` — perms/size/mtime column paint (shared with `DirectoryMode`).
  - `build.rs` — `from_flat_paths` (flat slash-paths → `Entry` tree).
  - `stats.rs` — `Stats` aggregate for InfoMode.
- `crates/peek-foundation/src/viewer/modes/mod.rs` — relevant existing vocab:
  - `Position { Unknown, Byte(u64), Line(usize) }` — already the unit for
    symbol→hex jump. No new type needed.
  - `ExtractTarget { EntryPath(String), FrameIndex(usize) }`.
  - `DescendFrame { source, detected, modes, breadcrumb_label }`.
  - `Mode::extract_target` / `build_descend_frame` — the two selection exits today.

Consumers (`crates/peek-types/src/types/`): archive, pdf, document (docx/odt zip
+ rtf embeds), ebook (epub zip) → `Extract`; spreadsheet, sqlite → faked file
rows + `descend_handler` → `Frame`. `DirectoryMode` is separate.

## Target shape

### Engine owns one column

Keep `ListingViewport` as the engine. It needs **exactly one column** — the
selectable "name" — for search match, sticky breadcrumb leaf, and selection
highlight. Everything else is opaque pre-painted text the engine just lays out.
This is the "name + extras" cut: engine keeps `name` structured (so it can
overlay match-ranges + selection-bg + feed sticky); extras are painted by the
provider.

Per-row data the provider hands the engine:

```rust
/// One row in the listing, as seen by the generic engine. The provider
/// paints `left`/`right`; the engine paints `name` (match overlay +
/// selection bg) and lays the cells out.
pub struct RowCells {
    /// Tree connectors (`│ ` / `├╴` / `└╴`); `None` = flat row, no prefix.
    pub prefix: Option<String>,
    /// Pre-painted cells left of the name (perms, size, mtime / addr, type).
    pub left: Vec<String>,
    /// The one selectable column. Engine paints it.
    pub name: NameCell,
    /// Pre-painted cells right of the name.
    pub right: Vec<String>,
    /// Parent row index for the sticky breadcrumb; `None` = top-level / flat.
    pub parent: Option<usize>,
    /// False for container rows that scroll past but can't be selected
    /// (tree directories). Flat all-selectable sources set every row true.
    pub selectable: bool,
}

pub struct NameCell {
    /// Raw text; engine searches + sticky-displays this.
    pub text: String,
    /// Trailing marker the engine appends after paint (e.g. `/` for dirs).
    pub suffix: Option<char>,
    /// Semantic role so the engine picks accent (dir) vs fg (file) etc.
    pub role: NameRole,
}
```

(`left`/`right` as pre-painted `String` keeps column layout the provider's
business. If column alignment across rows must stay engine-driven, promote to a
`Cell { text, width, align }` later — start with pre-painted strings; the
archive/directory port will show whether alignment needs to move in.)

### Provider trait

```rust
pub trait ListSource {
    fn len(&self) -> usize;
    fn row(&self, idx: usize) -> RowCells;
    /// What happens on Enter. Provider owns the semantic.
    fn on_select(&mut self, idx: usize) -> SelectOutcome;
    /// Status segment label (was `format_name`): "ZIP", "directory", …
    fn source_label(&self) -> &str;
}
```

### Select outcome — the real unlock

Replace "extract by default + bolt-on `descend_handler`" with one provider
method returning a unified outcome. `descend_handler` is **deleted**, not
generalized — it stops being an override on a file model and becomes the
mechanism:

```rust
pub enum SelectOutcome {
    /// Archive / embed: extract to temp → detect → compose → push frame.
    Extract(ExtractTarget),
    /// Synthetic view over the current source (sqlite table, spreadsheet
    /// sheet). Was `descend_handler`.
    Frame(DescendFrame),
    /// Jump within the current file to another mode at a position
    /// (binary symbol → hex offset). NEW mechanism — see below.
    Jump { mode: ModeId, pos: Position },
    None,
}
```

`Position::Byte(u64)` already exists, so symbol → hex is `Jump { mode:
ModeId::Hex, pos: Position::Byte(off) }`.

## The one genuinely new mechanism — scope separately

`Extract` and `Frame` already work today (`extract_target` /
`build_descend_frame` exits). `Jump { mode, pos }` is new: it switches to a
**sibling mode of the same file** at an offset, rather than pushing a frame for
a sub-file. **Do not bundle Jump with the refactor** — land the engine/trait
first with only `Extract` + `Frame` (pure refactor, behavior-preserving), add
`Jump` as its own step.

## Findings (verified against code)

Read `listing/viewport.rs`, `directory/mode.rs`, `viewer_session/state.rs`
before writing. No surprises; plan holds. Specifics:

- **Step 1 cut is mechanical.** `ListingViewport` reads `TreeRow` via exactly:
  `inner_path.is_some()` (selectable), `inner_path.as_deref()` (selection key),
  `parent_row` (sticky), `rows.len()`. It never touches size/mode/mtime/leaf/
  prefix. Swap `&[TreeRow]` for a `RowMeta { parent: Option<usize>, selectable:
  bool }` view + key accessor. `first_file_row`/`next_file_row`/`last_file_row`
  →`*_selectable_row(predicate)`.
- **`selectable` is precisely the listing-vs-directory difference.** Listing:
  dirs not selectable (move skips them). Directory: every row selectable (Enter
  descends into subdir), incl. the synthetic `..`. One flag covers both — no
  special-casing in the engine.
- **`DirectoryMode` fold is a real win (~120 lines).** It re-implements
  `reconcile`/`max_top`/`move_selection`/`page_selection`/`jump`/search-reveal —
  flat copies of `viewport.rs`. Flat = engine with every row `parent: None`
  (sticky no-ops, `max_top` degenerates to `total - viewport`) + all-selectable.
  Two behavior diffs to settle on fold (pick one each, not blockers):
  - *Page*: listing snaps selection to first-in-content; directory steps
    selection by `viewport − 1`.
  - *Position*: directory does `tracks_position` + `Position::Line(top)`; engine
    must keep that.
- **`Jump` is expressible with one small helper.** `set_active(idx)`
  (`state.rs:687`) already does capture-outgoing → switch → `restore_position` →
  `set_position(pos, source)`; Hex `set_position(Byte)` + `mode_index(ModeId::
  Hex)` (`state.rs:897`) exist. Wrinkle: `set_active` calls `capture_position`
  first, clobbering `f.position` with the *outgoing* mode's pos — so can't
  pre-seed it. Add `set_active_at(idx, pos)`: capture outgoing (for return) →
  switch → force `f.position = pos` → restore. One fn, no new subsystem.
- **One dispatch site.** Descend/extract handling at `state.rs:549`
  (`build_descend_frame`) is where all three outcomes land: `Extract` → existing
  extract path, `Frame` → existing frame push, `Jump` → `set_active_at`.

## Sequence

1. ✅ **Extract the nav engine.** `ListingViewport` now runs off a `RowMeta`
   trait (`parent` + `selectable`) instead of `TreeRow` fields. (`cf870d2`)
2. ✅ **Introduce `ListSource` + `RowCells`.** `ListingMode` is a thin engine
   over `Box<dyn ListSource>` + the viewport; `TreeListSource` is the first
   impl, carrying the `Entry` tree + file columns. Steps 2 and 3 landed
   together — a trait with no impl isn't independently testable. (`7967b99`)
3. ✅ **Archive (and every tree consumer) ported.** `ListingMode::new` builds a
   `TreeListSource`, so archive / pdf / epub / docx / odt / comic / audio /
   notebook / disk-image / email / spreadsheet / sqlite are untouched. (`7967b99`)
4. ✅ **Folded the directory viewer in.** `DirectoryMode` → `DirListSource`
   (flat, all-selectable); ~120 lines of duplicated navigation deleted. (`b684c6c`)

   **Deviation from original plan:** the descend handler stays *engine*-side
   (`ListingMode` keeps `descend_handler` + `with_descend_handler`), not on the
   source. That kept all 12 consumers — including the sqlite/spreadsheet handler
   sites — unchanged, so steps 2–4 are a zero-churn, behavior-preserving lift.
   `SelectOutcome` was therefore *not* introduced: a unified enum is dead weight
   until the bin dispatches on it, which only pays off with `Jump` (step 6).

   **Side effect:** the mtime column width is now computed once over all rows,
   not per visible slice, so it no longer jitters on scroll.

### Product/feature work on the seam

6. ✅ **New consumers.**
   - **Email** — the attachments listing got its own `AttachmentListSource`
     showing each part's **content type** (the file-tree source rendered faked
     perms + an empty mtime). Threaded `content_type` through `Attachment`.
     (`9e9e510`)
   - **Object-file symbols** — `Mode::select_jump` /
     `ListSource::jump_target` add an *in-frame jump* select-semantic: a row can
     switch the active mode and seek it (vs extract / descend). The Symbols
     **table** became a `SymbolListSource` listing (same columns, now
     name-searchable) where Enter jumps the Hex view to the symbol's file
     offset, recovered via its section's file range. Bin: `jump_to_position`,
     checked before `build_descend_frame`. (`cc74f28`)

   The `Jump` outcome is a narrow defaulted method, *not* the `SelectOutcome`
   enum the early plan sketched: a unified enum only earns its place once the bin
   dispatches on it, and a single new method does that with far less churn. The
   descend handler stays engine-side, so `Extract` / `Frame` are unchanged.

5. **Honest select-table / select-sheet sources** (the listing TOC, not the grid
   contents):
   - ✅ **Spreadsheet** (flat) — `SheetListSource` shows bare sheet names instead
     of faked zero-size `Entry` rows named `Sheet1.csv`. The `.csv` suffix lives
     on only as the extract / descend key. (`cb74586`)
   - **SQLite — assessed, deliberately left as-is.** The schema listing is a
     *tree* (kind groups → entities, two rows per entity: `.sql` schema + `.csv`
     contents). A bespoke source would have to reimplement tree flattening
     (`├╴`/`└╴` connectors, parent indices) — work `TreeListSource` already does —
     purely to change one column's units (the contents row reuses `size` for the
     row count, a documented, intentional choice, not harmful fakery). That's
     duplicating the tree machinery for a cosmetic gain → overstretch. The Entry
     tree + engine descend handler is the right fit here; revisit only if SQLite
     grows a genuinely non-file column that the `size` slot can't carry.

The seam (steps 1–4) unblocked all of the above; each is an independent,
reviewable change.

## Guardrails

- **Don't merge with `TableMode` / `RowsTableMode`.** Different selection model
  (grid cells vs one selectable column + tree sticky + select→action). They
  already exist for grid data. The listing engine's distinguishing value is the
  single selectable column + tree breadcrumb + select→outcome. Keep separate.
- **`row.rs` (perms/size/mtime paint) is a file-shaped concern** — it moves to
  the directory/archive providers, out of the generic engine. The engine never
  names perms/size/mtime.
- **Start `left`/`right` as pre-painted strings.** Promote to aligned `Cell`
  only if the archive/directory port shows cross-row alignment must stay
  engine-driven. Don't pre-build the column abstraction.
- Keep `Stats` / InfoMode wiring working — providers that have aggregate stats
  (archive, directory) expose them; others don't.

## Docs to touch on landing

- `CLAUDE.md` file map — `listing/` description (engine vs source), `directory`
  no longer a bespoke mode.
- `docs/architecture.md` — listing engine + `ListSource` in the abstractions
  section; "adding a new listing consumer" note.
- `docs/features.md` — email part/message listing, binary symbol listing when
  those ship.
- This plan → delete (no lasting rationale) or archive if the Jump mechanism
  writeup is worth keeping.
