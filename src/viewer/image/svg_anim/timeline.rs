//! Build merged frame timeline from resolved per-target stops.

use std::time::Duration;

use super::Frame;
use super::ResolvedTarget;
use super::keyframes::{PropChange, PropValue, TransformValue};

/// Per-target render state at a single sampled time. Carried inside
/// [`Frame::targets`]; coalescing in [`build_frames`] uses full-equality
/// so a frame is only emitted when *any* target's state changes.
#[derive(Clone, PartialEq, Eq, Default)]
pub(super) struct FrameTarget {
    /// `transform` attribute value to inject on the wrapper `<g>` (or
    /// empty string for "no transform").
    pub transform: String,
    /// Per-animated-property attribute slots. Index parallel to
    /// `ResolvedTarget::animated_props`. Each entry is the full slot
    /// text (e.g. ` r="5"`) or empty (no override — use whatever the
    /// element's own opening tag has, after slot-rewrite that means
    /// "no attribute at all").
    pub slots: Vec<String>,
}

pub(super) fn build_frames(targets: &[ResolvedTarget], total: Duration) -> Vec<Frame> {
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
        let delay = tg.spec.delay.as_secs_f64();
        if delay > 0.0 && delay < dur_s {
            times.push(delay);
        }
        for stop in &tg.stops {
            let t = delay + (stop.percent / 100.0) * target_dur;
            if t < dur_s {
                times.push(t);
            }
        }
        if !tg.spec.stepped {
            let stops_in_range: Vec<f64> = tg
                .stops
                .iter()
                .map(|s| delay + (s.percent / 100.0) * target_dur)
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

    // For each candidate time, compute per-target FrameTargets; coalesce
    // consecutive samples whose vectors are bit-identical.
    let mut samples: Vec<(f64, Vec<FrameTarget>)> = Vec::new();
    for &t in &times {
        let row: Vec<FrameTarget> = targets.iter().map(|tg| sample_target(tg, t)).collect();
        match samples.last() {
            Some((_, prev)) if prev == &row => {}
            _ => samples.push((t, row)),
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
            targets: std::mem::take(&mut samples[i].1),
        });
    }
    out
}

fn sample_target(target: &ResolvedTarget, t_global_s: f64) -> FrameTarget {
    let target_dur = target.spec.duration.as_secs_f64();
    let delay = target.spec.delay.as_secs_f64();
    if target_dur <= 0.0 || target.stops.is_empty() {
        return FrameTarget::default();
    }
    if t_global_s < delay {
        // Pre-delay: no transform, slots restore the element's
        // pre-animation state (original attribute values, or absent
        // entirely when the element didn't have the attribute).
        return FrameTarget {
            transform: String::new(),
            slots: target
                .animated_props
                .iter()
                .zip(target.original_values.iter())
                .map(|(name, orig)| match orig {
                    Some(v) => format!(" {name}=\"{v}\""),
                    None => String::new(),
                })
                .collect(),
        };
    }
    let t_local = t_global_s - delay;
    let local = if target.spec.infinite {
        t_local.rem_euclid(target_dur)
    } else {
        t_local.min(target_dur)
    };
    let pct = (local / target_dur) * 100.0;

    if target.spec.stepped {
        let mut current = &target.stops[0];
        for stop in &target.stops {
            if stop.percent <= pct + 1e-9 {
                current = stop;
            } else {
                break;
            }
        }
        return FrameTarget {
            transform: transform_to_attr(current.transform),
            slots: build_slots_step(target, &current.props),
        };
    }

    // Linear interpolation: find segment [prev, next] with prev.percent ≤
    // pct < next.percent.
    let mut prev_stop = &target.stops[0];
    let mut next_stop = &target.stops[target.stops.len() - 1];
    let mut found = false;
    for w in target.stops.windows(2) {
        if w[0].percent <= pct && pct < w[1].percent {
            prev_stop = &w[0];
            next_stop = &w[1];
            found = true;
            break;
        }
    }
    if !found {
        let s = if pct < target.stops[0].percent {
            &target.stops[0]
        } else {
            target.stops.last().unwrap()
        };
        return FrameTarget {
            transform: transform_to_attr(s.transform),
            slots: build_slots_step(target, &s.props),
        };
    }
    let span = (next_stop.percent - prev_stop.percent).max(1e-6);
    let alpha = ((pct - prev_stop.percent) / span).clamp(0.0, 1.0);
    let p = prev_stop
        .transform
        .unwrap_or(TransformValue { tx: 0.0, ty: 0.0 });
    let n = next_stop
        .transform
        .unwrap_or(TransformValue { tx: 0.0, ty: 0.0 });
    let tx = p.tx + (n.tx - p.tx) * alpha;
    let ty = p.ty + (n.ty - p.ty) * alpha;

    FrameTarget {
        transform: transform_to_attr(Some(TransformValue { tx, ty })),
        slots: build_slots_linear(target, &prev_stop.props, &next_stop.props, alpha),
    }
}

/// Build slot strings using one stop's props (stepped semantics). For
/// each animated prop name: if the stop defines a value, render it;
/// otherwise fall back to the element's original attribute value (or
/// empty when no original was present).
fn build_slots_step(target: &ResolvedTarget, props: &[PropChange]) -> Vec<String> {
    target
        .animated_props
        .iter()
        .zip(target.original_values.iter())
        .map(|(name, orig)| {
            let value = props
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
                .map(|p| p.value.render())
                .or_else(|| orig.clone());
            match value {
                Some(v) => format!(" {name}=\"{v}\""),
                None => String::new(),
            }
        })
        .collect()
}

/// Linear interpolation between two stops' props for matching numeric
/// values; otherwise mirrors [`build_slots_step`] on the previous stop.
fn build_slots_linear(
    target: &ResolvedTarget,
    prev: &[PropChange],
    next: &[PropChange],
    alpha: f64,
) -> Vec<String> {
    target
        .animated_props
        .iter()
        .zip(target.original_values.iter())
        .map(|(name, orig)| {
            let p_match = prev
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
                .map(|p| &p.value);
            let n_match = next
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name))
                .map(|p| &p.value);
            let value = match (p_match, n_match) {
                (
                    Some(PropValue::Numeric { value: a, unit: ua }),
                    Some(PropValue::Numeric { value: b, unit: ub }),
                ) if ua == ub => {
                    let v = a + (b - a) * alpha;
                    Some(format!("{}{}", fmt_num(v), ua))
                }
                (Some(p), _) => Some(p.render()),
                (None, _) => orig.clone(),
            };
            match value {
                Some(v) => format!(" {name}=\"{v}\""),
                None => String::new(),
            }
        })
        .collect()
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
