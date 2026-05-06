use std::rc::Rc;

use anyhow::Result;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Style};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference};

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType, StructuredFormat};
use crate::theme::{ColorMode, PeekTheme, PeekThemeName, ThemeManager};
use crate::types::archive::ArchiveMode;
use crate::types::image::{AnimationMode, ImageKind, ImageRenderMode};
use crate::types::svg::SvgAnimationMode;
use crate::viewer::modes::{AboutMode, ContentMode, HelpMode, HexMode, InfoMode, Mode};
use crate::viewer::ui::{Action, GLOBAL_ACTIONS};

pub mod hex;
pub mod interactive;
pub(crate) mod modes;
pub(crate) mod ui;

/// Highlight text content as colored terminal lines.
///
/// Drives `LineStreamHighlighter` line-by-line so the output is byte-for-byte
/// identical to the raw streaming path used by `ContentMode` — pretty
/// pre-rendered output and raw streamed output agree on every escape
/// sequence. (Previously this used `HighlightLines::highlight_line` without
/// a trailing newline; syntect's end-of-line rules then fired differently
/// from the streaming path, so toggling pretty/raw would shift highlight
/// colors on multi-line tags.)
pub fn highlight_lines(
    content: &str,
    syntax_token: &str,
    tm: &Rc<ThemeManager>,
    theme_name: PeekThemeName,
    color_mode: ColorMode,
) -> Result<Vec<String>> {
    let mut hl = LineStreamHighlighter::new(syntax_token.to_string(), Rc::clone(tm), theme_name);
    let mut lines = Vec::new();
    for line in content.lines() {
        lines.push(hl.feed(line, color_mode)?);
    }
    Ok(lines)
}

/// Resolve a syntax token to a syntect `SyntaxReference`. Same fallback
/// chain as the original inline lookup: token → name → extension fallback
/// → plain text.
fn resolve_syntax<'a>(tm: &'a ThemeManager, syntax_token: &str) -> &'a SyntaxReference {
    tm.syntax_set
        .find_syntax_by_token(syntax_token)
        .or_else(|| tm.syntax_set.find_syntax_by_name(syntax_token))
        .or_else(|| {
            fallback_syntax_token(syntax_token).and_then(|t| tm.syntax_set.find_syntax_by_name(t))
        })
        .unwrap_or_else(|| tm.syntax_set.find_syntax_plain_text())
}

/// Forward-only, line-stateful syntect feeder. Holds the parse and
/// highlight state across `feed()` calls so each line resumes from where
/// the previous one left off — required because syntect's parse state is
/// line-by-line (a `/* ... */` comment that opens on one line and closes
/// many lines later only highlights correctly when state carries over).
///
/// `at()` reports the index of the next line the highlighter expects to
/// consume. The driver (ContentMode) compares this to its target window:
/// if the desired start is ahead, feed catch-up lines; if it's behind,
/// `reset()` and replay from the top. Replay is O(N) lines — acceptable
/// for the common forward-scroll case; pathological backward jumps on
/// huge files pay a one-time cost.
///
/// State is kept as owned `ParseState` + `HighlightState` rather than a
/// borrowing `HighlightLines` so the struct doesn't need to thread theme
/// / syntax lifetimes through `ContentMode`. Theme and color mode are
/// passed in per call; `feed` rebuilds the lightweight `Highlighter`
/// wrapper from the live theme on each invocation. The caller is
/// responsible for `reset()`ing on theme change (the cached
/// `HighlightState` styles are theme-derived and would be stale).
pub(crate) struct LineStreamHighlighter {
    tm: Rc<ThemeManager>,
    syntax_token: String,
    /// Theme used to seed the current `highlight_state`. Stored so `feed`
    /// can paint with the matching theme; rotated on `reset()`.
    active_theme: PeekThemeName,
    parse_state: ParseState,
    highlight_state: HighlightState,
    next_line: usize,
    /// Reusable buffer for `line + '\n'` syntect input. Avoids per-line
    /// allocation when streaming millions of lines through `feed`.
    line_buf: String,
}

impl LineStreamHighlighter {
    pub(crate) fn new(
        syntax_token: String,
        tm: Rc<ThemeManager>,
        theme_name: PeekThemeName,
    ) -> Self {
        let (parse_state, highlight_state) = build_states(&tm, &syntax_token, theme_name);
        Self {
            tm,
            syntax_token,
            active_theme: theme_name,
            parse_state,
            highlight_state,
            next_line: 0,
            line_buf: String::new(),
        }
    }

    /// Discard accumulated state and rewind to line 0. Call before
    /// catching up from the top after a backward jump or a theme change
    /// (color mode changes don't need a reset — escape encoding is per-feed).
    pub(crate) fn reset(&mut self, theme_name: PeekThemeName) {
        let (parse_state, highlight_state) = build_states(&self.tm, &self.syntax_token, theme_name);
        self.parse_state = parse_state;
        self.highlight_state = highlight_state;
        self.active_theme = theme_name;
        self.next_line = 0;
    }

    pub(crate) fn active_theme(&self) -> PeekThemeName {
        self.active_theme
    }

    /// Feed the next line and return its escaped form. The line must be
    /// the highlighter's current `at()` line; the caller drives sequence.
    pub(crate) fn feed(&mut self, line: &str, color_mode: ColorMode) -> Result<String> {
        let theme = self.tm.theme_for(self.active_theme);
        let highlighter = Highlighter::new(theme);
        // syntect expects the trailing newline as part of the line for
        // correct state transitions on rules anchored to line ends.
        // Reuse `line_buf` so streaming millions of lines doesn't allocate
        // per call (capacity grows to the longest line seen).
        self.line_buf.clear();
        self.line_buf.push_str(line);
        self.line_buf.push('\n');
        let ops = self
            .parse_state
            .parse_line(&self.line_buf, &self.tm.syntax_set)?;
        let iter = HighlightIterator::new(
            &mut self.highlight_state,
            &ops,
            &self.line_buf,
            &highlighter,
        );
        let ranges: Vec<(Style, &str)> = iter.collect();
        // Drop the synthetic trailing newline from the styled output so
        // the caller can decide its own line termination.
        let escaped = ranges_to_escaped_trim_newline(&ranges, color_mode);
        self.next_line += 1;
        Ok(escaped)
    }

    /// Index of the next line the highlighter will consume.
    pub(crate) fn at(&self) -> usize {
        self.next_line
    }
}

fn build_states(
    tm: &ThemeManager,
    syntax_token: &str,
    theme_name: PeekThemeName,
) -> (ParseState, HighlightState) {
    let syntax = resolve_syntax(tm, syntax_token);
    let theme = tm.theme_for(theme_name);
    let highlighter = Highlighter::new(theme);
    let parse_state = ParseState::new(syntax);
    let highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
    (parse_state, highlight_state)
}

/// Same as `ranges_to_escaped` but skips a trailing `\n` if the styled
/// content ends with one. Used by `LineStreamHighlighter` because syntect
/// is fed `line + "\n"` for correct end-of-line state transitions. If
/// trimming leaves the final range empty (the common case — the trailing
/// newline often arrives as its own range), drop it so we don't emit a
/// stray foreground escape sequence with no text behind it.
fn ranges_to_escaped_trim_newline(ranges: &[(Style, &str)], color_mode: ColorMode) -> String {
    let mut out = String::new();
    for (i, (style, text)) in ranges.iter().enumerate() {
        let is_last = i + 1 == ranges.len();
        let slice: &str = if is_last {
            text.strip_suffix('\n').unwrap_or(text)
        } else {
            text
        };
        if slice.is_empty() {
            continue;
        }
        out.push_str(&color_mode.fg_seq(style.foreground));
        out.push_str(slice);
    }
    out.push_str(color_mode.reset());
    out
}

/// File-type-aware mode-stack builder. Holds the shared `ThemeManager`
/// plus the CLI-driven options every mode in the stack needs to consume
/// (plain mode, current theme, image config). Used by both the
/// interactive event loop and the print-mode `render_to_pipe` path —
/// `compose_modes` is the single dispatcher across both.
pub struct Registry {
    theme_manager: Rc<ThemeManager>,
    plain_mode: bool,
    theme_name: PeekThemeName,
    peek_theme: PeekTheme,
}

impl Registry {
    pub fn new(args: &Args) -> Result<Self> {
        let theme = Rc::new(ThemeManager::new(args.theme, args.color));
        let peek_theme = theme.peek_theme().clone();
        Ok(Self {
            theme_manager: theme,
            plain_mode: args.plain,
            theme_name: args.theme,
            peek_theme,
        })
    }

    pub fn theme_name(&self) -> PeekThemeName {
        self.theme_name
    }

    pub fn peek_theme(&self) -> &PeekTheme {
        &self.peek_theme
    }

    /// Compose the view-mode list for a given file type. Always appends
    /// Hex, Info, About, and Help so every file gets those views; other
    /// modes are file-type specific. The interactive event loop and the
    /// print-mode pipe path both consume this stack — pipe mode picks
    /// the first non-aux mode (or the first mode if all are aux, e.g.
    /// binary files).
    pub fn compose_modes(
        &self,
        source: &InputSource,
        detected: &Detected,
        args: &Args,
    ) -> Result<Vec<Box<dyn Mode>>> {
        let file_type = &detected.file_type;
        let mut modes: Vec<Box<dyn Mode>> = Vec::new();

        if self.plain_mode {
            // Binary/Archive/DiskImage in --plain still goes to Hex (the
            // universal tail); ContentMode requires UTF-8 input.
            if !matches!(
                file_type,
                FileType::Binary | FileType::Archive(_) | FileType::DiskImage(_)
            ) {
                modes.push(self.text_content_mode(source, file_type, args)?);
            }
        } else {
            match file_type {
                FileType::SourceCode { .. } | FileType::Structured(_) => {
                    modes.push(self.text_content_mode(source, file_type, args)?);
                }
                FileType::Image => {
                    let cfg = self.image_config(args);
                    // Animated GIF/WebP: AnimationMode owns the frame stack
                    // and drives ticks via the Mode trait. Static image:
                    // ImageRenderMode renders on demand.
                    if let Some(frames) =
                        crate::types::image::pipeline::animate::decode_anim_frames(
                            source,
                            detected.magic_mime.as_deref(),
                        )?
                    {
                        modes.push(Box::new(AnimationMode::new(frames, cfg)));
                    } else {
                        modes.push(Box::new(ImageRenderMode::new(
                            source.clone(),
                            cfg,
                            ImageKind::Raster,
                        )));
                    }
                }
                FileType::Svg => {
                    let cfg = self.image_config(args);
                    let anim = if args.no_svg_anim {
                        None
                    } else {
                        crate::types::image::pipeline::svg_anim::try_parse(source)?
                    };
                    if let Some(model) = anim {
                        modes.push(Box::new(SvgAnimationMode::new(model, cfg)));
                    } else {
                        modes.push(Box::new(ImageRenderMode::new(
                            source.clone(),
                            cfg,
                            ImageKind::Svg,
                        )));
                    }
                    // Pair the SVG view with its XML source.
                    modes.push(self.text_content_mode(source, file_type, args)?);
                }
                FileType::Archive(fmt) => {
                    modes.push(Box::new(ArchiveMode::new(source, *fmt)));
                }
                FileType::DiskImage(_) => {
                    // No content / TOC view — push Info as the primary so
                    // disk-image metadata is what the user lands on. The
                    // universal block below dedupes by ModeId, so the
                    // tail Info append becomes a no-op.
                    modes.push(Box::new(InfoMode::new()));
                }
                FileType::Binary => {
                    // Default view for binary IS hex; HexMode is appended
                    // below in the always-present block.
                }
            }
        }

        // Hex/Info/Help/About are universal — every file gets these views.
        // Dedupe by ModeId so a file-type arm that pre-pushes one of these
        // (e.g. DiskImage → Info) doesn't end up with two copies in the
        // mode list (which would break `i:Info` jump and Tab cycle).
        push_unique_mode(&mut modes, Box::new(HexMode::new(source, 0)?));
        push_unique_mode(&mut modes, Box::new(InfoMode::new()));
        push_unique_mode(&mut modes, Box::new(AboutMode::new()));

        // Help action union: globals + every preceding mode's extras,
        // deduped. Help itself contributes nothing new.
        let mut help_actions: Vec<(Action, &'static str)> = GLOBAL_ACTIONS.to_vec();
        for m in &modes {
            for (a, label) in m.extra_actions() {
                if !help_actions.iter().any(|(b, _)| b == a) {
                    help_actions.push((*a, *label));
                }
            }
        }
        modes.push(Box::new(HelpMode::new(help_actions)));

        Ok(modes)
    }

    /// Build a `ContentMode` for text-based file types: source code,
    /// structured (lazy pretty-print), plain text, or SVG XML.
    ///
    /// Constructs a `LineSource` over the input — one streaming pass to
    /// count lines and capture sparse anchors — instead of reading the
    /// whole file into memory. Pretty-print is deferred to the first
    /// time pretty view is rendered, capped at `PRETTY_MAX_BYTES`.
    fn text_content_mode(
        &self,
        source: &InputSource,
        file_type: &FileType,
        args: &Args,
    ) -> Result<Box<dyn Mode>> {
        let line_source = source.open_line_source()?;

        let pretty_target = if !self.plain_mode {
            match file_type {
                FileType::Structured(fmt) => Some(*fmt),
                FileType::Svg => Some(StructuredFormat::Xml),
                _ => None,
            }
        } else {
            None
        };

        let syntax_token = if self.plain_mode {
            None
        } else {
            syntax_token_for(args.language.as_deref(), source, file_type)
        };

        // Pretty-print is the default whenever it's available *and* the
        // round-trip is lossless. `--raw` always flips structured/SVG views
        // back to the raw source. JSONC and JSON5 have lossy pretty paths
        // (comments dropped, JSON5 syntax collapsed) so they default to raw —
        // `r` still toggles for users who want the strict-JSON view.
        let initial_use_pretty =
            pretty_target.is_some() && !args.raw && !pretty_target.is_some_and(is_lossy_pretty);

        // Structured formats and SVG (which is XML) both expose `r` as a
        // pretty/raw toggle on the Source view. Source code / plain text
        // have no pretty form, so `r` is inert there.
        let allow_pretty_toggle = matches!(file_type, FileType::Structured(_) | FileType::Svg);

        let label: &'static str = match file_type {
            FileType::SourceCode { .. } => "Source",
            FileType::Svg => "Source",
            _ => "Content",
        };

        Ok(Box::new(ContentMode::new(
            source.clone(),
            line_source,
            pretty_target,
            syntax_token,
            Rc::clone(&self.theme_manager),
            self.theme_name,
            initial_use_pretty,
            allow_pretty_toggle,
            args.line_numbers,
            label,
        )))
    }

    fn image_config(&self, args: &Args) -> crate::types::image::ImageConfig {
        use crate::types::image::{Background, FitMode, ImageConfig, ImageMode};
        ImageConfig {
            mode: ImageMode::from_str(&args.image_mode),
            width: args.width,
            background: Background::from_str(&args.background),
            margin: args.margin,
            color_mode: args.color,
            edge_density: args.edge_density,
            fit: FitMode::Contain,
        }
    }
}

/// Push `mode` onto `modes` only if no entry with the same `ModeId` is
/// already present. Used in `compose_modes` so the universal Hex/Info/About
/// tail can run unconditionally without doubling up on a mode a file-type
/// arm has already pushed.
fn push_unique_mode(modes: &mut Vec<Box<dyn Mode>>, mode: Box<dyn Mode>) {
    let id = mode.id();
    if modes.iter().any(|m| m.id() == id) {
        return;
    }
    modes.push(mode);
}

/// Resolve a syntect syntax token for a file. Priority: explicit
/// `--language` override, then the detected `FileType` syntax hint
/// (extension), then the bare filename (catches `Makefile`, `Dockerfile`
/// — syntect matches these by name). Structured/SVG always map to a
/// fixed syntax token.
pub(crate) fn syntax_token_for(
    forced_language: Option<&str>,
    source: &InputSource,
    file_type: &FileType,
) -> Option<String> {
    match file_type {
        FileType::SourceCode { syntax } => forced_language
            .map(String::from)
            .or_else(|| syntax.clone())
            .or_else(|| {
                source
                    .path()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(String::from)
            }),
        FileType::Structured(fmt) => Some(
            match fmt {
                StructuredFormat::Json
                | StructuredFormat::Jsonc
                | StructuredFormat::Json5
                | StructuredFormat::Jsonl => "JSON",
                StructuredFormat::Yaml => "YAML",
                StructuredFormat::Toml => "TOML",
                StructuredFormat::Xml => "XML",
            }
            .to_string(),
        ),
        FileType::Svg => Some("XML".to_string()),
        _ => None,
    }
}

/// True when pretty-printing the format drops information from the source
/// (comments / JSON5 features / etc.), so raw should be the default view.
fn is_lossy_pretty(fmt: StructuredFormat) -> bool {
    matches!(fmt, StructuredFormat::Jsonc | StructuredFormat::Json5)
}

/// Map file extensions that syntect doesn't natively support to the closest
/// available syntax name.
fn fallback_syntax_token(ext: &str) -> Option<&'static str> {
    match ext {
        "ts" | "tsx" | "mts" | "cts" => Some("JavaScript"),
        "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "jsonc" | "json5" => Some("JSON"),
        "zsh" | "bash" | "fish" => Some("Bourne Again Shell (bash)"),
        "h" => Some("C++"),
        "hpp" | "hxx" => Some("C++"),
        "cxx" | "cc" => Some("C++"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tm() -> Rc<ThemeManager> {
        Rc::new(ThemeManager::new(
            PeekThemeName::IdeaDark,
            ColorMode::TrueColor,
        ))
    }

    /// Feeding `LineStreamHighlighter` line-by-line must produce the same
    /// escaped output as `highlight_lines` over the whole content. Covers
    /// JSON (simple) and Rust (multi-line block comment exercises
    /// cross-line state).
    #[test]
    fn line_stream_matches_whole_string_highlight() {
        let cases: &[(&str, &str)] = &[
            (
                "JSON",
                r#"{
  "name": "peek",
  "version": 1,
  "tags": ["fast", "tiny"]
}"#,
            ),
            (
                "Rust",
                "/* multi-line\n   comment */\nfn main() {\n    let x = 42;\n    println!(\"{x}\");\n}\n",
            ),
            // XML with multi-line opening tag — pretty/raw color parity
            // depends on syntect getting the trailing newline so end-of-line
            // rules fire consistently. Without it, the second `<element`
            // inside a multi-line tag gets a different scope than the first.
            (
                "XML",
                r#"<svg
  xmlns="http://www.w3.org/2000/svg"
  width="100"
  height="100">
  <rect x="0" y="0" width="100" height="100"/>
  <circle cx="50" cy="50" r="40"/>
</svg>
"#,
            ),
        ];

        for (token, content) in cases {
            let tm = tm();
            let whole = highlight_lines(
                content,
                token,
                &tm,
                PeekThemeName::IdeaDark,
                ColorMode::TrueColor,
            )
            .unwrap();

            let mut streamed = LineStreamHighlighter::new(
                token.to_string(),
                Rc::clone(&tm),
                PeekThemeName::IdeaDark,
            );
            let per_line: Vec<String> = content
                .lines()
                .map(|l| streamed.feed(l, ColorMode::TrueColor).unwrap())
                .collect();

            assert_eq!(
                per_line.len(),
                whole.len(),
                "line count mismatch for {token}"
            );
            for (i, (a, b)) in per_line.iter().zip(whole.iter()).enumerate() {
                assert_eq!(a, b, "{token} line {i} differs");
            }
            assert_eq!(streamed.at(), content.lines().count());
        }
    }

    #[test]
    fn line_stream_reset_rewinds() {
        let tm = tm();
        let mut s =
            LineStreamHighlighter::new("Rust".to_string(), Rc::clone(&tm), PeekThemeName::IdeaDark);
        s.feed("fn a() {}", ColorMode::TrueColor).unwrap();
        s.feed("fn b() {}", ColorMode::TrueColor).unwrap();
        assert_eq!(s.at(), 2);
        s.reset(PeekThemeName::IdeaDark);
        assert_eq!(s.at(), 0);
    }
}
