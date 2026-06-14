//! Build [`CssStats`] with cssparser's rule / declaration parser traits.
//!
//! [`CssScanner`] implements the four cssparser parser traits and is
//! driven by [`StyleSheetParser`] (top level) and [`RuleBodyParser`]
//! (rule bodies). Letting cssparser own the parse buys two things a
//! hand-rolled token walk cannot get right:
//!
//! * **Declaration vs. nested rule.** CSS nesting means `& .x { … }` and
//!   `color: red;` share a context; cssparser does the lookahead.
//! * **No false colours.** `parse_value` only ever sees declaration
//!   values, so a colour word in a selector (`.gold`), a string
//!   (`content: "red"`), or a comment never reaches the colour scan.

use std::collections::{HashMap, HashSet};

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserInput, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser, Token,
};
use cssparser_color::{Color, hsl_to_rgb, hwb_to_rgb};

use crate::info::Extras;
use crate::types::css::info::{ColorSwatch, CssImport, CssInfo, CssStats, SelectorKindCounts};
use crate::types::text::info_gather::gather_capped_text;
use peek_io::InputSource;

/// Collect the CSS Info sidecar: streaming text stats plus a capped
/// whole-file rule/declaration parse. Returns `None` when the source is
/// over the sidecar cap or can't be read as text, so the gather falls
/// back to the generic text/binary path.
pub fn gather_extras(source: &InputSource) -> Option<Extras> {
    let (text_stats, text) = gather_capped_text(source)?;
    Some(Box::new(CssInfo {
        text: text_stats,
        stats: gather(&text),
    }))
}

/// Cap on palette swatches kept — an info panel shouldn't scroll forever
/// on a machine-generated stylesheet.
const MAX_SWATCHES: usize = 64;

/// Recursion-depth guard for pathologically nested CSS.
const MAX_DEPTH: usize = 64;

/// cssparser's error payload — unused; the scanner accepts every rule.
type E<'i> = ParseError<'i, ()>;

/// Parse `text` as CSS and collect [`CssStats`].
pub fn gather(text: &str) -> CssStats {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let mut scanner = CssScanner::default();
    {
        let sheet = StyleSheetParser::new(&mut parser, &mut scanner);
        for item in sheet {
            // Errors just mean a rule cssparser couldn't parse — skip it.
            let _ = item;
        }
    }
    scanner.finish()
}

#[derive(Default)]
struct CssScanner {
    rule_count: usize,
    selector_count: usize,
    kinds: SelectorKindCounts,
    custom_props: HashSet<String>,
    media_query_count: usize,
    keyframes_count: usize,
    imports: Vec<CssImport>,
    colors: HashMap<(u8, u8, u8), usize>,
    /// Body-nesting depth, for the recursion guard.
    depth: usize,
    /// True while inside an `@keyframes` body — its `0% { … }` stops are
    /// keyframe rules, not style rules, so they add nothing to the rule
    /// and selector counts.
    in_keyframes: bool,
}

impl CssScanner {
    /// Recurse into a `{ … }` body as a declaration / rule list.
    fn recurse_body(&mut self, input: &mut Parser) {
        if self.depth >= MAX_DEPTH {
            while input.next().is_ok() {}
            return;
        }
        self.depth += 1;
        {
            let body = RuleBodyParser::new(input, self);
            for item in body {
                let _ = item;
            }
        }
        self.depth -= 1;
    }

    fn record_color(&mut self, color: &Color) {
        if let Some(rgb) = color_to_rgb(color) {
            *self.colors.entry(rgb).or_insert(0) += 1;
        }
    }

    fn finish(self) -> CssStats {
        let total_colors = self.colors.len();
        let mut palette: Vec<ColorSwatch> = self
            .colors
            .into_iter()
            .map(|((r, g, b), count)| ColorSwatch {
                rgb: (r, g, b),
                hex: format!("#{r:02x}{g:02x}{b:02x}"),
                count,
            })
            .collect();
        // Most-frequent first; ties broken by hex for stable output.
        palette.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.hex.cmp(&b.hex)));
        palette.truncate(MAX_SWATCHES);

        CssStats {
            rule_count: self.rule_count,
            selector_count: self.selector_count,
            selector_kinds: self.kinds,
            custom_property_count: self.custom_props.len(),
            media_query_count: self.media_query_count,
            keyframes_count: self.keyframes_count,
            imports: self.imports,
            palette,
            total_colors,
        }
    }
}

// --- Declarations ----------------------------------------------------------

impl<'i> DeclarationParser<'i> for CssScanner {
    type Declaration = ();
    type Error = ();

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _start: &ParserState,
    ) -> Result<(), E<'i>> {
        if name.starts_with("--") {
            self.custom_props.insert(name.as_ref().to_string());
        }
        // `input` is delimited to this declaration's value — every token
        // here is a value token, so the colour scan is false-positive
        // free.
        scan_colors(self, input, 0);
        Ok(())
    }
}

/// Walk a declaration value, recording every colour literal. Recurses
/// into functions (`var()` fallbacks, gradient stops) and parentheses.
fn scan_colors(scanner: &mut CssScanner, input: &mut Parser, depth: usize) {
    loop {
        if let Ok(color) = input.try_parse(|p| Color::parse(p)) {
            scanner.record_color(&color);
            continue;
        }
        let token = match input.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        match token {
            Token::Function(_) | Token::ParenthesisBlock => {
                let _ = input.parse_nested_block::<_, _, ()>(|p| {
                    if depth < MAX_DEPTH {
                        scan_colors(scanner, p, depth + 1);
                    } else {
                        while p.next().is_ok() {}
                    }
                    Ok(())
                });
            }
            Token::CurlyBracketBlock | Token::SquareBracketBlock => {
                let _ = input.parse_nested_block::<_, _, ()>(|p| {
                    while p.next().is_ok() {}
                    Ok(())
                });
            }
            _ => {}
        }
    }
}

// --- Qualified (style) rules ----------------------------------------------

impl<'i> QualifiedRuleParser<'i> for CssScanner {
    type Prelude = SelectorSummary;
    type QualifiedRule = ();
    type Error = ();

    fn parse_prelude<'t>(&mut self, input: &mut Parser<'i, 't>) -> Result<SelectorSummary, E<'i>> {
        // `input` is delimited to the selector list (before `{`).
        Ok(parse_selectors(input))
    }

    fn parse_block<'t>(
        &mut self,
        prelude: SelectorSummary,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<(), E<'i>> {
        // Inside `@keyframes`, the "qualified rules" are `0% { … }` stops,
        // not style rules — count nothing, but still scan their bodies.
        if !self.in_keyframes {
            self.rule_count += 1;
            self.selector_count += prelude.selectors;
            self.kinds.class += prelude.class;
            self.kinds.id += prelude.id;
            self.kinds.element += prelude.element;
            self.kinds.pseudo += prelude.pseudo;
            self.kinds.attribute += prelude.attribute;
            self.kinds.universal += prelude.universal;
        }
        self.recurse_body(input);
        Ok(())
    }
}

/// Per-rule selector tally — merged into [`SelectorKindCounts`] once the
/// rule's block confirms it is real.
#[derive(Default)]
struct SelectorSummary {
    selectors: usize,
    class: usize,
    id: usize,
    element: usize,
    pseudo: usize,
    attribute: usize,
    universal: usize,
}

/// Split a selector list on top-level commas and tally per-kind
/// occurrences. Counts occurrences, not selectors — `.a.b` adds 2 to
/// `class`.
fn parse_selectors(input: &mut Parser) -> SelectorSummary {
    let mut sum = SelectorSummary::default();
    // Whether the current comma-separated part has any tokens yet.
    let mut started = false;
    // The previous token named a class (`.x`) or pseudo (`:x`), so a
    // following ident is that name, not an element.
    let mut prev_name_sigil = false;
    // Inside a run of consecutive colons (`::`), which counts once.
    let mut colon_run = false;

    loop {
        let token = match input.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        match token {
            Token::Comma => {
                if started {
                    sum.selectors += 1;
                }
                started = false;
                prev_name_sigil = false;
                colon_run = false;
            }
            Token::Delim('.') => {
                started = true;
                sum.class += 1;
                prev_name_sigil = true;
                colon_run = false;
            }
            Token::Delim('*') => {
                started = true;
                sum.universal += 1;
                prev_name_sigil = false;
                colon_run = false;
            }
            Token::Colon => {
                started = true;
                if !colon_run {
                    sum.pseudo += 1;
                }
                colon_run = true;
                prev_name_sigil = true;
            }
            Token::Hash(_) | Token::IDHash(_) => {
                started = true;
                sum.id += 1;
                prev_name_sigil = false;
                colon_run = false;
            }
            Token::Ident(_) => {
                started = true;
                if !prev_name_sigil {
                    sum.element += 1;
                }
                prev_name_sigil = false;
                colon_run = false;
            }
            Token::SquareBracketBlock => {
                started = true;
                sum.attribute += 1;
                drain_block(input);
                prev_name_sigil = false;
                colon_run = false;
            }
            Token::Function(_) | Token::ParenthesisBlock => {
                // A functional pseudo (`:not(…)`) — the `:` already
                // counted; the arguments are not added to the histogram.
                started = true;
                drain_block(input);
                prev_name_sigil = false;
                colon_run = false;
            }
            Token::CurlyBracketBlock => drain_block(input),
            // Combinators (` `, `>`, `+`, `~`), `&`, namespaces — these
            // continue the current part without naming a kind.
            _ => {
                prev_name_sigil = false;
                colon_run = false;
            }
        }
    }
    if started {
        sum.selectors += 1;
    }
    sum
}

// --- At-rules --------------------------------------------------------------

impl<'i> AtRuleParser<'i> for CssScanner {
    type Prelude = AtPrelude;
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<AtPrelude, E<'i>> {
        let prelude = match strip_vendor(&name.to_ascii_lowercase()) {
            "import" => AtPrelude::Import(scan_first_url(input)),
            "media" => AtPrelude::Media,
            "keyframes" => AtPrelude::Keyframes,
            "supports" | "layer" | "container" | "document" | "scope" | "starting-style" => {
                AtPrelude::NestedRules
            }
            "font-face"
            | "page"
            | "property"
            | "counter-style"
            | "font-feature-values"
            | "font-palette-values" => AtPrelude::Declarations,
            _ => AtPrelude::Other,
        };
        // Drain anything the prelude scan didn't consume.
        while input.next().is_ok() {}
        Ok(prelude)
    }

    fn rule_without_block(&mut self, prelude: AtPrelude, _start: &ParserState) -> Result<(), ()> {
        if let AtPrelude::Import(Some(url)) = prelude {
            let external = is_external(&url);
            self.imports.push(CssImport { url, external });
        }
        Ok(())
    }

    fn parse_block<'t>(
        &mut self,
        prelude: AtPrelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<(), E<'i>> {
        match prelude {
            AtPrelude::Media => {
                self.media_query_count += 1;
                self.recurse_body(input);
            }
            AtPrelude::Keyframes => {
                self.keyframes_count += 1;
                let outer = self.in_keyframes;
                self.in_keyframes = true;
                self.recurse_body(input);
                self.in_keyframes = outer;
            }
            AtPrelude::NestedRules | AtPrelude::Declarations | AtPrelude::Other => {
                self.recurse_body(input);
            }
            // `@import` never carries a block.
            AtPrelude::Import(_) => {}
        }
        Ok(())
    }
}

enum AtPrelude {
    /// `@import` — the resolved URL, when one was found.
    Import(Option<String>),
    Media,
    Keyframes,
    /// `@supports` / `@layer` / `@container` / … — body is a rule list.
    NestedRules,
    /// `@font-face` / `@page` / … — body is a declaration list.
    Declarations,
    /// Anything else — consumed for balance, nothing counted.
    Other,
}

impl<'i> RuleBodyItemParser<'i, (), ()> for CssScanner {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        true
    }
}

// --- Helpers ---------------------------------------------------------------

/// Find the first string / `url()` target in an `@import` prelude.
fn scan_first_url(input: &mut Parser) -> Option<String> {
    let mut found: Option<String> = None;
    loop {
        let token = match input.next() {
            Ok(t) => t.clone(),
            Err(_) => break,
        };
        match token {
            Token::QuotedString(s) | Token::UnquotedUrl(s) => {
                found.get_or_insert_with(|| s.as_ref().to_string());
            }
            Token::Function(_) | Token::ParenthesisBlock => {
                // `url("…")` — the string lives inside the function block.
                let inner = capture_string(input);
                if let Some(u) = inner {
                    found.get_or_insert(u);
                }
            }
            Token::CurlyBracketBlock | Token::SquareBracketBlock => drain_block(input),
            _ => {}
        }
    }
    found
}

/// Pull the first string token out of a just-opened block.
fn capture_string(input: &mut Parser) -> Option<String> {
    let mut found: Option<String> = None;
    let _ = input.parse_nested_block::<_, _, ()>(|p| {
        while let Ok(token) = p.next() {
            if let Token::QuotedString(s) | Token::UnquotedUrl(s) = token {
                found.get_or_insert_with(|| s.as_ref().to_string());
            }
        }
        Ok(())
    });
    found
}

/// Consume a just-opened block, discarding its contents.
fn drain_block(input: &mut Parser) {
    let _ = input.parse_nested_block::<_, _, ()>(|p| {
        while p.next().is_ok() {}
        Ok(())
    });
}

/// Resolve a parsed colour to an sRGB triple. The CIE / Oklab spaces and
/// `currentColor` need more context than a swatch can carry, so they are
/// left out of the palette.
fn color_to_rgb(color: &Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgba(c) => {
            // A fully-transparent literal (`transparent`) has no swatch.
            if c.alpha == 0.0 {
                None
            } else {
                Some((c.red, c.green, c.blue))
            }
        }
        Color::Hsl(c) => {
            let (r, g, b) = hsl_to_rgb(
                c.hue.unwrap_or(0.0) / 360.0,
                c.saturation.unwrap_or(0.0),
                c.lightness.unwrap_or(0.0),
            );
            Some(unit_rgb(r, g, b))
        }
        Color::Hwb(c) => {
            let (r, g, b) = hwb_to_rgb(
                c.hue.unwrap_or(0.0) / 360.0,
                c.whiteness.unwrap_or(0.0),
                c.blackness.unwrap_or(0.0),
            );
            Some(unit_rgb(r, g, b))
        }
        _ => None,
    }
}

fn unit_rgb(r: f32, g: f32, b: f32) -> (u8, u8, u8) {
    let conv = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (conv(r), conv(g), conv(b))
}

/// Strip a leading vendor prefix so `-webkit-keyframes` routes like
/// `keyframes`.
fn strip_vendor(name: &str) -> &str {
    for prefix in ["-webkit-", "-moz-", "-o-", "-ms-"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest;
        }
    }
    name
}

fn is_external(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("//")
        || u.starts_with("http://")
        || u.starts_with("https://")
        || u.starts_with("ftp://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_rules_and_selectors() {
        let css = ".a, .b { color: red } div p { color: blue }";
        let s = gather(css);
        assert_eq!(s.rule_count, 2);
        assert_eq!(s.selector_count, 3);
        assert_eq!(s.selector_kinds.class, 2);
        assert_eq!(s.selector_kinds.element, 2);
    }

    #[test]
    fn classifies_selector_kinds() {
        let css = "#main a[href]:hover::before { color: #fff } * { margin: 0 }";
        let s = gather(css);
        assert_eq!(s.selector_kinds.id, 1);
        assert_eq!(s.selector_kinds.element, 1);
        assert_eq!(s.selector_kinds.attribute, 1);
        // `:hover` and `::before` are two pseudos; only a literal `::`
        // double-colon collapses to one.
        assert_eq!(s.selector_kinds.pseudo, 2);
        assert_eq!(s.selector_kinds.universal, 1);
    }

    #[test]
    fn custom_properties_are_deduped() {
        let css = ":root { --x: 1; --y: 2 } .a { --x: 3; color: var(--y) }";
        let s = gather(css);
        assert_eq!(s.custom_property_count, 2);
    }

    #[test]
    fn at_rules_counted() {
        let css = "@media (min-width: 1px) { .a { color: red } } \
                   @keyframes spin { from { opacity: 0 } to { opacity: 1 } }";
        let s = gather(css);
        assert_eq!(s.media_query_count, 1);
        assert_eq!(s.keyframes_count, 1);
        // Only the `.a` rule counts — keyframe stops do not.
        assert_eq!(s.rule_count, 1);
    }

    #[test]
    fn nested_rules_are_counted() {
        let css = ".card { color: red; & > h2 { color: blue } &:hover { color: lime } }";
        let s = gather(css);
        // `.card`, `& > h2`, `&:hover`.
        assert_eq!(s.rule_count, 3);
    }

    #[test]
    fn imports_flag_external() {
        let css = "@import \"local.css\"; \
                   @import url(\"https://cdn.example.com/x.css\");";
        let s = gather(css);
        assert_eq!(s.imports.len(), 2);
        assert!(!s.imports[0].external);
        assert_eq!(s.imports[0].url, "local.css");
        assert!(s.imports[1].external);
    }

    #[test]
    fn colors_collected_and_deduped() {
        let css = ".a { color: #ff0000; background: red } .b { color: rgb(255, 0, 0) }";
        let s = gather(css);
        // `#ff0000`, `red`, and `rgb(255,0,0)` are the same colour.
        assert_eq!(s.total_colors, 1);
        assert_eq!(s.palette.len(), 1);
        assert_eq!(s.palette[0].rgb, (255, 0, 0));
        assert_eq!(s.palette[0].hex, "#ff0000");
        assert_eq!(s.palette[0].count, 3);
    }

    #[test]
    fn no_false_colour_from_strings_or_selectors() {
        // `red` appears as a class name, a string, and a comment — none
        // is a colour use.
        let css = ".red { content: \"red is a word\" } /* red */ .x { width: 1px }";
        let s = gather(css);
        assert_eq!(s.total_colors, 0);
        assert_eq!(s.selector_kinds.class, 2);
    }

    #[test]
    fn hsl_resolves_to_rgb() {
        let s = gather(".a { color: hsl(0, 100%, 50%) }");
        assert_eq!(s.palette.len(), 1);
        assert_eq!(s.palette[0].rgb, (255, 0, 0));
    }
}
