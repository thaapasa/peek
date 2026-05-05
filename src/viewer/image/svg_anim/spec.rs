//! Inline-style `animation:` / `animation-*` parsing into [`AnimSpec`].

use std::time::Duration;

#[derive(Clone)]
pub(super) struct AnimSpec {
    pub name: String,
    pub duration: Duration,
    /// True when timing function is `steps(...)` — used by the sampler to
    /// emit one frame per stop instead of resampling.
    pub stepped: bool,
    pub infinite: bool,
}

pub(super) fn parse_anim_spec(style: &str) -> Option<AnimSpec> {
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
