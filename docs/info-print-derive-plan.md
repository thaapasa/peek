# Plan: info print-derive — unify info-section render + JSON from one struct

> **Status: Active plan (in progress).** Started 2026-06-06. Delete or archive when done.

## Goal

Collapse each info-extras type's two hand-written outputs (themed terminal
`render_section` + JSON `json_section`) into a **single Serialize view struct**
that drives both:

- **JSON** — already via `#[derive(serde::Serialize)]` (shipped).
- **Print** — via a new `#[derive(InfoSection)]` that walks the fields,
  pulling each row's *label* from a `#[info(label = "...")]` attribute and its
  *value* from an `InfoValue` trait impl on the field's type.

End state: one struct per flat section, both derives, hand `render_section`
deleted. The `Value` enum (already shipped) is where per-field formatting +
theming lives, so JSON gets the machine form and print gets the human form
from the same field.

## Progress

- ✅ **Steps 1–3 landed.** `peek-foundation-derive` crate (`#[derive(InfoSection)]`),
  the `InfoValue` / `InfoSection` / `MaybeZero` traits + `render_info_section`
  in `peek-foundation/src/info/section.rs`, `impl InfoValue for Value` (the
  size/count/timestamp colouring moved into `render/`), and `text` fully
  collapsed onto a `TextView` view struct that derives **both**
  `Serialize` + `InfoSection`. Verified byte-identical print + JSON (text and
  the SVG reuse path) against the prior commit. `TextStats` stays the
  streaming-gather accumulator (and the struct `svg` embeds); `TextView` is its
  presentation projection — for this type stats and view stay two structs
  because the accumulator is shared, but the label/skip/format logic is now
  declared once.
- ☐ **Step 4** — migrate the other flat sections (see Sequencing).
- ☐ **Step 5** — leave the bespoke sections documented as not-derived.

## Current state (already shipped on branch `info-json`)

- `peek x --info --json` ships: core `FileInfo` typed + every one of the 27
  per-type sections emits a typed object under its own key
  (`InfoExtras::json_section`, wired via the 3-arg `impl_info_extras!`).
- `info::Value` enum (peek-foundation, `crates/peek-foundation/src/info/value.rs`):
  `Size`/`Count`/`Int`/`Ratio`/`Timestamp`/`DurationMs`/`Text`/`Token`/`Bool`.
  `impl Serialize` emits the machine form (Size→number, Timestamp→ISO-8601 Z).
  **Print not implemented yet — this plan adds it.**
- Two exemplars already migrated off hand `json!` to serde view structs:
  - `text` (`types/text/info_render.rs`) — `TextJson` with `Value` fields.
    The "clear value" case.
  - `cert` (`types/cert/info_render.rs`) — `#[serde(tag = "kind")]` view enum
    + `skip_serializing_if`. The "complex / custom Serialize" case.
- The other 25 types still build their JSON with hand `json!` in their
  `json_section` fn. They keep working; migrate opportunistically.

## Locked design decisions

1. **Value render output = painted `String`.** `InfoValue::render_value(&self,
   theme) -> String` returns a themed SGR string, matching `push_field`'s
   existing `(label, colored_value)` contract. No structured cell type for now.
2. **Skip semantics: read serde's attrs.** The print derive parses each field's
   `#[serde(skip_serializing_if = "<path>")]` and mirrors it
   (`if !(<path>)(&self.field) { push row }`), so the skip condition is declared
   **once** for both outputs. A print-only `#[info(skip_if_zero)]` /
   `#[info(skip_if = "<path>")]` overrides only where print must diverge from
   JSON (e.g. `blank_lines`: JSON emits `0`, print hides it).
3. **Collapse stats + view into one struct.** The gathered `Extras` payload
   *becomes* the view struct (`Value` fields, both derives). Hand
   `render_section` deleted per type; `json_section` becomes
   `serde_json::to_value(self)`. Tests that `downcast_extras` and assert raw
   fields adapt to assert on `Value` / the view struct. Migrate per type, not
   big-bang.
4. **Section title via struct attr** — `#[info(title = "Content")]`.
5. **Scope: flat sections only.** Irregular sections keep a hand
   `render_section` for print and use serde for JSON. Do NOT try to derive:
   `cert` (per-entry `── Certificate #N` headers, `days_remaining` colour
   thresholds, one-of variants), `disk_image` (iso/dmg/raw one-of), `sqlite`
   (per-table rows), `font` (per-face rows), `css` (palette swatch grid),
   `vobject` (calendar/contact one-of). These stay manual print + serde JSON.

Compile-time cost of `syn` is accepted (build-time only; "no runtime deps"
holds).

## The shape to build

All in **peek-foundation** except the derive (its own proc-macro subcrate).

```rust
// crates/peek-foundation/src/info/  — the per-field render contract
pub trait InfoValue {
    fn render_value(&self, theme: &PeekTheme) -> String;   // themed SGR string
}

// what the derive generates
pub trait InfoSection {
    fn title(&self) -> &'static str;
    fn rows(&self, theme: &PeekTheme) -> Vec<(&'static str, String)>;
}

// shared driver — replaces every hand render_section for flat types
pub fn render_info_section<S: InfoSection>(lines: &mut Vec<String>, s: &S, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, s.title(), theme);
    for (label, value) in s.rows(theme) {
        push_field(lines, label, &value, theme);
    }
}
```

`Value: InfoValue` is where the bespoke colouring moves from
`info/render/file.rs`:
- `Value::Size`  → `format_size_human` + size-magnitude gradient (`size_color`)
- `Value::Count` → `thousands_sep` + log-intensity (`count_color` / `paint_count`)
- `Value::Timestamp` → local time (`format_time(_, false)`) + age gradient (`timestamp_color`)
- `Value::Token`/`Text` → value colour; `Value::Bool` → `"yes"`/`"no"`; etc.

Foundation also blanket-impls `InfoValue` for `String`, `&str`, `bool`,
integer types, `Option<T: InfoValue>` (None → see skip), `Vec<T: InfoValue>`
(comma-joined) so a view struct mixing `Value` and plain fields just works.
Custom value types in peek-types impl the foundation trait (orphan-legal:
local type, upstream trait).

### The derive: `#[derive(InfoSection)]`

New crate `crates/peek-foundation-derive` (`proc-macro = true`; deps `syn`,
`quote`, `proc-macro2`). peek-foundation depends on it and re-exports the
derive so call sites write `#[derive(InfoSection)]`.

Attributes:
- struct: `#[info(title = "Content")]` (required).
- field: `#[info(label = "Lines")]` (required, the print label).
- field: `#[info(skip_if_zero)]` / `#[info(skip_if = "<path>")]` (optional,
  print-only skip override).

Generated `rows()` per field, in declaration order:
1. Compute the skip predicate: prefer `#[info(skip_if*)]`; else the field's
   `#[serde(skip_serializing_if = "<path>")]`; else none.
2. `if <not skipped> { rows.push((<label>, self.<field>.render_value(theme))); }`

The derive must read the field's other-macro attrs (the `#[serde(...)]` token
stream) and parse out `skip_serializing_if`; ignore serde attrs it doesn't
recognise (`rename`, `skip`, etc. — `rename` is JSON-only, doesn't affect the
print label).

### Nested sub-objects

A field that is a nested struct (e.g. `metadata: DocMeta`) doesn't fit the
flat row model. For the first cut: such types stay hand-rendered, OR add an
`#[info(flatten)]` that makes the derive inline the sub-struct's rows. Start
WITHOUT flatten; add it only if a flat-enough type needs it.

## Sequencing

1. **Traits + Value print, no macro.** Add `InfoValue` + `InfoSection` +
   `render_info_section` to peek-foundation. Impl `InfoValue` for `Value`
   (move the `*_color` logic in) and the std blanket impls. Hand-write one
   `impl InfoSection for TextJson` and switch text's `render_section` to
   `render_info_section`. Prove print output is byte-identical to current via
   a render snapshot test (see Verification).
2. **Add the derive crate.** Replace the hand `impl InfoSection for TextJson`
   with `#[derive(InfoSection)]` + attrs. Confirm identical. Implement the
   serde-skip-reading.
3. **Collapse text fully.** Make gather emit `TextJson` as the Extras payload;
   delete the raw `TextStats` render path + hand `json_section`; adapt tests.
4. **Migrate the other flat sections** one at a time (structured, markdown,
   notebook, binary, classfile, directory, comic, eps, email, archive-core,
   pdf-core, …). Each: stats→view struct with `Value` + both derives, delete
   hand render + json, adapt tests, verify byte-identical JSON + snapshot print.
5. **Leave bespoke sections** (cert, disk_image, sqlite, font, css, vobject)
   on hand `render_section` + serde JSON. Document that they're intentionally
   not derived.

## Verification (per migrated type)

- **JSON parity:** capture `--info --json` before/after, assert byte-identical.
  Compare against a committed ref using a **`git worktree`** on that ref or a
  saved snapshot — **never `git stash`** (it corrupted the build cache + lost
  uncommitted work last time; see memory `feedback_no_git_stash_for_diffing`).
- **Print parity:** add a snapshot test that renders the info section (plain
  StyleMode to avoid SGR noise, or full and compare bytes) and asserts it
  matches the pre-migration output.
- **Gate (exactly what the project runs):**
  `just lint` = `cargo +nightly fmt -- --check` + `cargo check --workspace
  --all-targets` + `cargo clippy --workspace --all-targets -- -D warnings`,
  then `cargo test --workspace`. After heavy rebuild/branch churn,
  `cargo clean` first — a sub-second "Finished" recompiled nothing (stale cache
  reports false passes).

## Docs to update when done

- `docs/features.md` — note print + JSON share one model.
- `docs/planned.md` — the "Info JSON — collapse render + encode" entry: mark
  done / remove.
- `CLAUDE.md` file map — the new `peek-foundation-derive` crate + the
  `InfoValue`/`InfoSection` traits in `info/`.
- This plan file — delete or archive to `docs/archived/`.
