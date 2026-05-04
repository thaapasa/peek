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
//!    `__PEEK_ANIM_CLOSE_<i>__` placeholders inserted just inside each
//!    animated element, then per frame substitute the open marker with
//!    `<g transform="...">` and the close marker with `</g>`.
//!
//! The `<g>` wrapper exists because resvg/usvg currently ignore the
//! `transform` attribute when applied directly to a nested `<svg>`
//! element (covered by `nested_svg_transform_actually_shifts_content`).
//! Wrapping the element's children in a transformed group dodges that
//! limitation without changing the document's coordinate semantics.
//!
//! Phase 1 scope: CSS keyframes only (no SMIL `<animate>`); inline-style
//! targets only (no class/id selector matching); `transform: translateX/Y`
//! and `translate(x, y)` properties only. Unknown timing functions are
//! treated as `linear`.

use std::time::Duration;

use anyhow::{Context, Result};

use crate::input::InputSource;

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
    let scan = scan_svg(text);
    let keyframes = parse_keyframes(&scan.style_text);
    if keyframes.is_empty() {
        return None;
    }

    let mut targets: Vec<ResolvedTarget> = Vec::new();
    for rt in scan.targets {
        if let Some(stops) = keyframes.get(&rt.spec.name) {
            targets.push(ResolvedTarget {
                open_at: rt.open_at,
                close_at: rt.close_at,
                spec: rt.spec,
                stops: stops.clone(),
            });
        }
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

    let (width_px, height_px) = root_svg_dimensions(text).unwrap_or((1, 1));
    let frames = build_frames(&targets, duration);
    let marked = build_marked_svg(text, &targets);

    Some(AnimatedSvg {
        marked,
        duration,
        infinite,
        frames,
        width_px,
        height_px,
    })
}

/// Substitute the per-target placeholders in `marked` with the transform
/// values for `frame`. Result is a complete SVG document ready to feed to
/// resvg.
pub fn render_frame(model: &AnimatedSvg, frame_idx: usize) -> String {
    let frame = &model.frames[frame_idx % model.frames.len()];
    let mut out = model.marked.clone();
    for (i, transform) in frame.transforms.iter().enumerate() {
        let open = format!("__PEEK_ANIM_OPEN_{i}__");
        let close = format!("__PEEK_ANIM_CLOSE_{i}__");
        let (open_repl, close_repl) = if transform.is_empty() {
            (String::new(), String::new())
        } else {
            (format!("<g transform=\"{transform}\">"), "</g>".to_string())
        };
        out = out.replace(&open, &open_repl);
        out = out.replace(&close, &close_repl);
    }
    out
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

fn contains_anim_marker(text: &str) -> bool {
    text.contains("@keyframes") || text.contains("animation:") || text.contains("animation-name")
}

// ---------------------------------------------------------------------------
// Keyframe parsing
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct KeyframeStop {
    percent: f64,
    transform: Option<TransformValue>,
}

#[derive(Clone, Copy)]
struct TransformValue {
    tx: f64,
    ty: f64,
}

/// Result of one quick-xml pass: byte spans of every animated element +
/// concatenated `<style>` text content for the CSS parser.
struct XmlScan {
    targets: Vec<RawTarget>,
    style_text: String,
}

/// Walk the SVG once with quick-xml, collecting:
/// - Animated elements: any `Start` event whose `style="..."` attribute
///   contains an `animation-*` reference. Records open/close byte spans
///   tracking nesting depth so the matching `</tag>` is found correctly.
/// - `<style>` block text content (used by the CSS keyframe parser).
fn scan_svg(text: &str) -> XmlScan {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);

    struct Pending {
        depth: i32,
        open_at: usize,
        spec: AnimSpec,
    }

    let mut depth: i32 = 0;
    let mut pending: Vec<Pending> = Vec::new();
    let mut targets: Vec<RawTarget> = Vec::new();
    let mut style_text = String::new();
    let mut in_style_depth: i32 = 0;

    loop {
        let pos_before = reader.buffer_position() as usize;
        let event = match reader.read_event() {
            Ok(e) => e,
            Err(_) => break,
        };
        let pos_after = reader.buffer_position() as usize;

        match event {
            Event::Start(e) => {
                depth += 1;
                let local = e.local_name();
                if local.as_ref().eq_ignore_ascii_case(b"style") {
                    in_style_depth = depth;
                }
                if let Some(spec) = anim_spec_from_attrs(&e) {
                    pending.push(Pending {
                        depth,
                        open_at: pos_after,
                        spec,
                    });
                }
            }
            Event::End(e) => {
                while let Some(top) = pending.last() {
                    if top.depth == depth {
                        let p = pending.pop().unwrap();
                        targets.push(RawTarget {
                            open_at: p.open_at,
                            close_at: pos_before,
                            spec: p.spec,
                        });
                    } else {
                        break;
                    }
                }
                let local = e.local_name();
                if local.as_ref().eq_ignore_ascii_case(b"style") && depth == in_style_depth {
                    in_style_depth = 0;
                }
                depth -= 1;
            }
            Event::Text(t) if in_style_depth > 0 => {
                if let Ok(s) = t.decode() {
                    style_text.push_str(&s);
                    style_text.push('\n');
                }
            }
            Event::CData(t) if in_style_depth > 0 => {
                if let Ok(s) = std::str::from_utf8(t.as_ref()) {
                    style_text.push_str(s);
                    style_text.push('\n');
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    XmlScan {
        targets,
        style_text,
    }
}

fn anim_spec_from_attrs(e: &quick_xml::events::BytesStart<'_>) -> Option<AnimSpec> {
    for attr in e.attributes().with_checks(false) {
        let attr = attr.ok()?;
        if attr
            .key
            .local_name()
            .as_ref()
            .eq_ignore_ascii_case(b"style")
        {
            let val = std::str::from_utf8(&attr.value).ok()?;
            if let Some(spec) = parse_anim_spec(val) {
                return Some(spec);
            }
        }
    }
    None
}

fn parse_keyframes(css: &str) -> std::collections::HashMap<String, Vec<KeyframeStop>> {
    let mut out = std::collections::HashMap::new();
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(at) = find_substr(css, i, "@keyframes") else {
            break;
        };
        let name_start = skip_ws(css, at + "@keyframes".len());
        let name_end = name_start
            + css[name_start..]
                .find(|c: char| c.is_whitespace() || c == '{')
                .unwrap_or(0);
        let name = css[name_start..name_end].trim().to_string();
        let Some(brace_rel) = css[name_end..].find('{') else {
            break;
        };
        let body_start = name_end + brace_rel + 1;
        let Some(body_end) = find_matching_brace(css, body_start) else {
            break;
        };
        let body = &css[body_start..body_end];
        let stops = parse_keyframe_stops(body);
        if !name.is_empty() && !stops.is_empty() {
            out.insert(name, stops);
        }
        i = body_end + 1;
    }
    out
}

fn parse_keyframe_stops(body: &str) -> Vec<KeyframeStop> {
    let mut out = Vec::new();
    let mut cursor = 0;
    let bytes = body.as_bytes();
    while cursor < bytes.len() {
        cursor = skip_ws(body, cursor);
        if cursor >= bytes.len() {
            break;
        }
        // Read percentage list (comma-separated): `0%`, `50%`, `from`, `to`.
        let mut percents: Vec<f64> = Vec::new();
        loop {
            cursor = skip_ws(body, cursor);
            let token_start = cursor;
            while cursor < bytes.len() {
                let c = bytes[cursor] as char;
                if c == '%' || c == '{' || c == ',' || c.is_whitespace() {
                    break;
                }
                cursor += 1;
            }
            let token = body[token_start..cursor].trim();
            let pct = parse_percent_token(token);
            if cursor < bytes.len() && bytes[cursor] == b'%' {
                cursor += 1;
            }
            if let Some(p) = pct {
                percents.push(p);
            }
            cursor = skip_ws(body, cursor);
            if cursor < bytes.len() && bytes[cursor] == b',' {
                cursor += 1;
                continue;
            }
            break;
        }
        cursor = skip_ws(body, cursor);
        if cursor >= bytes.len() || bytes[cursor] != b'{' {
            // Skip ahead to next `}` to recover; degenerate input.
            if let Some(rel) = body[cursor..].find('}') {
                cursor += rel + 1;
                continue;
            }
            break;
        }
        cursor += 1;
        let Some(close_rel) = body[cursor..].find('}') else {
            break;
        };
        let close = cursor + close_rel;
        let decls = &body[cursor..close];
        let transform = parse_transform_decl(decls);
        for p in percents {
            out.push(KeyframeStop {
                percent: p,
                transform,
            });
        }
        cursor = close + 1;
    }
    out.sort_by(|a, b| a.percent.partial_cmp(&b.percent).unwrap());
    out
}

fn parse_percent_token(tok: &str) -> Option<f64> {
    if tok.is_empty() {
        return None;
    }
    if tok.eq_ignore_ascii_case("from") {
        return Some(0.0);
    }
    if tok.eq_ignore_ascii_case("to") {
        return Some(100.0);
    }
    tok.parse::<f64>().ok()
}

fn parse_transform_decl(decls: &str) -> Option<TransformValue> {
    // Walk `prop:value;` pairs; only `transform:` is consumed.
    for chunk in decls.split(';') {
        let Some(colon) = chunk.find(':') else {
            continue;
        };
        let prop = chunk[..colon].trim();
        if !prop.eq_ignore_ascii_case("transform") {
            continue;
        }
        let value = chunk[colon + 1..].trim();
        return parse_transform_value(value);
    }
    None
}

fn parse_transform_value(value: &str) -> Option<TransformValue> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(rest) = strip_fn(v, "translateX") {
        return Some(TransformValue {
            tx: parse_length(rest)?,
            ty: 0.0,
        });
    }
    if let Some(rest) = strip_fn(v, "translateY") {
        return Some(TransformValue {
            tx: 0.0,
            ty: parse_length(rest)?,
        });
    }
    if let Some(rest) = strip_fn(v, "translate") {
        let mut parts = rest.split(',').map(str::trim);
        let tx = parse_length(parts.next().unwrap_or(""))?;
        let ty = parts
            .next()
            .map(parse_length)
            .and_then(|o| o)
            .unwrap_or(0.0);
        return Some(TransformValue { tx, ty });
    }
    None
}

fn strip_fn<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let v = value.trim();
    if !v.starts_with(name) {
        return None;
    }
    let rest = v[name.len()..].trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let rest = &rest[1..];
    let close = rest.rfind(')')?;
    Some(rest[..close].trim())
}

fn parse_length(s: &str) -> Option<f64> {
    let t = s.trim();
    let num_end = t
        .find(|c: char| !(c == '-' || c == '+' || c == '.' || c.is_ascii_digit() || c == 'e'))
        .unwrap_or(t.len());
    let num = &t[..num_end];
    num.parse::<f64>().ok()
}

// ---------------------------------------------------------------------------
// Animation reference parsing (inline `style=` only, phase 1)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AnimSpec {
    name: String,
    duration: Duration,
    /// True when timing function is `steps(...)` — used by the sampler to
    /// emit one frame per stop instead of resampling.
    stepped: bool,
    infinite: bool,
}

struct RawTarget {
    /// Byte position right after the `>` closing the element's opening
    /// tag — the open marker (later replaced with `<g transform="...">`)
    /// is inserted here.
    open_at: usize,
    /// Byte position right before the matching `</svg>` closing tag —
    /// the close marker (replaced with `</g>`) is inserted here.
    close_at: usize,
    spec: AnimSpec,
}

struct ResolvedTarget {
    open_at: usize,
    close_at: usize,
    spec: AnimSpec,
    stops: Vec<KeyframeStop>,
}

fn parse_anim_spec(style: &str) -> Option<AnimSpec> {
    let mut name: Option<String> = None;
    let mut duration: Option<Duration> = None;
    let mut iter_count: Option<String> = None;
    let mut timing: Option<String> = None;

    for decl in style.split(';') {
        let Some(colon) = decl.find(':') else {
            continue;
        };
        let prop = decl[..colon].trim().to_ascii_lowercase();
        let value = decl[colon + 1..].trim();
        match prop.as_str() {
            "animation-name" => name = Some(value.to_string()),
            "animation-duration" => duration = parse_time(value),
            "animation-iteration-count" => iter_count = Some(value.to_string()),
            "animation-timing-function" => timing = Some(value.to_string()),
            "animation" => {
                if let Some(parsed) = parse_animation_shorthand(value) {
                    name = name.or(parsed.name);
                    duration = duration.or(parsed.duration);
                    iter_count = iter_count.or(parsed.iter);
                    timing = timing.or(parsed.timing);
                }
            }
            _ => {}
        }
    }

    let name = name?;
    let duration = duration?;
    let infinite = iter_count
        .map(|s| s.eq_ignore_ascii_case("infinite"))
        .unwrap_or(false);
    let stepped = timing
        .map(|s| s.trim_start().starts_with("steps"))
        .unwrap_or(false);
    Some(AnimSpec {
        name,
        duration,
        stepped,
        infinite,
    })
}

struct AnimationShorthand {
    name: Option<String>,
    duration: Option<Duration>,
    iter: Option<String>,
    timing: Option<String>,
}

fn parse_animation_shorthand(value: &str) -> Option<AnimationShorthand> {
    let mut name = None;
    let mut duration = None;
    let mut iter = None;
    let mut timing = None;

    // `steps(...)` and `cubic-bezier(...)` may contain spaces inside parens.
    // Tokenize respecting paren depth.
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in value.chars() {
        match c {
            '(' => {
                depth += 1;
                buf.push(c);
            }
            ')' => {
                depth -= 1;
                buf.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        tokens.push(buf);
    }

    for tok in tokens {
        if duration.is_none()
            && let Some(d) = parse_time(&tok)
        {
            duration = Some(d);
            continue;
        }
        if iter.is_none() && (tok.eq_ignore_ascii_case("infinite") || tok.parse::<f64>().is_ok()) {
            iter = Some(tok);
            continue;
        }
        if timing.is_none()
            && (tok.starts_with("steps")
                || tok.starts_with("cubic-bezier")
                || tok == "linear"
                || tok == "ease"
                || tok == "ease-in"
                || tok == "ease-out"
                || tok == "ease-in-out"
                || tok == "step-start"
                || tok == "step-end")
        {
            timing = Some(tok);
            continue;
        }
        if name.is_none() {
            name = Some(tok);
        }
    }
    Some(AnimationShorthand {
        name,
        duration,
        iter,
        timing,
    })
}

fn parse_time(value: &str) -> Option<Duration> {
    let v = value.trim();
    if let Some(num) = v.strip_suffix("ms") {
        let n: f64 = num.trim().parse().ok()?;
        return Some(Duration::from_secs_f64(n / 1000.0));
    }
    if let Some(num) = v.strip_suffix('s') {
        let n: f64 = num.trim().parse().ok()?;
        return Some(Duration::from_secs_f64(n));
    }
    None
}

// ---------------------------------------------------------------------------
// Frame timeline
// ---------------------------------------------------------------------------

fn build_frames(targets: &[ResolvedTarget], total: Duration) -> Vec<Frame> {
    let dur_s = total.as_secs_f64();
    if dur_s <= 0.0 {
        return Vec::new();
    }

    // Collect candidate sample times in [0, dur_s). Stepped targets only
    // need stop times; linear targets fill the gaps at FPS resolution.
    const FPS: f64 = 30.0;
    let mut times: Vec<f64> = vec![0.0];
    for tg in targets {
        let target_dur = tg.spec.duration.as_secs_f64();
        for stop in &tg.stops {
            let t = (stop.percent / 100.0) * target_dur;
            if t < dur_s {
                times.push(t);
            }
        }
        if !tg.spec.stepped {
            let stops_in_range: Vec<f64> = tg
                .stops
                .iter()
                .map(|s| (s.percent / 100.0) * target_dur)
                .filter(|t| *t <= dur_s)
                .collect();
            for w in stops_in_range.windows(2) {
                let gap = (w[1] - w[0]).max(0.0);
                let n = (gap * FPS).floor() as usize;
                for k in 1..n {
                    let t = w[0] + (k as f64 / FPS);
                    if t < dur_s {
                        times.push(t);
                    }
                }
            }
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

    // For each candidate time, compute per-target transforms; coalesce
    // consecutive samples whose transform vectors are identical.
    let mut samples: Vec<(f64, Vec<String>)> = Vec::new();
    for &t in &times {
        let xforms: Vec<String> = targets.iter().map(|tg| sample_target(tg, t)).collect();
        match samples.last() {
            Some((_, prev)) if prev == &xforms => {}
            _ => samples.push((t, xforms)),
        }
    }

    // Convert to Frame[] with per-frame delays. Last frame's delay extends
    // to dur_s so a full loop sums to `total`.
    let mut out = Vec::with_capacity(samples.len());
    for i in 0..samples.len() {
        let now = samples[i].0;
        let next = if i + 1 < samples.len() {
            samples[i + 1].0
        } else {
            dur_s
        };
        let delay_s = (next - now).max(0.020);
        out.push(Frame {
            delay: Duration::from_secs_f64(delay_s),
            transforms: std::mem::take(&mut samples[i].1),
        });
    }
    out
}

fn sample_target(target: &ResolvedTarget, t_global_s: f64) -> String {
    let target_dur = target.spec.duration.as_secs_f64();
    if target_dur <= 0.0 || target.stops.is_empty() {
        return String::new();
    }
    let local = if target.spec.infinite {
        t_global_s.rem_euclid(target_dur)
    } else {
        t_global_s.min(target_dur)
    };
    let pct = (local / target_dur) * 100.0;

    if target.spec.stepped {
        // Hold the most recent stop whose percent ≤ pct (CSS `steps(N, end)`
        // semantics: at the boundary, the new value takes effect).
        let mut current = &target.stops[0];
        for stop in &target.stops {
            if stop.percent <= pct + 1e-9 {
                current = stop;
            } else {
                break;
            }
        }
        return transform_to_attr(current.transform);
    }

    // Linear interpolation: find segment [prev, next] with prev.percent ≤
    // pct < next.percent.
    let mut prev = &target.stops[0];
    let mut next = &target.stops[target.stops.len() - 1];
    let mut found = false;
    for w in target.stops.windows(2) {
        if w[0].percent <= pct && pct < w[1].percent {
            prev = &w[0];
            next = &w[1];
            found = true;
            break;
        }
    }
    if !found {
        if pct < target.stops[0].percent {
            return transform_to_attr(target.stops[0].transform);
        }
        return transform_to_attr(target.stops.last().unwrap().transform);
    }
    let span = (next.percent - prev.percent).max(1e-6);
    let alpha = ((pct - prev.percent) / span).clamp(0.0, 1.0);
    let p = prev
        .transform
        .unwrap_or(TransformValue { tx: 0.0, ty: 0.0 });
    let n = next
        .transform
        .unwrap_or(TransformValue { tx: 0.0, ty: 0.0 });
    let tx = p.tx + (n.tx - p.tx) * alpha;
    let ty = p.ty + (n.ty - p.ty) * alpha;
    transform_to_attr(Some(TransformValue { tx, ty }))
}

fn transform_to_attr(v: Option<TransformValue>) -> String {
    match v {
        None => String::new(),
        Some(t) if t.tx == 0.0 && t.ty == 0.0 => String::new(),
        Some(t) => format!("translate({},{})", fmt_num(t.tx), fmt_num(t.ty)),
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{:.0}", n)
    } else {
        format!("{:.4}", n)
    }
}

// ---------------------------------------------------------------------------
// Marked-SVG construction
// ---------------------------------------------------------------------------

fn build_marked_svg(text: &str, targets: &[ResolvedTarget]) -> String {
    // Inject `__PEEK_ANIM_OPEN_<i>__` and `__PEEK_ANIM_CLOSE_<i>__`
    // markers around each animated element's children. Sort all
    // injection points descending so byte offsets remain valid as we
    // mutate.
    let mut out = text.to_string();
    let mut points: Vec<(usize, String)> = Vec::with_capacity(targets.len() * 2);
    for (i, t) in targets.iter().enumerate() {
        points.push((t.open_at, format!("__PEEK_ANIM_OPEN_{i}__")));
        points.push((t.close_at, format!("__PEEK_ANIM_CLOSE_{i}__")));
    }
    points.sort_by_key(|(pos, _)| std::cmp::Reverse(*pos));
    for (pos, marker) in points {
        out.insert_str(pos, &marker);
    }
    out
}

// ---------------------------------------------------------------------------
// Root <svg> dimensions (viewport pixel size)
// ---------------------------------------------------------------------------

fn root_svg_dimensions(text: &str) -> Option<(u32, u32)> {
    let open = text.find("<svg")?;
    let after = &text[open..];
    let close = after.find('>')?;
    let header = &after[..close];
    let w = attr_value(header, "width").and_then(|s| parse_length(&s).map(|f| f as u32));
    let h = attr_value(header, "height").and_then(|s| parse_length(&s).map(|f| f as u32));
    Some((w.unwrap_or(0).max(1), h.unwrap_or(0).max(1)))
}

fn attr_value(header: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=");
    let pos = header.find(&needle)?;
    let after = &header[pos + needle.len()..];
    let q = after.chars().next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let body = &after[1..];
    let close = body.find(q)?;
    Some(body[..close].to_string())
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

fn skip_ws(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    i
}

fn find_substr(haystack: &str, from: usize, needle: &str) -> Option<usize> {
    haystack[from..].find(needle).map(|r| from + r)
}

fn find_matching_brace(s: &str, body_start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1i32;
    let mut i = body_start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
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
            "{}/Downloads/demo.svg",
            std::env::var("HOME").unwrap_or_default()
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

    /// Sanity-check the demo file (`~/Downloads/demo.svg`) when present:
    /// frame 0 should differ visually from a mid-animation frame. Skipped
    /// if the file isn't on disk so CI / fresh checkouts pass without it.
    #[test]
    fn demo_svg_frames_diverge_when_available() {
        let path = match std::env::var("HOME") {
            Ok(h) => format!("{h}/Downloads/demo.svg"),
            Err(_) => return,
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
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
