use std::rc::Rc;

use anyhow::Result;

use super::{Handled, Mode, ModeId, RenderCtx, Window};
use crate::input::detect::StructuredFormat;
use crate::input::{InputSource, LineSource};
use crate::output::PrintOutput;
use crate::theme::{ColorMode, PeekThemeName, ThemeManager};
use crate::viewer::structured;
use crate::viewer::ui::Action;
use crate::viewer::{LineStreamHighlighter, highlight_lines};

/// Pretty-printing the structured form of a file requires holding the
/// whole document — there's no streaming JSON pretty-printer. Reads above
/// this size fall back to the raw streamed view with a warning so we
/// don't OOM on a multi-GB log shaped like JSON.
const PRETTY_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Content view: text, syntax-highlighted source, pretty-printed structured
/// data, or SVG XML source.
///
/// Raw mode streams from a `LineSource` (anchor-indexed line iterator
/// over the input). Each `render_window` fetches just the visible window
/// of lines; multi-GB files never load into memory. With a syntax token,
/// a forward-only `LineStreamHighlighter` is driven across the window;
/// backward scrolls past its current cursor reset and replay from line 0.
///
/// Pretty mode requires the full document and so caps file size at
/// `PRETTY_MAX_BYTES`. Above the cap we push a warning, force `use_pretty
/// = false`, and the user sees the raw (streamed) source. Below the cap
/// the parsed-and-pretty-printed text is cached on first access; if a
/// syntax token is set, the cached pretty form is also re-highlighted as
/// a single batch (bounded by the cap).
///
/// `r` flips `use_pretty` when `allow_pretty_toggle` is set — used for
/// structured files (JSON/YAML/TOML/XML) where raw vs pretty is a
/// meaningful user choice. SVG XML opts out so `r` instead falls through
/// to cycling between SVG-rasterized and SVG-XML.
pub(crate) struct ContentMode {
    source: InputSource,
    line_source: LineSource,
    /// Forward-only syntect feeder for raw-mode highlighting. `None` when
    /// the view has no associated syntax (plain text, --plain mode).
    highlighter: Option<LineStreamHighlighter>,

    /// Format to pretty-print as, when one applies. None means "no pretty
    /// form available" (source code, plain text).
    pretty_target: Option<StructuredFormat>,
    /// Lazy pretty-print result. `None` = not yet attempted; `Some(Ok)` =
    /// cached pretty text; `Some(Err)` = parse failure (the matching
    /// warning was already pushed to `pending_warnings`).
    pretty: Option<Result<String, String>>,
    /// Highlighted-pretty cache, keyed by the (theme, color) tuple it was
    /// rendered for. Pretty content is bounded by `PRETTY_MAX_BYTES` so
    /// holding one materialized `Vec<String>` is acceptable. Invalidated
    /// whenever the active theme/color tuple changes.
    pretty_highlighted: Option<(PeekThemeName, ColorMode, Vec<String>)>,
    /// Cached split of pretty content into raw lines (no syntax). Same
    /// reasoning as `pretty_highlighted` — bounded by the cap.
    pretty_raw_lines: Option<Vec<String>>,

    /// Warnings produced during render that haven't been collected by
    /// `ViewerState` yet — drained on every `take_warnings` call.
    pending_warnings: Vec<String>,
    syntax_token: Option<String>,
    theme_manager: Rc<ThemeManager>,
    use_pretty: bool,
    allow_pretty_toggle: bool,
    label: &'static str,
}

const RAW_TOGGLE_ACTIONS: &[(Action, &str)] = &[(Action::ToggleRawSource, "Toggle raw / pretty")];

impl ContentMode {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source: InputSource,
        line_source: LineSource,
        pretty_target: Option<StructuredFormat>,
        syntax_token: Option<String>,
        theme_manager: Rc<ThemeManager>,
        initial_theme: PeekThemeName,
        initial_use_pretty: bool,
        allow_pretty_toggle: bool,
        label: &'static str,
    ) -> Self {
        let highlighter = syntax_token.as_ref().map(|t| {
            LineStreamHighlighter::new(t.clone(), Rc::clone(&theme_manager), initial_theme)
        });
        Self {
            source,
            line_source,
            highlighter,
            pretty_target,
            pretty: None,
            pretty_highlighted: None,
            pretty_raw_lines: None,
            pending_warnings: Vec::new(),
            syntax_token,
            theme_manager,
            use_pretty: initial_use_pretty && pretty_target.is_some(),
            allow_pretty_toggle,
            label,
        }
    }

    /// Run pretty-print if it's the first time pretty mode is rendered.
    /// Caches the result; on parse failure pushes one warning. Above the
    /// size cap, refuses to load: pushes a warning and forces `use_pretty
    /// = false` so the streamed raw view takes over.
    fn ensure_pretty(&mut self) {
        if self.pretty.is_some() {
            return;
        }
        let Some(target) = self.pretty_target else {
            return;
        };
        if self.line_source.total_bytes() > PRETTY_MAX_BYTES {
            let mb = self.line_source.total_bytes() / (1024 * 1024);
            self.pending_warnings.push(format!(
                "file too large for pretty-print ({mb} MB > {} MB cap); showing raw source",
                PRETTY_MAX_BYTES / (1024 * 1024)
            ));
            // Surface a sentinel error so `render_window` falls through
            // to the raw branch; also flip `use_pretty` so subsequent
            // renders skip the cap check entirely.
            self.pretty = Some(Err("size cap".to_string()));
            self.use_pretty = false;
            return;
        }
        let raw = match self.source.read_text() {
            Ok(s) => s,
            Err(e) => {
                self.pending_warnings.push(format!(
                    "read failed for pretty-print ({e}); showing raw source"
                ));
                self.pretty = Some(Err(e.to_string()));
                return;
            }
        };
        self.pretty = Some(match structured::pretty_print(&raw, target) {
            Ok(s) => Ok(s),
            Err(e) => {
                let format_name = match target {
                    StructuredFormat::Json => "JSON",
                    StructuredFormat::Yaml => "YAML",
                    StructuredFormat::Toml => "TOML",
                    StructuredFormat::Xml => "XML",
                };
                self.pending_warnings.push(format!(
                    "{format_name} parse failed ({e}); showing raw source"
                ));
                Err(e.to_string())
            }
        });
    }

    /// Return the slice of `lines` corresponding to a viewport that
    /// starts at `scroll` and is `rows` tall. Total is `lines.len()`.
    fn slice_window(lines: &[String], scroll: usize, rows: usize) -> Vec<String> {
        let start = scroll.min(lines.len());
        let end = start.saturating_add(rows).min(lines.len());
        lines[start..end].to_vec()
    }

    /// Pretty branch: produce a windowed view over the cached pretty
    /// content. With a syntax token, the whole pretty string is
    /// pre-highlighted (bounded by `PRETTY_MAX_BYTES`); without one, it's
    /// just split into lines. Either way the cache is keyed so a theme/color
    /// cycle invalidates and recomputes.
    ///
    /// Caller has guaranteed `self.pretty` is `Some(Ok(_))`.
    fn render_pretty_window(
        &mut self,
        ctx: &RenderCtx,
        scroll: usize,
        rows: usize,
    ) -> Result<Window> {
        if self.syntax_token.is_some() {
            let stale = self
                .pretty_highlighted
                .as_ref()
                .is_none_or(|(t, c, _)| *t != ctx.theme_name || *c != ctx.peek_theme.color_mode);
            if stale {
                // Borrow `self.pretty` and `self.syntax_token` only for
                // the duration of `highlight_lines`; the assignment to
                // `self.pretty_highlighted` happens after the borrow
                // ends. (`self.theme_manager` is a disjoint field — no
                // conflict.)
                let highlighted = {
                    let pretty = match &self.pretty {
                        Some(Ok(s)) => s.as_str(),
                        _ => unreachable!("render_window guards pretty branch"),
                    };
                    let token = self.syntax_token.as_deref().unwrap();
                    highlight_lines(
                        pretty,
                        token,
                        &self.theme_manager,
                        ctx.theme_name,
                        ctx.peek_theme.color_mode,
                    )?
                };
                self.pretty_highlighted =
                    Some((ctx.theme_name, ctx.peek_theme.color_mode, highlighted));
            }
            let cached = &self.pretty_highlighted.as_ref().unwrap().2;
            let total = cached.len();
            let lines = Self::slice_window(cached, scroll, rows);
            Ok(Window { lines, total })
        } else {
            if self.pretty_raw_lines.is_none() {
                let pretty = match &self.pretty {
                    Some(Ok(s)) => s.as_str(),
                    _ => unreachable!("render_window guards pretty branch"),
                };
                self.pretty_raw_lines = Some(pretty.lines().map(String::from).collect());
            }
            let cached = self.pretty_raw_lines.as_ref().unwrap();
            let total = cached.len();
            let lines = Self::slice_window(cached, scroll, rows);
            Ok(Window { lines, total })
        }
    }

    /// Raw streaming branch: pull the visible window from `line_source`,
    /// optionally piping each line through the line-stateful highlighter.
    /// Backward scroll past the highlighter's cursor resets and replays.
    fn render_raw_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let total = self.line_source.total_lines();

        if let Some(hl) = self.highlighter.as_mut() {
            // Theme cycle invalidates the cached HighlightState styles.
            // Color cycle is fine — `feed()` re-encodes from the live arg.
            let theme_changed = hl.active_theme() != ctx.theme_name;
            let needs_reset = theme_changed || hl.at() > scroll;
            if needs_reset {
                hl.reset(ctx.theme_name);
            }

            let start_at = hl.at();
            let end_at = scroll.saturating_add(rows).min(total);
            if start_at >= end_at {
                return Ok(Window {
                    lines: Vec::new(),
                    total,
                });
            }
            let raw_lines = self.line_source.window(start_at..end_at)?;
            let mut out = Vec::with_capacity(end_at.saturating_sub(scroll));
            for (offset, line) in raw_lines.iter().enumerate() {
                let escaped = hl.feed(line, ctx.peek_theme.color_mode)?;
                let line_idx = start_at + offset;
                if line_idx >= scroll {
                    out.push(escaped);
                }
            }
            Ok(Window { lines: out, total })
        } else {
            let end = scroll.saturating_add(rows).min(total);
            let lines = self.line_source.window(scroll..end)?;
            Ok(Window { lines, total })
        }
    }

    /// Drop both pretty caches. Called when the active branch flips so
    /// the new branch starts fresh and doesn't carry stale memory.
    fn invalidate_pretty_caches(&mut self) {
        self.pretty_highlighted = None;
        self.pretty_raw_lines = None;
    }
}

impl Mode for ContentMode {
    fn id(&self) -> ModeId {
        ModeId::Content
    }

    fn label(&self) -> &str {
        self.label
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        if self.use_pretty {
            self.ensure_pretty();
        }
        // After ensure_pretty, `use_pretty` may have been forced off due
        // to the size cap; re-check before branching.
        let pretty_ready = self.use_pretty && matches!(self.pretty, Some(Ok(_)));
        if pretty_ready {
            self.render_pretty_window(ctx, scroll, rows)
        } else {
            self.render_raw_window(ctx, scroll, rows)
        }
    }

    /// Pipe-mode render. Raw streams line-by-line through the highlighter
    /// (or unstyled `line_source.iter_all()`); pretty writes the full
    /// pretty string in one shot — same byte-fidelity as before A1 for
    /// un-highlighted text (no synthetic trailing newline added).
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        if self.use_pretty {
            self.ensure_pretty();
        }
        if self.use_pretty
            && let Some(Ok(pretty)) = self.pretty.as_ref()
        {
            if let Some(ref token) = self.syntax_token {
                let lines = highlight_lines(
                    pretty,
                    token,
                    &self.theme_manager,
                    ctx.theme_name,
                    ctx.peek_theme.color_mode,
                )?;
                for line in &lines {
                    out.write_line(line)?;
                }
            } else {
                out.write_str(pretty)?;
            }
            return Ok(());
        }
        // Pretty unavailable — fall through to raw stream.

        // Raw stream. With a syntax token, every line (including the
        // last) is `\n`-terminated — pre-A1 contract: escape sequences
        // are line-scoped and the natural shape is per-line writes.
        // Without a token, preserve the source's trailing-newline status
        // for byte-for-byte fidelity (matches `cat` and the pre-A1
        // un-highlighted path).
        if let Some(hl) = self.highlighter.as_mut() {
            hl.reset(ctx.theme_name);
            for line in self.line_source.iter_all() {
                let line = line?;
                let escaped = hl.feed(&line, ctx.peek_theme.color_mode)?;
                out.write_line(&escaped)?;
            }
        } else {
            let total = self.line_source.total_lines();
            let trailing_nl = self.line_source.ends_with_newline();
            for (idx, line) in self.line_source.iter_all().enumerate() {
                let line = line?;
                let is_last = idx + 1 == total;
                if is_last && !trailing_nl {
                    out.write_str(&line)?;
                } else {
                    out.write_line(&line)?;
                }
            }
        }
        Ok(())
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        if self.allow_pretty_toggle {
            RAW_TOGGLE_ACTIONS
        } else {
            &[]
        }
    }

    fn handle(&mut self, action: Action) -> Handled {
        if action == Action::ToggleRawSource
            && self.allow_pretty_toggle
            && self.pretty_target.is_some()
        {
            self.use_pretty = !self.use_pretty;
            // Pretty line N and raw line N are unrelated content — the
            // user's previous scroll offset would put them somewhere
            // arbitrary in the new view. Reset to the top. The
            // highlighter doesn't need an explicit reset here: its
            // `at()` is preserved across the toggle, and the next
            // raw-mode `render_window` will detect `at() > 0` (the new
            // scroll) and reset itself before catching up.
            self.invalidate_pretty_caches();
            Handled::YesResetScroll
        } else {
            Handled::No
        }
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }

    fn total_lines(&self) -> Option<usize> {
        // Cheapest in raw mode (LineSource knows), unknown in pretty
        // (caller will learn after the next render_window).
        if self.use_pretty {
            None
        } else {
            Some(self.line_source.total_lines())
        }
    }

    /// ContentMode tracks position in line units when showing raw —
    /// the line index then corresponds 1:1 to source lines, so a
    /// switch to Hex (and back) lands on the right byte.
    ///
    /// In pretty mode the line index has no relation to source bytes
    /// (e.g. pretty-printed JSON line 50 may correspond to source byte
    /// 200 or 20000). Tracking would lie, so we opt out: switching
    /// from pretty Content to Hex preserves whatever position Hex
    /// previously had instead of synthesizing a wrong one.
    fn tracks_position(&self) -> bool {
        !self.use_pretty
    }
}
