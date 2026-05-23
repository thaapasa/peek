# Checkup findings

Codebase review notes from `/checkup` run on 2026-05-23. Architecture has good
bones — `compose_modes` is a real dispatch table, `InputSource::ByteSource`
streams, `Mode` trait carries `render_window` + `render_to_pipe` from one place.
Recent commits (paged-image unification, content-mode config struct, RenderCtx
dedup) show ongoing cleanup. Below: where parallel structure accreted real
duplication, plus abstractions earning less than they cost.

Finding IDs: `M2`–`M3`. IDs are stable — completed / dropped items are removed
but remaining IDs keep their numbers so references in commit messages / PRs stay
valid.

## Medium

### M2. `ComposeCtx` paid for by every per-type compose even though most don't use it

14 compose entries, only 4 (HTML, SVG, audio, csv) read `ctx` (for
`text_content_mode`). Other 10 take `_ctx: &ComposeCtx`. Real purpose is
`text_content_mode`; other two fields (`theme_manager`, `theme_name`) either
dead in most composes or available from `Registry`. Either (a) make
`text_content_mode` a free function taking args it actually needs and drop
`ComposeCtx`, or (b) accept it's a real shared bundle and stop apologising
for it across 10 signatures.

### M3. `Registry` carries `plain_mode` only to thread into `ComposeCtx.plain_mode`

Read at exactly two places: `svg/compose.rs:24`, `html/compose.rs:22`. Two
`if !plain_mode` branches deciding whether to push rendered-text view. Flag
could live on `Args` (already does — `args.plain`) and two compose sites
read directly. Then `ComposeCtx` shrinks to `theme_manager + theme_name`,
which collapses into `&Rc<ThemeManager>` since `theme_name` is
`tm.active_theme_name()`.

## File index

- `src/viewer/mod.rs` (Registry + ComposeCtx — M2, M3)
- `src/types/svg/compose.rs` (plain_mode reader — M3)
- `src/types/html/compose.rs` (plain_mode reader — M3)
