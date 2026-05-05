//! Build merged frame timeline from resolved per-target stops.

use std::time::Duration;

use super::Frame;
use super::ResolvedTarget;
use super::keyframes::TransformValue;

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
        // Sample the delay→animate boundary so the pre-delay frame is
        // distinguishable from the first stop when they share a value.
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
    let delay = target.spec.delay.as_secs_f64();
    if target_dur <= 0.0 || target.stops.is_empty() {
        return String::new();
    }
    if t_global_s < delay {
        // Pre-delay: hold the un-animated state. Matches CSS
        // `animation-fill-mode: none` (the default) — no transform
        // applied until the iteration begins.
        return String::new();
    }
    let t_local = t_global_s - delay;
    let local = if target.spec.infinite {
        t_local.rem_euclid(target_dur)
    } else {
        t_local.min(target_dur)
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
