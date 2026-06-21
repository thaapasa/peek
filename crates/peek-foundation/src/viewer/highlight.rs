//! Syntect-driven syntax highlighting: the whole-text `highlight_lines`
//! entry point used by pretty-renderers, plus the forward-only
//! `LineStreamHighlighter` driven by `ContentMode`'s streaming path.
//! Also owns syntax-token resolution — the policy for picking a syntect
//! syntax name from a `FileType` + filename + `--language` override.

use std::rc::Rc;

use anyhow::Result;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Style};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference};

use peek_detect::{FileType, StructuredFormat};
use peek_io::InputSource;
use peek_theme::{PeekThemeName, StyleMode, ThemeManager};

/// Per-line length ceiling (bytes) for syntax highlighting. Beyond this a
/// line renders verbatim (unstyled) instead of going through syntect, whose
/// parse cost grows steeply with line length — bounding the worst-case cost
/// of any single pathological line. See `LineStreamHighlighter::feed`.
const MAX_HIGHLIGHT_LINE_BYTES: usize = 64 * 1024;

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
    style_mode: StyleMode,
) -> Result<Vec<String>> {
    let mut hl = LineStreamHighlighter::new(syntax_token.to_string(), Rc::clone(tm), theme_name);
    let mut lines = Vec::new();
    for line in content.lines() {
        lines.push(hl.feed(line, style_mode)?);
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
pub struct LineStreamHighlighter {
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
    pub fn new(syntax_token: String, tm: Rc<ThemeManager>, theme_name: PeekThemeName) -> Self {
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
    pub fn reset(&mut self, theme_name: PeekThemeName) {
        let (parse_state, highlight_state) = build_states(&self.tm, &self.syntax_token, theme_name);
        self.parse_state = parse_state;
        self.highlight_state = highlight_state;
        self.active_theme = theme_name;
        self.next_line = 0;
    }

    pub fn active_theme(&self) -> PeekThemeName {
        self.active_theme
    }

    /// Feed the next line and return its escaped form. The line must be
    /// the highlighter's current `at()` line; the caller drives sequence.
    pub fn feed(&mut self, line: &str, style_mode: StyleMode) -> Result<String> {
        // syntect's regex parse cost climbs steeply with line length, so a
        // single pathological line (a minified blob, or the raw fallback
        // after a structured parse fails on a deep-nesting bomb) can stall
        // the render loop for seconds. Past the cap, skip highlighting and
        // emit the line verbatim — it still displays, just unstyled, and
        // the parse state carries forward unchanged for following lines.
        if line.len() > MAX_HIGHLIGHT_LINE_BYTES {
            self.next_line += 1;
            return Ok(line.to_string());
        }
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
        let escaped = ranges_to_escaped_trim_newline(&ranges, style_mode);
        self.next_line += 1;
        Ok(escaped)
    }

    /// Index of the next line the highlighter will consume.
    pub fn at(&self) -> usize {
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

/// Walk syntect's styled `LineRanges` into an escape-coded string,
/// skipping a trailing `\n` if the styled content ends with one. Used
/// by `LineStreamHighlighter` because syntect
/// is fed `line + "\n"` for correct end-of-line state transitions. If
/// trimming leaves the final range empty (the common case — the trailing
/// newline often arrives as its own range), drop it so we don't emit a
/// stray foreground escape sequence with no text behind it.
fn ranges_to_escaped_trim_newline(ranges: &[(Style, &str)], style_mode: StyleMode) -> String {
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
        out.push_str(&style_mode.fg_seq(style.foreground));
        out.push_str(slice);
    }
    out.push_str(style_mode.reset());
    out
}

/// Pick the syntect syntax token for a file. Order: explicit
/// `--language` override, then the detected `FileType` syntax hint
/// (extension), then the bare filename (catches `Makefile`, `Dockerfile`
/// — syntect matches these by name). Structured/SVG always map to a
/// fixed syntax token.
pub fn syntax_token_for(
    forced_language: Option<&str>,
    source: &InputSource,
    file_type: &FileType,
) -> Option<String> {
    match file_type {
        FileType::SourceCode { syntax } => {
            let file_name = source
                .disk_path()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str());
            forced_language
                .map(String::from)
                // Filename-keyed config grammars must win over the extension:
                // `.env.local` has a misleading `local` extension, and
                // `justfile` has no extension at all.
                .or_else(|| file_name.and_then(config_name_syntax).map(String::from))
                .or_else(|| syntax.clone())
                .or_else(|| file_name.map(String::from))
        }
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
        FileType::Html => Some("HTML".to_string()),
        FileType::Markdown => Some("Markdown".to_string()),
        _ => None,
    }
}

/// Map config filenames to a syntax name when the extension is misleading
/// (`.env.local` → `local`) or absent (`justfile`), or when no dedicated
/// grammar exists and a same-format one is close enough. Returns `None` for
/// names that resolve fine via extension / bare-name lookup.
fn config_name_syntax(file_name: &str) -> Option<&'static str> {
    let lower = file_name.to_ascii_lowercase();
    // `.env`, `.env.local`, `.env.production`, … (and `.envrc`).
    if lower.starts_with(".env") {
        return Some("DotENV");
    }
    match lower.as_str() {
        // No Just grammar in two-face; Makefile is the closest fit (recipes,
        // `#` comments, `:=` assignment).
        "justfile" | ".justfile" => Some("Makefile"),
        // Same line-glob format as .gitignore, which two-face does carry.
        ".dockerignore" => Some("Git Ignore"),
        _ => None,
    }
}

/// Map file extensions that syntect doesn't natively support to the closest
/// available syntax name.
fn fallback_syntax_token(ext: &str) -> Option<&'static str> {
    match ext {
        "ts" | "tsx" | "mts" | "cts" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "jsonc" | "json5" => Some("JSON"),
        "zsh" | "bash" | "fish" => Some("Bourne Again Shell (bash)"),
        "h" | "hpp" | "hxx" | "cxx" | "cc" => Some("C++"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tm() -> Rc<ThemeManager> {
        Rc::new(ThemeManager::new(
            PeekThemeName::IdeaDark,
            StyleMode::TrueColor,
        ))
    }

    /// Config filenames whose extension is misleading or absent must resolve
    /// to a real grammar, not plain text.
    #[test]
    fn config_name_syntax_resolves() {
        let tm = tm();
        let cases = [
            (".env", "DotENV"),
            (".env.local", "DotENV"),
            (".env.production", "DotENV"),
            (".envrc", "DotENV"),
            ("justfile", "Makefile"),
            ("Justfile", "Makefile"),
            (".dockerignore", "Git Ignore"),
        ];
        for (name, expected) in cases {
            let token = config_name_syntax(name).expect("mapped");
            assert_eq!(token, expected, "{name}");
            assert_eq!(resolve_syntax(&tm, token).name, expected, "{name} resolves");
        }
        // A real extension must still win / pass through untouched.
        assert_eq!(config_name_syntax("config.toml"), None);
        assert_eq!(config_name_syntax("main.rs"), None);
    }

    /// Every syntax token `peek-detect`'s shebang sniffer can emit must
    /// resolve to a real grammar here — otherwise an extensionless script
    /// (postinst, configure, a git hook) silently renders as plain text.
    /// The list mirrors `detect::shebang_syntax`; keep them in sync.
    #[test]
    fn shebang_syntax_tokens_resolve() {
        let tm = tm();
        let plain = tm.syntax_set.find_syntax_plain_text().name.clone();
        for token in ["sh", "py", "pl", "rb", "js", "php", "lua", "tcl", "awk"] {
            let name = &resolve_syntax(&tm, token).name;
            assert_ne!(
                name, &plain,
                "shebang token {token:?} fell back to plain text"
            );
        }
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
                StyleMode::TrueColor,
            )
            .unwrap();

            let mut streamed = LineStreamHighlighter::new(
                token.to_string(),
                Rc::clone(&tm),
                PeekThemeName::IdeaDark,
            );
            let per_line: Vec<String> = content
                .lines()
                .map(|l| streamed.feed(l, StyleMode::TrueColor).unwrap())
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
        s.feed("fn a() {}", StyleMode::TrueColor).unwrap();
        s.feed("fn b() {}", StyleMode::TrueColor).unwrap();
        assert_eq!(s.at(), 2);
        s.reset(PeekThemeName::IdeaDark);
        assert_eq!(s.at(), 0);
    }
}
