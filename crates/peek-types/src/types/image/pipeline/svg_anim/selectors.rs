//! CSS rule extraction for `svg_anim`. Walks a style-sheet text and
//! collects rule blocks whose declaration body references `animation`,
//! parsed into (matchers, decls) pairs.
//!
//! Selectors recognized are flat: a single tag, class, id, or
//! `tag.class` chain. Combinators (` `, `>`, `+`, `~`),
//! pseudo-classes (`:hover`), attribute selectors (`[foo]`), and the
//! universal selector (`*`) are detected and silently dropped — keeps
//! the matcher API tiny and SVG animation files in the wild rarely
//! depend on those.
//!
//! `@keyframes` and other at-rules are skipped here; keyframe stops
//! are parsed by [`super::keyframes`].

use super::util::{find_matching_brace, skip_ws};

pub(super) struct CssRule {
    pub matchers: Vec<Matcher>,
    pub decls: String,
}

pub(super) enum Matcher {
    Class(String),
    Id(String),
    Tag(String),
    TagClass { tag: String, class: String },
}

impl Matcher {
    pub fn matches(&self, tag: &str, classes: &[String], id: Option<&str>) -> bool {
        match self {
            Matcher::Class(c) => classes.iter().any(|x| x == c),
            Matcher::Id(i) => id == Some(i.as_str()),
            Matcher::Tag(t) => tag.eq_ignore_ascii_case(t),
            Matcher::TagClass { tag: t, class: c } => {
                tag.eq_ignore_ascii_case(t) && classes.iter().any(|x| x == c)
            }
        }
    }
}

/// Walk the style-sheet text, collect every flat-selector rule whose
/// declaration body references `animation`. Returns rules in CSS source
/// order so the caller can preserve cascade order when merging decls.
pub(super) fn parse_rules(css: &str) -> Vec<CssRule> {
    let mut out = Vec::new();
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        i = skip_ws(css, i);
        if i >= bytes.len() {
            break;
        }
        // Skip at-rules (`@keyframes`, `@media`, ...) entirely.
        if bytes[i] == b'@' {
            let Some(brace) = css[i..].find('{') else {
                break;
            };
            let body_start = i + brace + 1;
            let Some(end) = find_matching_brace(css, body_start) else {
                break;
            };
            i = end + 1;
            continue;
        }
        let sel_start = i;
        let Some(brace) = css[i..].find('{') else {
            break;
        };
        let sel_end = i + brace;
        let selector_text = &css[sel_start..sel_end];
        let body_start = sel_end + 1;
        let Some(body_end) = find_matching_brace(css, body_start) else {
            break;
        };
        let body = &css[body_start..body_end];
        if body.contains("animation") {
            let matchers = parse_selector_list(selector_text);
            if !matchers.is_empty() {
                out.push(CssRule {
                    matchers,
                    decls: body.to_string(),
                });
            }
        }
        i = body_end + 1;
    }
    out
}

fn parse_selector_list(text: &str) -> Vec<Matcher> {
    let mut out = Vec::new();
    for piece in text.split(',') {
        if let Some(m) = parse_one_selector(piece.trim()) {
            out.push(m);
        }
    }
    out
}

fn parse_one_selector(s: &str) -> Option<Matcher> {
    if s.is_empty() {
        return None;
    }
    if s.contains(|c: char| {
        c.is_whitespace() || c == '>' || c == '+' || c == '~' || c == ':' || c == '[' || c == '*'
    }) {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[0] == b'.' {
        let class = s[1..].to_string();
        if class.is_empty() || class.contains('.') || class.contains('#') {
            return None;
        }
        return Some(Matcher::Class(class));
    }
    if bytes[0] == b'#' {
        let id = s[1..].to_string();
        if id.is_empty() || id.contains('.') || id.contains('#') {
            return None;
        }
        return Some(Matcher::Id(id));
    }
    if s.contains('#') {
        return None;
    }
    match s.find('.') {
        None => Some(Matcher::Tag(s.to_string())),
        Some(p) => {
            let tag = &s[..p];
            let class = &s[p + 1..];
            if tag.is_empty() || class.is_empty() || class.contains('.') {
                return None;
            }
            Some(Matcher::TagClass {
                tag: tag.to_string(),
                class: class.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes_of(rule: &CssRule) -> Vec<&str> {
        rule.matchers
            .iter()
            .filter_map(|m| match m {
                Matcher::Class(c) => Some(c.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn extracts_class_rule_with_animation() {
        let css = ".dot{animation:spin 1s infinite}";
        let rules = parse_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(classes_of(&rules[0]), vec!["dot"]);
        assert!(rules[0].decls.contains("animation"));
    }

    #[test]
    fn skips_keyframes_at_rule() {
        let css = "@keyframes spin{0%{transform:translateX(0)}}.dot{animation:spin 1s}";
        let rules = parse_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(classes_of(&rules[0]), vec!["dot"]);
    }

    #[test]
    fn skips_rule_without_animation_decl() {
        let css = ".plain{fill:red}.dot{animation:spin 1s}";
        let rules = parse_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(classes_of(&rules[0]), vec!["dot"]);
    }

    #[test]
    fn rejects_combinators_and_pseudo() {
        let css = ".a .b{animation:x 1s}.c>.d{animation:x 1s}.e:hover{animation:x 1s}";
        let rules = parse_rules(css);
        assert!(rules.is_empty());
    }

    #[test]
    fn parses_id_and_tagclass() {
        let css = "#main{animation:x 1s}circle.foo{animation:y 1s}";
        let rules = parse_rules(css);
        assert_eq!(rules.len(), 2);
        match &rules[0].matchers[0] {
            Matcher::Id(i) => assert_eq!(i, "main"),
            _ => panic!("expected Id"),
        }
        match &rules[1].matchers[0] {
            Matcher::TagClass { tag, class } => {
                assert_eq!(tag, "circle");
                assert_eq!(class, "foo");
            }
            _ => panic!("expected TagClass"),
        }
    }

    #[test]
    fn comma_separated_selector_list() {
        let css = ".a, #b, span.c{animation:x 1s}";
        let rules = parse_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].matchers.len(), 3);
    }

    #[test]
    fn matches_class() {
        let m = Matcher::Class("dot".into());
        assert!(m.matches("circle", &["dot".into()], None));
        assert!(m.matches("circle", &["a".into(), "dot".into()], None));
        assert!(!m.matches("circle", &["a".into()], None));
    }

    #[test]
    fn matches_tag_case_insensitive() {
        let m = Matcher::Tag("CIRCLE".into());
        assert!(m.matches("circle", &[], None));
    }

    #[test]
    fn matches_id() {
        let m = Matcher::Id("main".into());
        assert!(m.matches("g", &[], Some("main")));
        assert!(!m.matches("g", &[], Some("other")));
        assert!(!m.matches("g", &[], None));
    }

    #[test]
    fn matches_tag_class() {
        let m = Matcher::TagClass {
            tag: "circle".into(),
            class: "dot".into(),
        };
        assert!(m.matches("circle", &["dot".into()], None));
        assert!(!m.matches("rect", &["dot".into()], None));
        assert!(!m.matches("circle", &["other".into()], None));
    }
}
