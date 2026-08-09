//! Interactive Bytecode view for classfiles.
//!
//! Renders the whole-class `javap -c`-style disassembly as themed lines,
//! cached per `(width, style_mode, theme)`. `n` / `p` jump between
//! methods (each method header is an anchor), and `/` searches the
//! listing. The view is caller-scrolled, so method jumps return
//! [`Handled::YesScrollTo`].

use anyhow::Result;
use peek_theme::{PeekTheme, PeekThemeName, StyleMode};
use syntect::highlighting::Color;

use super::bytecode::{Disassembly, MethodAsm};
use crate::viewer::modes::{
    Handled, Mode, ModeId, RenderCtx, Window, apply_search, slice_window, step_search,
};
use crate::viewer::search::{self, SearchQuery, SearchState, SearchTarget};
use crate::viewer::ui::{Action, HelpEntry, strip_ansi_width, wrap_styled};

const EXTRA_ACTIONS: &[HelpEntry] = &[
    (
        // With a search active these step matches instead of methods
        // (mirrors the EPUB read mode's n/p overload).
        &[Action::Next, Action::Prev],
        "Next / previous method",
    ),
    (&[Action::OpenSearch], "Search"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    style_mode: StyleMode,
    theme_name: PeekThemeName,
}

struct Rendered {
    key: CacheKey,
    lines: Vec<String>,
    /// First line index of each method's header — the `n` / `p` targets.
    anchors: Vec<usize>,
}

/// Whole-class disassembly view.
pub(crate) struct BytecodeMode {
    disasm: Disassembly,
    cache: Option<Rendered>,
    /// Index into the current render's `anchors` the last jump landed on.
    method_cursor: usize,
    search: Option<SearchState>,
}

impl BytecodeMode {
    pub(crate) fn new(disasm: Disassembly) -> Self {
        Self {
            disasm,
            cache: None,
            method_cursor: 0,
            search: None,
        }
    }

    fn ensure_rendered(
        &mut self,
        width: usize,
        theme: &PeekTheme,
        theme_name: PeekThemeName,
        style_mode: StyleMode,
    ) -> &Rendered {
        let key = CacheKey {
            width,
            style_mode,
            theme_name,
        };
        let needs = self.cache.as_ref().map(|c| c.key != key).unwrap_or(true);
        if needs {
            let (lines, anchors) = render_lines(&self.disasm, width, theme);
            self.cache = Some(Rendered {
                key,
                lines,
                anchors,
            });
        }
        self.cache.as_ref().expect("cache populated")
    }

    /// Step the method cursor and return a scroll target at that method's
    /// header. No-op before the first render (anchors unknown).
    fn jump_method(&mut self, forward: bool) -> Handled {
        let Some(cache) = &self.cache else {
            return Handled::No;
        };
        if cache.anchors.is_empty() {
            return Handled::No;
        }
        let last = cache.anchors.len() - 1;
        self.method_cursor = if forward {
            (self.method_cursor + 1).min(last)
        } else {
            self.method_cursor.saturating_sub(1)
        };
        Handled::YesScrollTo(cache.anchors[self.method_cursor])
    }
}

impl Mode for BytecodeMode {
    fn id(&self) -> ModeId {
        ModeId::Content
    }

    fn label(&self) -> &str {
        "Bytecode"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let cache = self.ensure_rendered(
            ctx.term_cols,
            ctx.peek_theme,
            ctx.theme_name,
            ctx.peek_theme.style_mode,
        );
        let total = cache.lines.len();
        let mut win = slice_window(&cache.lines, scroll, rows);
        search::overlay_window(&mut win, scroll, self.search.as_ref(), ctx.peek_theme);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.lines.len())
    }

    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {
        // A width change re-wraps; match indices no longer line up.
        self.search = None;
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::Back if self.search.is_some() => {
                self.search = None;
                Handled::Yes
            }
            // `n` / `p` jump methods — but while a search is active they
            // step matches instead (Esc clears the search to get method
            // jumps back), matching the EPUB read mode.
            Action::Next if self.search.is_some() => step_search(&mut self.search, 1),
            Action::Prev if self.search.is_some() => step_search(&mut self.search, -1),
            Action::Next => self.jump_method(true),
            Action::Prev => self.jump_method(false),
            _ => Handled::No,
        }
    }

    fn set_search(&mut self, query: Option<&SearchQuery>) -> SearchTarget {
        let lines = self
            .cache
            .as_ref()
            .map(|c| c.lines.as_slice())
            .unwrap_or(&[]);
        apply_search(&mut self.search, lines.iter(), query)
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        self.search
            .as_ref()
            .map(|s| vec![s.status_segment(theme)])
            .unwrap_or_default()
    }
}

/// Build the themed disassembly lines and the per-method header anchors.
/// Anchors are recorded against the final (wrapped) line list so a method
/// jump lands exactly on its header regardless of long-line wrapping.
fn render_lines(
    disasm: &Disassembly,
    width: usize,
    theme: &PeekTheme,
) -> (Vec<String>, Vec<usize>) {
    let mut lines = Vec::new();
    let mut anchors = Vec::new();
    if disasm.methods.is_empty() {
        lines.push(theme.paint_muted("(no methods)"));
        return (lines, anchors);
    }
    for (i, method) in disasm.methods.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        anchors.push(lines.len());
        push_wrapped(&mut lines, theme.paint_heading(&method.signature), width);
        render_method_body(&mut lines, method, width, theme);
    }
    (lines, anchors)
}

fn render_method_body(
    lines: &mut Vec<String>,
    method: &MethodAsm,
    width: usize,
    theme: &PeekTheme,
) {
    if let Some(note) = method.note {
        push_wrapped(lines, format!("    {}", theme.paint_muted(note)), width);
        return;
    }
    for insn in &method.instructions {
        let offset = theme.paint_muted(&format!("{:>5}", insn.offset));
        let mnemonic = theme.paint_accent(&insn.mnemonic);
        let line = if insn.operand.is_empty() {
            format!("  {offset}  {mnemonic}")
        } else {
            format!(
                "  {offset}  {mnemonic} {}",
                theme.paint_value(&insn.operand)
            )
        };
        push_wrapped(lines, line, width);
    }
}

/// Push `line`, splitting it into width-fit rows only when it overflows —
/// an over-wide line the terminal soft-wraps but the redraw counts as one
/// row would desync the screen (same reasoning as the Info view).
fn push_wrapped(lines: &mut Vec<String>, line: String, width: usize) {
    if width > 0 && strip_ansi_width(&line) > width {
        lines.extend(wrap_styled(&line, width));
    } else {
        lines.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile a literal query for the search tests (the default engine).
    fn lit(q: &str) -> SearchQuery {
        SearchQuery::compile(q, false).unwrap()
    }
    use std::path::PathBuf;

    use peek_io::InputSource;
    use peek_theme::{PeekThemeName, make_peek_theme};

    fn disasm() -> Disassembly {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-data/Sample.class");
        super::super::bytecode::build(&InputSource::File(p)).unwrap()
    }

    /// One header anchor per method, the first at line 0, each pointing at
    /// a non-blank header line.
    #[test]
    fn one_anchor_per_method_at_its_header() {
        let d = disasm();
        let method_count = d.methods.len();
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let (lines, anchors) = render_lines(&d, 100, &theme);
        assert_eq!(anchors.len(), method_count);
        assert_eq!(anchors.first(), Some(&0));
        for &a in &anchors {
            assert!(!lines[a].is_empty(), "anchor lands on a header line");
        }
    }

    /// `n` walks forward through method anchors and clamps at the last;
    /// `p` walks back and clamps at the first.
    #[test]
    fn method_jump_walks_and_clamps() {
        let mut mode = BytecodeMode::new(disasm());
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        // Jumps are no-ops until the first render populates anchors.
        assert_eq!(mode.handle(Action::Next), Handled::No);
        let _ = mode.ensure_rendered(100, &theme, PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let anchors = mode.cache.as_ref().unwrap().anchors.clone();

        assert_eq!(mode.handle(Action::Next), Handled::YesScrollTo(anchors[1]));
        // Walk to the end and confirm it clamps on the last method.
        for _ in 0..anchors.len() + 5 {
            let _ = mode.handle(Action::Next);
        }
        assert_eq!(
            mode.handle(Action::Next),
            Handled::YesScrollTo(*anchors.last().unwrap())
        );
        // Walk back to the first and confirm it clamps at zero.
        for _ in 0..anchors.len() + 5 {
            let _ = mode.handle(Action::Prev);
        }
        assert_eq!(mode.handle(Action::Prev), Handled::YesScrollTo(0));
    }

    /// While a search is active, `n` / `p` step matches instead of jumping
    /// methods (the action stays Next; the handler reinterprets it),
    /// so the method cursor must not move.
    #[test]
    fn n_p_step_matches_while_searching() {
        let mut mode = BytecodeMode::new(disasm());
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::TrueColor);
        let _ = mode.ensure_rendered(100, &theme, PeekThemeName::IdeaDark, StyleMode::TrueColor);
        mode.set_search(Some(&lit("a"))); // matches many lines
        assert!(mode.search.is_some());

        assert!(mode.handle(Action::Next).was_consumed());
        assert_eq!(
            mode.method_cursor, 0,
            "searching: n steps a match, leaving the method cursor put"
        );
    }
}
