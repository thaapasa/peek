//! CSS `@keyframes` animation support for SVG.
//!
//! resvg/usvg do not evaluate CSS animations: a static SVG with
//! `@keyframes` rules and `animation: ...` properties renders as a single
//! frame (typically the rest state, since usvg ignores `animation-*`
//! properties it doesn't paint with). To turn such an SVG into a playable
//! sequence we:
//!
//! 1. Parse `<style>` for `@keyframes NAME { <pct>% { transform: ... } ... }`.
//! 2. Find elements whose inline `style="..."` references one of those names
//!    via `animation-name: NAME` / `animation: NAME ...`.
//! 3. Sample each target's timeline into discrete frames (one per stop for
//!    `steps()` timing, ~30 fps interpolated for `linear`/unspecified).
//! 4. Build a "marked SVG" once. For each animated element:
//!    - Outer wrapper `<g __PEEK_ANIM_TR_<i>__>...</g>` carries the
//!      per-frame transform.
//!    - Property animations are baked in by rewriting the original
//!      opening tag: each animated attribute (`r`, `cx`, ...) is
//!      excised and a `__PEEK_ANIM_S_<i>_<j>__` slot placeholder is
//!      added before the closing `>`/`/>`. [`render_frame`] then
//!      substitutes ` name="value"` (or restores the original) per
//!      sample.
//!
//! The `<g>` wrapper exists because resvg/usvg currently ignore the
//! `transform` attribute when applied directly to a nested `<svg>`
//! element (covered by `nested_svg_transform_actually_shifts_content`).
//! Self-closing leaves and container tags wrap uniformly.
//!
//! Property animation can't go through CSS: usvg 0.47 does not honor
//! `<style>` rules for SVG geometric properties (verified
//! experimentally — `<style>circle{r:5px}</style>` doesn't change `r`).
//! Direct attribute substitution sidesteps that limitation.
//!
//! Scope: CSS keyframes only (no SMIL `<animate>`); inline-style and
//! flat class/id/tag/`tag.class` selector targets. Animatable
//! properties: `transform: translateX/Y` and `translate(x, y)` plus
//! arbitrary CSS properties (`r`, `cx`, `cy`, `opacity`,
//! `stroke-width`, ...) — numeric+unit values interpolate between
//! stops on `linear` timing, and any value steps under `steps()`
//! timing. Selector combinators, pseudo-classes, attribute selectors,
//! and `*` are dropped silently. Unknown timing functions are treated
//! as `linear`.
//!
//! `animation-delay` (longhand and as the second time token in the
//! `animation:` shorthand) is honored — pre-delay holds the un-animated
//! state, then the iteration begins. Caveat: delay applies *each* loop
//! pass in our merged-timeline model, where CSS only applies it once
//! before the first iteration. For staggered fixtures the visual
//! effect is correct on the first cycle and mostly so on repeats.
//!
//! Module layout:
//! - [`scan`]      — quick-xml walk; collects element class/id/style + `<style>` text.
//! - [`spec`]      — `animation:` / `animation-*` declaration parser → [`spec::AnimSpec`].
//! - [`keyframes`] — CSS `@keyframes` rule parser → [`keyframes::KeyframeStop`].
//! - [`selectors`] — flat-selector CSS rule parser → [`selectors::CssRule`].
//! - [`timeline`]  — merged frame timeline construction + per-target sampling.
//! - [`marker`]    — `__PEEK_ANIM_*__` marker injection + [`render_frame`] substitution.
//! - [`util`]      — small shared helpers.

use std::time::Duration;

use anyhow::{Context, Result};

use crate::input::InputSource;

mod keyframes;
mod marker;
mod scan;
mod selectors;
mod spec;
mod timeline;
mod util;

pub use marker::render_frame;

use timeline::FrameTarget;

/// Parsed animation model + a pre-marked SVG string ready for per-frame
/// substitution.
pub struct AnimatedSvg {
    /// SVG source with `__PEEK_ANIM_TR_<i>__` (transform) and
    /// `__PEEK_ANIM_S_<i>_<j>__` (per-prop attribute slot) placeholders
    /// pre-injected. Per-frame rendering replaces every placeholder.
    pub marked: String,
    /// Total animation duration (one iteration).
    pub duration: Duration,
    /// True when iteration count is `infinite`.
    pub infinite: bool,
    /// Merged frame timeline across all targets. One entry per visible
    /// transition; `delay` is the time to hold the frame before advancing.
    pub frames: Vec<Frame>,
    /// SVG viewport pixel size (width, height) — for downstream
    /// rasterization.
    pub width_px: u32,
    pub height_px: u32,
}

/// One playable frame in the merged timeline.
pub struct Frame {
    /// Time the frame holds before advancing (saturates against duration
    /// for the final frame so a full loop totals `duration`).
    pub delay: Duration,
    /// Per-target render state at this frame, parallel to the target
    /// list resolved during parse. Private — only consumed by the
    /// marker submodule and the in-module tests.
    targets: Vec<FrameTarget>,
}

/// Per-target join of [`scan::RawElement`] (byte spans + tag) with the
/// keyframe stops referenced by name + per-target prop bookkeeping.
/// Built in [`parse_text`], consumed by [`timeline::build_frames`]
/// (sampling) and [`marker::build_marked_svg`] (opening-tag rewrite).
struct ResolvedTarget {
    open_at: usize,
    close_at: usize,
    open_tag_end: usize,
    spec: spec::AnimSpec,
    stops: Vec<keyframes::KeyframeStop>,
    /// Distinct CSS property names mutated by any keyframe stop, in
    /// source order. Each entry corresponds to one `__PEEK_ANIM_S_<i>_<j>__`
    /// placeholder in the marked SVG.
    animated_props: Vec<String>,
    /// Original attribute value per `animated_props` index, captured
    /// from the element's opening tag. `None` = attribute was not
    /// present originally; pre-delay frames render no slot at all.
    original_values: Vec<Option<String>>,
}

/// Outcome of attempting to interpret an SVG as a peek-renderable
/// animation. The plain `try_parse_*` wrappers below collapse this
/// into `Option<AnimatedSvg>` for the viewer; the info path wants the
/// `Unsupported` reason to surface as a warning.
pub enum ParseOutcome {
    /// SVG had no animation hints (no `@keyframes`, no `animation:`,
    /// no `animation-name`).
    NotAnimated,
    /// SVG declared an animation that peek can play.
    Animated(AnimatedSvg),
    /// SVG declared an animation that peek cannot play (unsupported
    /// feature, malformed, or rasterization probe failed). The reason
    /// is human-readable for the info-view warning row.
    Unsupported(String),
}

/// Try to extract a CSS animation model from an SVG source.
pub fn try_parse(source: &InputSource) -> Result<Option<AnimatedSvg>> {
    let bytes = source
        .read_bytes()
        .context("failed to read SVG for animation parse")?;
    Ok(try_parse_bytes(&bytes))
}

/// Same as [`try_parse`] but operates on an in-memory SVG byte buffer.
pub fn try_parse_bytes(bytes: &[u8]) -> Option<AnimatedSvg> {
    match diagnose_bytes(bytes) {
        ParseOutcome::Animated(m) => Some(m),
        _ => None,
    }
}

/// Diagnostic variant: returns the rich [`ParseOutcome`] so callers
/// (like the info gatherer) can surface why an animation was rejected.
pub fn diagnose_bytes(bytes: &[u8]) -> ParseOutcome {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return ParseOutcome::NotAnimated;
    };
    parse_text(text)
}

fn parse_text(text: &str) -> ParseOutcome {
    if !contains_anim_marker(text) {
        if contains_smil(text) {
            return ParseOutcome::Unsupported(
                "SMIL animation (<animate>, <animateMotion>, ...) not supported yet".into(),
            );
        }
        return ParseOutcome::NotAnimated;
    }
    let scanned = scan::scan_svg(text);
    let kf = keyframes::parse_keyframes(&scanned.style_text);
    if kf.is_empty() {
        return ParseOutcome::Unsupported(
            "animation references found but no parseable @keyframes rule".into(),
        );
    }
    let rules = selectors::parse_rules(&scanned.style_text);

    let mut targets: Vec<ResolvedTarget> = Vec::new();
    for el in scanned.elements {
        let Some(combined) = combine_decls(&rules, &el) else {
            continue;
        };
        let Some(spec) = spec::parse_anim_spec(&combined) else {
            continue;
        };
        let Some(stops) = kf.get(&spec.name) else {
            continue;
        };
        let stops = stops.clone();
        let animated_props = collect_animated_props(&stops);
        let opening_tag = &text[el.open_at..el.open_tag_end];
        let original_values: Vec<Option<String>> = animated_props
            .iter()
            .map(|name| marker::find_attr_value(opening_tag, name))
            .collect();
        targets.push(ResolvedTarget {
            open_at: el.open_at,
            close_at: el.close_at,
            open_tag_end: el.open_tag_end,
            spec,
            stops,
            animated_props,
            original_values,
        });
    }
    if targets.is_empty() {
        return ParseOutcome::Unsupported(
            "no element matches a known @keyframes rule (selectors / SMIL not supported)".into(),
        );
    }

    let duration = targets
        .iter()
        .map(|t| t.spec.duration + t.spec.delay)
        .fold(Duration::from_millis(0), |a, b| if b > a { b } else { a });
    if duration.is_zero() {
        return ParseOutcome::Unsupported("animation duration is zero".into());
    }
    let infinite = targets.iter().any(|t| t.spec.infinite);

    let (width_px, height_px) = util::root_svg_dimensions(text).unwrap_or((1, 1));
    let frames = timeline::build_frames(&targets, duration);
    if frames.len() < 2 {
        // Resolved targets but no visible animation — every sample
        // produced an identical FrameTarget vector. Probably an
        // unsupported keyframe property (rotate, scale, color, ...).
        return ParseOutcome::Unsupported(
            "keyframes mutate properties peek can't animate yet (only transform translate, \
             plus numeric/length CSS properties via attribute substitution)"
                .into(),
        );
    }
    let marked = marker::build_marked_svg(text, &targets);

    // Sanity-probe: rasterize frame 0 at a small size. If this fails
    // the markup is broken (out of our control or ours) — refuse to
    // ship the animation rather than letting the viewer crash mid-play.
    let model = AnimatedSvg {
        marked,
        duration,
        infinite,
        frames,
        width_px,
        height_px,
    };
    let probe = marker::render_frame(&model, 0);
    if let Err(e) = super::svg::rasterize_svg_bytes(probe.as_bytes(), 32, 32) {
        return ParseOutcome::Unsupported(format!("animated SVG fails to rasterize: {e}"));
    }
    ParseOutcome::Animated(model)
}

/// Concatenate declaration blocks that apply to `el`: every matching
/// rule's body in CSS source order, then the element's inline `style=`
/// (if any). The result is fed straight to [`spec::parse_anim_spec`],
/// whose declaration walk is order-sensitive — the inline style trails
/// the rule decls so its longhands take precedence.
fn combine_decls(rules: &[selectors::CssRule], el: &scan::RawElement) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for rule in rules {
        if rule
            .matchers
            .iter()
            .any(|m| m.matches(&el.tag, &el.classes, el.id.as_deref()))
        {
            parts.push(&rule.decls);
        }
    }
    let inline_anim = el
        .inline_style
        .as_ref()
        .is_some_and(|s| s.contains("animation"));
    if parts.is_empty() && !inline_anim {
        return None;
    }
    let mut out = parts.join(";");
    if let Some(s) = &el.inline_style {
        if !out.is_empty() && !out.ends_with(';') {
            out.push(';');
        }
        out.push_str(s);
    }
    Some(out)
}

fn contains_anim_marker(text: &str) -> bool {
    text.contains("@keyframes") || text.contains("animation:") || text.contains("animation-name")
}

/// True when the SVG carries a SMIL animation element (`<animate>`,
/// `<animateMotion>`, `<animateTransform>`, `<animateColor>`, `<set>`).
/// peek doesn't model SMIL — used only to surface a "not supported"
/// warning in the info view.
fn contains_smil(text: &str) -> bool {
    text.contains("<animate") || text.contains("<set ") || text.contains("<set/")
}

/// Collect distinct CSS property names that appear in any stop's
/// `props` list, preserving the order they were first encountered.
fn collect_animated_props(stops: &[keyframes::KeyframeStop]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in stops {
        for p in &s.props {
            if !out.iter().any(|n| n.eq_ignore_ascii_case(&p.name)) {
                out.push(p.name.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Three distinct stops, no 100% rule — termsvg-style filmstrip layout.
    const SAMPLE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
<style>@keyframes slide{0%{transform:translateX(0)}33%{transform:translateX(-100px)}66%{transform:translateX(-200px)}}</style>
<g style="animation-name:slide;animation-duration:3s;animation-iteration-count:infinite;animation-timing-function:steps(1,end)">
<rect width="200" height="100" fill="red"/>
</g>
</svg>"#;

    fn must_parse(text: &str) -> AnimatedSvg {
        match parse_text(text) {
            ParseOutcome::Animated(m) => m,
            ParseOutcome::NotAnimated => panic!("expected animation, got NotAnimated"),
            ParseOutcome::Unsupported(reason) => {
                panic!("expected animation, got Unsupported: {reason}")
            }
        }
    }

    #[test]
    fn detects_keyframes_and_targets() {
        let model = must_parse(SAMPLE);
        assert_eq!(model.duration, Duration::from_secs(3));
        assert!(model.infinite);
        assert_eq!(model.frames.len(), 3);
    }

    #[test]
    fn stepped_holds_value_until_next_stop() {
        let model = must_parse(SAMPLE);
        assert_eq!(model.frames[0].targets[0].transform, "");
        assert_eq!(model.frames[1].targets[0].transform, "translate(-100,0)");
        assert_eq!(model.frames[2].targets[0].transform, "translate(-200,0)");
    }

    #[test]
    fn render_frame_substitutes_marker() {
        let model = must_parse(SAMPLE);
        let f1 = render_frame(&model, 1);
        assert!(f1.contains("transform=\"translate(-100,0)\""));
        assert!(!f1.contains("__PEEK_ANIM_"));
    }

    #[test]
    fn no_keyframes_returns_none() {
        let plain = r#"<svg><rect/></svg>"#;
        assert!(matches!(
            parse_text(plain),
            ParseOutcome::NotAnimated | ParseOutcome::Unsupported(_)
        ));
    }

    #[test]
    fn patched_svg_parses_with_resvg() {
        let model = must_parse(SAMPLE);
        for i in 0..model.frames.len() {
            let bytes = render_frame(&model, i);
            // Round-trip through resvg/usvg to confirm injection produced
            // a valid document at every frame.
            super::super::svg::rasterize_svg_bytes(bytes.as_bytes(), 32, 16)
                .unwrap_or_else(|e| panic!("frame {i} failed to rasterize: {e}"));
        }
    }

    /// Verifies that resvg/usvg actually applies a `transform` attribute
    /// injected on a nested `<svg>` element (as the demo SVG / termsvg
    /// filmstrip layout requires). We render two frames whose only
    /// difference is the injected translate; if the rasterized pixmaps
    /// match, the transform was silently ignored — and the animation
    /// would visually freeze on frame 0.
    #[test]
    fn nested_svg_transform_actually_shifts_content() {
        // Outer viewBox = 100x40. Animated nested <svg> contains a green
        // rect at x=0 (frame 0) and a red rect at x=100 (frame 1). The
        // 0% stop is identity; the 100% stop translates left by 100, so
        // the red rect should be visible in frame 1.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40" viewBox="0 0 100 40">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-100px)}}</style>
<svg width="200" style="animation-name:m;animation-duration:2s;animation-iteration-count:infinite;animation-timing-function:steps(1,end)">
<rect x="0" y="0" width="100" height="40" fill="rgb(0,255,0)"/>
<rect x="100" y="0" width="100" height="40" fill="rgb(255,0,0)"/>
</svg>
</svg>"#;
        let model = must_parse(svg);
        assert_eq!(model.frames.len(), 2, "expected two distinct frames");
        let f0 = render_frame(&model, 0);
        let f1 = render_frame(&model, 1);
        let img0 = super::super::svg::rasterize_svg_bytes(f0.as_bytes(), 100, 40).expect("frame 0");
        let img1 = super::super::svg::rasterize_svg_bytes(f1.as_bytes(), 100, 40).expect("frame 1");
        let p0 = img0.to_rgba8().get_pixel(50, 20).0;
        let p1 = img1.to_rgba8().get_pixel(50, 20).0;
        assert_ne!(
            p0, p1,
            "transform on animated nested <svg> not applied: frame0={p0:?} frame1={p1:?}"
        );
        assert!(
            p0[1] > 200 && p0[0] < 50,
            "frame 0 should be green, got {p0:?}"
        );
        assert!(
            p1[0] > 200 && p1[1] < 50,
            "frame 1 should be red, got {p1:?}"
        );
    }

    /// Diagnostic: dump loader-dots marked SVG.
    #[test]
    fn dump_loader_dots() {
        if std::env::var("PEEK_DUMP_LOADER").is_err() {
            return;
        }
        let bytes = std::fs::read(format!(
            "{}/test-images/loader-dots.svg",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let m = try_parse_bytes(&bytes).expect("parsed");
        eprintln!("MARKED:\n{}\n", m.marked);
        for i in 0..m.frames.len().min(3) {
            let r = render_frame(&m, i);
            eprintln!("FRAME {i}:\n{r}\n");
        }
    }

    /// Diagnostic: dump a specific demo frame to `/tmp/peek-frame.png`.
    /// Off by default; enable with `PEEK_DUMP_FRAME=N cargo test
    /// dump_demo_frame_for_inspection`.
    #[test]
    fn dump_demo_frame_for_inspection() {
        let Ok(n_str) = std::env::var("PEEK_DUMP_FRAME") else {
            return;
        };
        let n: usize = n_str.parse().unwrap_or(5);
        let path = format!(
            "{}/test-images/airlock-demo.svg",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap();
        let model = try_parse_bytes(&bytes).expect("animated");
        let svg = render_frame(&model, n);
        std::fs::write("/tmp/peek-frame.svg", &svg).unwrap();
        let img = super::super::svg::rasterize_svg_bytes(svg.as_bytes(), 800, 480).unwrap();
        img.save("/tmp/peek-frame.png").unwrap();
        println!(
            "frame {n} of {}: wrote /tmp/peek-frame.{{svg,png}}",
            model.frames.len()
        );
    }

    #[test]
    fn smil_only_svg_returns_unsupported_warning() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><circle cx="10" cy="10" r="5"><animate attributeName="r" from="0" to="5" dur="1s" repeatCount="indefinite"/></circle></svg>"#;
        match parse_text(svg) {
            ParseOutcome::Unsupported(reason) => {
                assert!(reason.contains("SMIL"), "got: {reason}");
            }
            other => panic!("expected Unsupported, got {:?}", outcome_label(&other)),
        }
    }

    #[test]
    fn malformed_keyframes_returns_unsupported() {
        // `@keyframes` declared but the body is unparseable — there are
        // no stops, so kf is empty and parse_text bails with a reason.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><style>@keyframes m {}</style><circle class="dot" style="animation:m 1s infinite" cx="10" cy="10" r="5"/></svg>"#;
        match parse_text(svg) {
            ParseOutcome::Unsupported(_) => {}
            other => panic!("expected Unsupported, got {:?}", outcome_label(&other)),
        }
    }

    fn outcome_label(o: &ParseOutcome) -> &'static str {
        match o {
            ParseOutcome::NotAnimated => "NotAnimated",
            ParseOutcome::Animated(_) => "Animated",
            ParseOutcome::Unsupported(_) => "Unsupported",
        }
    }

    /// Sanity-check the airlock demo fixture: frame 0 should differ
    /// visually from a mid-animation frame.
    #[test]
    fn demo_svg_frames_diverge_when_available() {
        let path = format!(
            "{}/test-images/airlock-demo.svg",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).expect("airlock-demo.svg fixture missing");
        let model = try_parse_bytes(&bytes).expect("demo file is animated");
        assert!(model.frames.len() > 4, "expected many frames");
        let mid = model.frames.len() / 2;
        let f0 = render_frame(&model, 0);
        let fm = render_frame(&model, mid);
        let img0 = super::super::svg::rasterize_svg_bytes(f0.as_bytes(), 400, 240).unwrap();
        let imgm = super::super::svg::rasterize_svg_bytes(fm.as_bytes(), 400, 240).unwrap();
        let diff: u64 = img0
            .to_rgba8()
            .pixels()
            .zip(imgm.to_rgba8().pixels())
            .map(|(a, b)| {
                a.0.iter()
                    .zip(b.0.iter())
                    .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as u64)
                    .sum::<u64>()
            })
            .sum();
        assert!(
            diff > 1000,
            "frame 0 and frame {mid} should differ visually (sum-abs = {diff})"
        );
    }

    #[test]
    fn class_selector_resolves_animation() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-10px)}}.dot{animation:m 1s steps(1,end) infinite}</style>
<circle class="dot" cx="10" cy="10" r="2"/>
</svg>"#;
        let model = must_parse(svg);
        assert_eq!(model.duration, Duration::from_secs(1));
        assert!(model.infinite);
        assert!(
            model.frames.len() >= 2,
            "expected stepped class anim to yield multiple frames"
        );
        let f1 = render_frame(&model, 1);
        assert!(
            f1.contains("translate"),
            "frame 1 should carry translate transform: {f1}"
        );
    }

    #[test]
    fn id_selector_resolves_animation() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-10px)}}#a{animation:m 1s steps(1,end) infinite}</style>
<circle id="a" cx="10" cy="10" r="2"/>
</svg>"#;
        let model = must_parse(svg);
        assert!(model.frames.len() >= 2);
    }

    #[test]
    fn tag_class_selector_resolves_animation() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-10px)}}circle.foo{animation:m 1s steps(1,end) infinite}</style>
<circle class="foo" cx="10" cy="10" r="2"/>
<rect class="foo" width="10" height="10"/>
</svg>"#;
        let model = must_parse(svg);
        // Only the circle matches `circle.foo`; the rect (same class
        // but different tag) is ignored.
        let f1 = render_frame(&model, 1);
        assert!(f1.contains("translate"));
    }

    #[test]
    fn property_animation_resolves_and_renders() {
        // Class-resolved animation that only mutates `r` (no transform).
        // The per-frame style block targets the wrapper id; resvg
        // applies the `r` override over the element's presentation
        // attribute, producing a visible animation.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{r:0}50%{r:5px}}.dot{animation:m 1s steps(1,end) infinite}</style>
<circle class="dot" cx="10" cy="10" r="0" fill="red"/>
</svg>"#;
        let model = must_parse(svg);
        assert!(
            model.frames.len() >= 2,
            "stepped r animation should yield multiple frames"
        );
        // Slot strings carry the rebuilt attribute (e.g. ` r="5"`).
        let zero_idx = model
            .frames
            .iter()
            .position(|f| f.targets[0].slots[0].contains("r=\"0\""));
        let big_idx = model
            .frames
            .iter()
            .position(|f| f.targets[0].slots[0].contains("r=\"5"));
        assert!(zero_idx.is_some() && big_idx.is_some());
        // Pixel-level check: at the small-r frame the centre is red,
        // at the r=0 frame nothing draws there.
        let big = render_frame(&model, big_idx.unwrap());
        let small = render_frame(&model, zero_idx.unwrap());
        let img_big = super::super::svg::rasterize_svg_bytes(big.as_bytes(), 20, 20).unwrap();
        let img_small = super::super::svg::rasterize_svg_bytes(small.as_bytes(), 20, 20).unwrap();
        let p_big = img_big.to_rgba8().get_pixel(10, 10).0;
        let p_small = img_small.to_rgba8().get_pixel(10, 10).0;
        assert!(
            p_big[0] > 200 && p_big[1] < 50,
            "r=5 frame should paint red at centre, got {p_big:?}"
        );
        assert!(
            p_small[3] == 0 || (p_small[0] < 50 && p_small[3] < 50),
            "r=0 frame should leave centre transparent, got {p_small:?}"
        );
    }

    #[test]
    fn linear_property_interpolation_between_stops() {
        // 0%→100% linear `r:0`→`r:10`. At ~half the timeline `r` should
        // be ≈5. We assert the literal substring appears in some frame.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{r:0}100%{r:10}}.dot{animation:m 1s linear infinite}</style>
<circle class="dot" cx="10" cy="10" r="0"/>
</svg>"#;
        let model = must_parse(svg);
        // FPS sampling produces ~30 frames; some frame should land near
        // r=5 ± a touch.
        let mid_hit = model.frames.iter().any(|f| {
            f.targets[0].slots[0].contains("r=\"4") || f.targets[0].slots[0].contains("r=\"5")
        });
        assert!(
            mid_hit,
            "expected an interpolated frame with r near 5; slots: {:?}",
            model
                .frames
                .iter()
                .map(|f| f.targets[0].slots.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn merged_rules_combine_with_inline_style() {
        // Class rule supplies `animation:` shorthand; inline style
        // overrides duration. Verifies cascade: inline trails class.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-10px)}}.dot{animation:m 5s steps(1,end) infinite}</style>
<g class="dot" style="animation-duration:1s"></g>
</svg>"#;
        let model = must_parse(svg);
        assert_eq!(model.duration, Duration::from_secs(1));
    }

    #[test]
    fn animation_delay_longhand_extends_total() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}100%{transform:translateX(-10px)}}.dot{animation-name:m;animation-duration:1s;animation-delay:500ms;animation-iteration-count:infinite}</style>
<circle class="dot" cx="10" cy="10" r="2"/>
</svg>"#;
        let model = must_parse(svg);
        assert_eq!(model.duration, Duration::from_millis(1500));
        // Frame 0 is pre-delay → empty transform; some later frame
        // carries the post-delay translate.
        assert_eq!(model.frames[0].targets[0].transform, "");
        assert!(
            model
                .frames
                .iter()
                .any(|f| !f.targets[0].transform.is_empty()),
            "expected at least one post-delay frame with translate"
        );
    }

    #[test]
    fn animation_delay_shorthand_second_time_token() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}100%{transform:translateX(-10px)}}.dot{animation:m 1s 0.5s linear infinite}</style>
<circle class="dot" cx="10" cy="10" r="2"/>
</svg>"#;
        let model = must_parse(svg);
        assert_eq!(model.duration, Duration::from_millis(1500));
    }

    #[test]
    fn delay_creates_phase_offset_across_targets() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-5px)}}.a{animation:m 1s steps(1,end) infinite}.b{animation:m 1s 0.5s steps(1,end) infinite}</style>
<circle class="a" cx="10" cy="10" r="2"/>
<circle class="b" cx="20" cy="10" r="2"/>
</svg>"#;
        let model = must_parse(svg);
        // Total = max(1s+0, 1s+0.5s) = 1.5s.
        assert_eq!(model.duration, Duration::from_millis(1500));
        // Find a frame in which target a is animating but target b is
        // still pre-delay (proves the phase offset is preserved).
        let mid = model
            .frames
            .iter()
            .find(|f| !f.targets[0].transform.is_empty() && f.targets[1].transform.is_empty());
        assert!(
            mid.is_some(),
            "expected at least one frame with a animating, b pre-delay"
        );
    }

    #[test]
    fn anim_shorthand_recognized() {
        let svg = r#"<svg width="10" height="10">
<style>@keyframes m{0%{transform:translate(0,0)}100%{transform:translate(10,5)}}</style>
<g style="animation:m 1s linear infinite"></g>
</svg>"#;
        let model = must_parse(svg);
        assert_eq!(model.duration, Duration::from_secs(1));
        assert!(model.infinite);
        // Linear timing, infinite loop → last sample approaches (but does
        // not reach) the 100% value, since reaching 100% would wrap to 0.
        let last = model.frames.last().unwrap();
        assert!(last.targets[0].transform.starts_with("translate(9."));
    }
}
