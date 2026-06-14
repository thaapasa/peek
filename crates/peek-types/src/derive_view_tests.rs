//! Behavioural tests for `#[derive(InfoView)]`'s skip-predicate resolution.
//!
//! The derive lives in `peek-foundation-derive`, but its generated paths
//! resolve only through `::peek_foundation`, so it can only be *exercised* from
//! a crate that depends on the foundation — this one. These synthetic view
//! structs pin the precedence the macro documents:
//!
//! `skip_if_zero` > `skip_if = "path"` > `serde(skip_serializing_if)`, with
//! `no_skip` overriding all three to always print the row. Each test makes the
//! competing rules *disagree*, so the surviving row proves which one won —
//! reordering the precedence checks in the macro breaks exactly one test.

use serde::Serialize;

use crate::info::{InfoNode, InfoView, Value};
use peek_theme::{PeekTheme, PeekThemeName, StyleMode, load_embedded_theme};

fn plain_theme() -> PeekTheme {
    let mut t = PeekTheme::from_syntect(&load_embedded_theme(
        PeekThemeName::default().tmtheme_source(),
    ));
    t.style_mode = StyleMode::Plain;
    t
}

/// Flatten a view's nodes into `(label, value)` rows.
fn rows(view: &impl InfoView) -> Vec<(String, String)> {
    fn collect(node: &InfoNode, out: &mut Vec<(String, String)>) {
        match node {
            InfoNode::Row { label, value } => out.push((label.to_string(), value.clone())),
            InfoNode::Block { body, .. } => body.iter().for_each(|c| collect(c, out)),
            InfoNode::Line(_) => {}
        }
    }
    let mut out = Vec::new();
    for node in view.info_nodes(&plain_theme()) {
        collect(&node, &mut out);
    }
    out
}

fn labels(view: &impl InfoView) -> Vec<String> {
    rows(view).into_iter().map(|(l, _)| l).collect()
}

// --- skip predicates referenced by `skip_if = "…"` -------------------------

fn is_empty_str(s: &str) -> bool {
    s.is_empty()
}
fn never_opt(_: &Option<String>) -> bool {
    false
}
// Referenced by `ZeroBeatsPredView`'s `skip_if`, but the derive resolves
// `skip_if_zero` first and never emits the call — so this staying uncalled is
// itself the precedence proof. Silence the resulting dead-code lint.
#[allow(dead_code)]
fn never_val(_: &Value) -> bool {
    false
}

// --- `skip_if_zero`: hide a numeric zero, keep non-zero --------------------

#[derive(Serialize, InfoView)]
#[info(title = "Zero")]
struct ZeroView {
    #[info(label = "Kept")]
    kept: Value,
    #[info(label = "Hidden", skip_if_zero)]
    hidden: Value,
}

#[test]
fn skip_if_zero_hides_only_zero() {
    let z = ZeroView {
        kept: Value::count(1),
        hidden: Value::count(0),
    };
    assert_eq!(labels(&z), ["Kept"], "zero field must be hidden");

    let nz = ZeroView {
        kept: Value::count(1),
        hidden: Value::count(5),
    };
    assert_eq!(labels(&nz), ["Kept", "Hidden"], "non-zero field shows");
}

// --- `skip_if = "path"`: custom predicate ----------------------------------

#[derive(Serialize, InfoView)]
#[info(title = "Pred")]
struct PredView {
    #[info(label = "Name", skip_if = "is_empty_str")]
    name: String,
}

#[test]
fn skip_if_predicate_hides_when_true() {
    assert!(
        labels(&PredView {
            name: String::new()
        })
        .is_empty()
    );
    assert_eq!(labels(&PredView { name: "x".into() }), ["Name"]);
}

// --- `serde(skip_serializing_if)` mirrored into the print row --------------

#[derive(Serialize, InfoView)]
#[info(title = "Serde")]
struct SerdeSkipView {
    #[info(label = "Opt")]
    #[serde(skip_serializing_if = "Option::is_none")]
    opt: Option<String>,
}

#[test]
fn serde_skip_is_mirrored_in_print() {
    assert!(labels(&SerdeSkipView { opt: None }).is_empty());
    assert_eq!(
        labels(&SerdeSkipView {
            opt: Some("y".into())
        }),
        ["Opt"]
    );
}

// --- precedence: `no_skip` overrides a serde skip (highest precedence) ------

#[derive(Serialize, InfoView)]
#[info(title = "NoSkip")]
struct NoSkipView {
    #[info(label = "Always", no_skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    val: Option<String>,
}

#[test]
fn no_skip_forces_row_despite_serde_skip() {
    // serde would omit the None field from JSON; `no_skip` still prints the
    // row (with an empty value). Proves no_skip beats serde skip.
    let printed = rows(&NoSkipView { val: None });
    assert_eq!(printed, [("Always".to_string(), String::new())]);
}

// --- precedence: `skip_if` beats `serde(skip_serializing_if)` ---------------

#[derive(Serialize, InfoView)]
#[info(title = "PredVsSerde")]
struct PredBeatsSerdeView {
    // skip_if = never → keep the row; serde would skip None. skip_if wins.
    #[info(label = "Forced", skip_if = "never_opt")]
    #[serde(skip_serializing_if = "Option::is_none")]
    val: Option<String>,
}

#[test]
fn skip_if_beats_serde_skip() {
    assert_eq!(labels(&PredBeatsSerdeView { val: None }), ["Forced"]);
}

// --- precedence: `skip_if_zero` beats `skip_if` -----------------------------

#[derive(Serialize, InfoView)]
#[info(title = "ZeroVsPred")]
struct ZeroBeatsPredView {
    // zero rule and a never-skip predicate disagree on a zero value; the zero
    // rule wins and the row vanishes.
    #[info(label = "N", skip_if_zero, skip_if = "never_val")]
    n: Value,
}

#[test]
fn skip_if_zero_beats_skip_if() {
    assert!(labels(&ZeroBeatsPredView { n: Value::count(0) }).is_empty());
    assert_eq!(labels(&ZeroBeatsPredView { n: Value::count(3) }), ["N"]);
}
