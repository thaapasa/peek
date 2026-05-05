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
//! 4. Cache a "marked SVG" string with `__PEEK_ANIM_OPEN_<i>__` /
//!    `__PEEK_ANIM_CLOSE_<i>__` placeholders inserted immediately
//!    *outside* each animated element's tag span; per frame, the open
//!    marker becomes `<g transform="...">` and the close marker `</g>`.
//!
//! The `<g>` wrapper exists because resvg/usvg currently ignore the
//! `transform` attribute when applied directly to a nested `<svg>`
//! element (covered by `nested_svg_transform_actually_shifts_content`).
//! Wrapping the element from outside lifts the transform onto a parent
//! group whose effect cascades through the original element — works
//! identically for container and self-closing leaf tags.
//!
//! Scope: CSS keyframes only (no SMIL `<animate>`); inline-style and
//! flat class/id/tag/`tag.class` selector targets; `transform:
//! translateX/Y` and `translate(x, y)` properties only. Combinators,
//! pseudo-classes, attribute selectors, and `*` are dropped silently.
//! Unknown timing functions are treated as `linear`.
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

/// Parsed animation model + a pre-marked SVG string ready for per-frame
/// substitution.
pub struct AnimatedSvg {
    /// SVG source with `__PEEK_ANIM_<i>__` markers injected at each
    /// target's opening-tag attribute slot. Per-frame rendering replaces
    /// each marker with ` transform="..."` (or empty string).
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
    /// Per-target SVG `transform` attribute value at this frame. Empty
    /// string = no transform attribute injected.
    pub transforms: Vec<String>,
}

/// Per-target join of [`scan::RawTarget`] (byte spans + spec) with the
/// keyframe stops referenced by name. Built in [`parse_text`], consumed
/// by [`timeline::build_frames`] and [`marker::build_marked_svg`].
struct ResolvedTarget {
    open_at: usize,
    close_at: usize,
    spec: spec::AnimSpec,
    stops: Vec<keyframes::KeyframeStop>,
}

/// Try to extract a CSS animation model from an SVG source. Returns
/// `Ok(None)` when no animation references are found, or when none of
/// them resolve to known `@keyframes` rules.
pub fn try_parse(source: &InputSource) -> Result<Option<AnimatedSvg>> {
    let bytes = source
        .read_bytes()
        .context("failed to read SVG for animation parse")?;
    Ok(try_parse_bytes(&bytes))
}

/// Same as [`try_parse`] but operates on an in-memory SVG byte buffer.
pub fn try_parse_bytes(bytes: &[u8]) -> Option<AnimatedSvg> {
    let text = std::str::from_utf8(bytes).ok()?;
    parse_text(text)
}

fn parse_text(text: &str) -> Option<AnimatedSvg> {
    if !contains_anim_marker(text) {
        return None;
    }
    let scanned = scan::scan_svg(text);
    let kf = keyframes::parse_keyframes(&scanned.style_text);
    if kf.is_empty() {
        return None;
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
        targets.push(ResolvedTarget {
            open_at: el.open_at,
            close_at: el.close_at,
            spec,
            stops: stops.clone(),
        });
    }
    if targets.is_empty() {
        return None;
    }

    let duration = targets
        .iter()
        .map(|t| t.spec.duration)
        .fold(Duration::from_millis(0), |a, b| if b > a { b } else { a });
    if duration.is_zero() {
        return None;
    }
    let infinite = targets.iter().any(|t| t.spec.infinite);

    let (width_px, height_px) = util::root_svg_dimensions(text).unwrap_or((1, 1));
    let frames = timeline::build_frames(&targets, duration);
    if frames.len() < 2 {
        // Resolved targets but no visible animation — every sampled
        // frame produced the same per-target transforms (e.g. keyframes
        // animate a property we don't yet support). Fall back to the
        // static SVG render so we don't burn cycles on a fixed image.
        return None;
    }
    let marked = marker::build_marked_svg(text, &targets);

    Some(AnimatedSvg {
        marked,
        duration,
        infinite,
        frames,
        width_px,
        height_px,
    })
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

    #[test]
    fn detects_keyframes_and_targets() {
        let model = parse_text(SAMPLE).expect("parsed");
        assert_eq!(model.duration, Duration::from_secs(3));
        assert!(model.infinite);
        assert_eq!(model.frames.len(), 3);
    }

    #[test]
    fn stepped_holds_value_until_next_stop() {
        let model = parse_text(SAMPLE).expect("parsed");
        assert_eq!(model.frames[0].transforms[0], "");
        assert_eq!(model.frames[1].transforms[0], "translate(-100,0)");
        assert_eq!(model.frames[2].transforms[0], "translate(-200,0)");
    }

    #[test]
    fn render_frame_substitutes_marker() {
        let model = parse_text(SAMPLE).expect("parsed");
        let f1 = render_frame(&model, 1);
        assert!(f1.contains("transform=\"translate(-100,0)\""));
        assert!(!f1.contains("__PEEK_ANIM_"));
    }

    #[test]
    fn no_keyframes_returns_none() {
        let plain = r#"<svg><rect/></svg>"#;
        assert!(parse_text(plain).is_none());
    }

    #[test]
    fn patched_svg_parses_with_resvg() {
        let model = parse_text(SAMPLE).expect("parsed");
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
        let model = parse_text(svg).expect("parsed");
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
        let model = parse_text(svg).expect("parsed");
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
        let model = parse_text(svg).expect("parsed");
        assert!(model.frames.len() >= 2);
    }

    #[test]
    fn tag_class_selector_resolves_animation() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-10px)}}circle.foo{animation:m 1s steps(1,end) infinite}</style>
<circle class="foo" cx="10" cy="10" r="2"/>
<rect class="foo" width="10" height="10"/>
</svg>"#;
        let model = parse_text(svg).expect("parsed");
        // Only the circle matches `circle.foo`; the rect (same class
        // but different tag) is ignored.
        let f1 = render_frame(&model, 1);
        assert!(f1.contains("translate"));
    }

    #[test]
    fn unsupported_property_falls_back_to_static() {
        // Class-resolved animation that only mutates `r` (no transform)
        // — until attribute animation lands, every sampled frame
        // collapses to the same empty transform vector. parse_text
        // detects this and returns None so the viewer uses the static
        // image render instead.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{r:0}50%{r:5px}}.dot{animation:m 1s steps(1,end) infinite}</style>
<circle class="dot" cx="10" cy="10" r="0"/>
</svg>"#;
        assert!(parse_text(svg).is_none());
    }

    #[test]
    fn merged_rules_combine_with_inline_style() {
        // Class rule supplies `animation:` shorthand; inline style
        // overrides duration. Verifies cascade: inline trails class.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<style>@keyframes m{0%{transform:translateX(0)}50%{transform:translateX(-10px)}}.dot{animation:m 5s steps(1,end) infinite}</style>
<g class="dot" style="animation-duration:1s"></g>
</svg>"#;
        let model = parse_text(svg).expect("parsed");
        assert_eq!(model.duration, Duration::from_secs(1));
    }

    #[test]
    fn anim_shorthand_recognized() {
        let svg = r#"<svg width="10" height="10">
<style>@keyframes m{0%{transform:translate(0,0)}100%{transform:translate(10,5)}}</style>
<g style="animation:m 1s linear infinite"></g>
</svg>"#;
        let model = parse_text(svg).expect("parsed");
        assert_eq!(model.duration, Duration::from_secs(1));
        assert!(model.infinite);
        // Linear timing, infinite loop → last sample approaches (but does
        // not reach) the 100% value, since reaching 100% would wrap to 0.
        let last = model.frames.last().unwrap();
        assert!(last.transforms[0].starts_with("translate(9."));
    }
}
