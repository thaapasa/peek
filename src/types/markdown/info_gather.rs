//! Build [`MarkdownStats`] by line-scanning the document.
//!
//! The scanner is intentionally lenient: it follows CommonMark/GFM closely
//! enough for stats but ignores edge cases that would require a real
//! block parser (e.g. lazy-continuation in lists). Fenced code blocks are
//! tracked so heading-like text inside ` ``` ` blocks isn't counted as a
//! heading. Reading time uses 230 wpm — middle of common estimates.

use crate::info::{FrontmatterKind, MarkdownStats};

const READING_WPM: u32 = 230;

pub fn gather(text: &str) -> MarkdownStats {
    let mut stats = MarkdownStats {
        heading_counts: [0; 6],
        code_block_count: 0,
        code_block_languages: Vec::new(),
        inline_code_count: 0,
        link_count: 0,
        image_count: 0,
        table_count: 0,
        list_item_count: 0,
        task_done: 0,
        task_total: 0,
        blockquote_lines: 0,
        footnote_def_count: 0,
        frontmatter: None,
        prose_words: 0,
        reading_minutes: 0,
    };

    let mut lines = text.lines().enumerate().peekable();

    // Frontmatter: YAML (`---`) or TOML (`+++`) on the first line, closed
    // by a matching fence. Skip its content for everything else.
    if let Some((_, first)) = lines.peek().copied() {
        let trimmed = first.trim_end();
        let kind = match trimmed {
            "---" => Some(FrontmatterKind::Yaml),
            "+++" => Some(FrontmatterKind::Toml),
            _ => None,
        };
        if let Some(kind) = kind {
            stats.frontmatter = Some(kind);
            let fence = trimmed;
            lines.next();
            for (_, line) in lines.by_ref() {
                if line.trim_end() == fence {
                    break;
                }
            }
        }
    }

    let mut in_fence: Option<char> = None;
    // Tracks "previous line could be a Setext header underline target".
    let mut prev_text_for_setext: Option<String> = None;
    // Tracks consecutive table rows for one-table accounting.
    let mut in_table = false;

    for (_, raw) in lines {
        let line = strip_trailing_cr(raw);

        // Fenced code blocks dominate everything else inside.
        if let Some(fence_ch) = in_fence {
            if is_closing_fence(line, fence_ch) {
                in_fence = None;
            }
            prev_text_for_setext = None;
            in_table = false;
            continue;
        }
        if let Some((fence_ch, lang)) = parse_opening_fence(line) {
            stats.code_block_count += 1;
            if !lang.is_empty() && !stats.code_block_languages.iter().any(|l| l == lang) {
                stats.code_block_languages.push(lang.to_string());
            }
            in_fence = Some(fence_ch);
            prev_text_for_setext = None;
            in_table = false;
            continue;
        }

        let trimmed = line.trim_start();

        // ATX headings — `#` to `######` followed by a space.
        if let Some(level) = atx_heading_level(trimmed) {
            stats.heading_counts[level - 1] += 1;
            count_inline_features(line, &mut stats);
            stats.prose_words += word_count(strip_atx_marker(trimmed));
            prev_text_for_setext = None;
            in_table = false;
            continue;
        }

        // Setext heading: previous non-blank line gets promoted to H1/H2
        // if this line is `===` or `---`.
        if let Some(prev) = &prev_text_for_setext
            && let Some(level) = setext_underline_level(trimmed)
        {
            stats.heading_counts[level - 1] += 1;
            // The previous line was already counted as prose; that's a
            // small over-count we tolerate to keep the scanner one-pass.
            let _ = prev;
            prev_text_for_setext = None;
            continue;
        }

        // Tables: `| … |` + alignment row `| --- | :---: |` etc.
        if is_table_separator(trimmed) {
            if !in_table {
                stats.table_count += 1;
                in_table = true;
            }
            prev_text_for_setext = None;
            continue;
        }
        if in_table && !looks_like_table_row(trimmed) {
            in_table = false;
        }

        // Footnote definitions: `[^id]: …`
        if is_footnote_definition(trimmed) {
            stats.footnote_def_count += 1;
        }

        // Blockquote
        if trimmed.starts_with('>') {
            stats.blockquote_lines += 1;
        }

        // Lists + task lists
        if let Some(after_marker) = strip_list_marker(trimmed) {
            stats.list_item_count += 1;
            match task_box(after_marker) {
                Some(true) => {
                    stats.task_total += 1;
                    stats.task_done += 1;
                }
                Some(false) => {
                    stats.task_total += 1;
                }
                None => {}
            }
            count_inline_features(line, &mut stats);
            stats.prose_words += word_count(strip_task_box(after_marker));
            prev_text_for_setext = Some(line.to_string());
            continue;
        }

        // Plain paragraph / blank line
        if trimmed.is_empty() {
            prev_text_for_setext = None;
            in_table = false;
            continue;
        }

        count_inline_features(line, &mut stats);
        stats.prose_words += word_count(line);
        prev_text_for_setext = Some(line.to_string());
    }

    stats.reading_minutes = if stats.prose_words == 0 {
        0
    } else {
        ((stats.prose_words as u32).div_ceil(READING_WPM)).max(1)
    };

    stats
}

fn strip_trailing_cr(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

fn atx_heading_level(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }
    if i == 0 || i > 6 {
        return None;
    }
    if i == bytes.len() {
        return Some(i);
    }
    if bytes[i] == b' ' || bytes[i] == b'\t' {
        Some(i)
    } else {
        None
    }
}

fn strip_atx_marker(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    let mut end = bytes.len();
    while end > i && (bytes[end - 1] == b'#' || bytes[end - 1] == b' ' || bytes[end - 1] == b'\t') {
        end -= 1;
    }
    &s[i..end]
}

fn setext_underline_level(s: &str) -> Option<usize> {
    if s.is_empty() {
        return None;
    }
    let first = s.as_bytes()[0];
    if first != b'=' && first != b'-' {
        return None;
    }
    if !s.bytes().all(|b| b == first) {
        return None;
    }
    if s.len() < 2 {
        return None;
    }
    Some(if first == b'=' { 1 } else { 2 })
}

fn parse_opening_fence(line: &str) -> Option<(char, &str)> {
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    if leading > 3 {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let ch = match bytes.first()? {
        b'`' => '`',
        b'~' => '~',
        _ => return None,
    };
    let mut count = 0;
    while count < bytes.len() && bytes[count] == ch as u8 {
        count += 1;
    }
    if count < 3 {
        return None;
    }
    let info = trimmed[count..].trim();
    // Backtick fences disallow another backtick in the info string.
    if ch == '`' && info.contains('`') {
        return None;
    }
    let lang = info.split_whitespace().next().unwrap_or("");
    Some((ch, lang))
}

fn is_closing_fence(line: &str, ch: char) -> bool {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let bytes = trimmed.as_bytes();
    let mut count = 0;
    while count < bytes.len() && bytes[count] == ch as u8 {
        count += 1;
    }
    count >= 3 && trimmed[count..].trim().is_empty()
}

fn is_table_separator(s: &str) -> bool {
    let trimmed = s.trim();
    if !trimmed.contains('|') || !trimmed.contains('-') {
        return false;
    }
    // Pipe-separated cells, each cell is `:?-+:?` (with optional spaces).
    let body = trimmed.trim_matches('|');
    body.split('|').all(|cell| {
        let c = cell.trim();
        !c.is_empty()
            && c.chars()
                .all(|ch| ch == '-' || ch == ':' || ch == ' ' || ch == '\t')
            && c.contains('-')
    })
}

fn looks_like_table_row(s: &str) -> bool {
    s.trim().contains('|')
}

fn strip_list_marker(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    // Bullet: `- `, `* `, `+ `
    if matches!(bytes[0], b'-' | b'*' | b'+')
        && bytes.len() >= 2
        && (bytes[1] == b' ' || bytes[1] == b'\t')
    {
        return Some(s[2..].trim_start());
    }
    // Ordered: `1. `, `12) `
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i <= 9 && i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b')') {
        let after = i + 1;
        if after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\t') {
            return Some(s[after + 1..].trim_start());
        }
    }
    None
}

fn task_box(after_marker: &str) -> Option<bool> {
    let bytes = after_marker.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'[' || bytes[2] != b']' {
        return None;
    }
    if bytes.len() > 3 && bytes[3] != b' ' && bytes[3] != b'\t' {
        return None;
    }
    match bytes[1] {
        b' ' => Some(false),
        b'x' | b'X' => Some(true),
        _ => None,
    }
}

fn strip_task_box(after_marker: &str) -> &str {
    if task_box(after_marker).is_some() {
        after_marker[3..].trim_start()
    } else {
        after_marker
    }
}

fn is_footnote_definition(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 5 || &bytes[0..2] != b"[^" {
        return false;
    }
    let Some(close_idx) = s.find("]:") else {
        return false;
    };
    close_idx > 2 && bytes[2..close_idx].iter().all(|b| !b.is_ascii_whitespace())
}

/// Walk a single line, counting inline features. Inline code spans are
/// recognised by paired backticks; their contents are skipped so URLs and
/// `[text](url)` patterns inside code don't get counted as links.
fn count_inline_features(line: &str, stats: &mut MarkdownStats) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'`' => {
                // Count run of backticks; matching close run of same length.
                let start = i;
                while i < bytes.len() && bytes[i] == b'`' {
                    i += 1;
                }
                let run_len = i - start;
                let close = find_backtick_run(bytes, i, run_len);
                if let Some(end_start) = close {
                    stats.inline_code_count += 1;
                    i = end_start + run_len;
                } else {
                    // Unmatched — leave alone, continue scanning.
                }
            }
            b'!' if i + 1 < bytes.len() && bytes[i + 1] == b'[' => {
                if let Some(consumed) = match_link(&bytes[i + 1..]) {
                    stats.image_count += 1;
                    i += 1 + consumed;
                } else {
                    i += 1;
                }
            }
            b'[' => {
                if let Some(consumed) = match_link(&bytes[i..]) {
                    stats.link_count += 1;
                    i += consumed;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
}

fn find_backtick_run(bytes: &[u8], start: usize, run_len: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let s = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            if i - s == run_len {
                return Some(s);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Match `[text](url)` or `[text][ref]`. Returns total bytes consumed.
fn match_link(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'[') {
        return None;
    }
    let mut depth = 1;
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'[' => {
                depth += 1;
                i += 1;
            }
            b']' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            _ => i += 1,
        }
    }
    if depth != 0 {
        return None;
    }
    if i >= bytes.len() {
        return None;
    }
    match bytes[i] {
        b'(' => {
            let mut paren = 1;
            let mut j = i + 1;
            while j < bytes.len() && paren > 0 {
                match bytes[j] {
                    b'\\' => j += 2,
                    b'(' => {
                        paren += 1;
                        j += 1;
                    }
                    b')' => {
                        paren -= 1;
                        j += 1;
                    }
                    _ => j += 1,
                }
            }
            if paren == 0 { Some(j) } else { None }
        }
        b'[' => {
            // Reference link `[text][ref]` — count it.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b']' {
                j += 1;
            }
            if j < bytes.len() { Some(j + 1) } else { None }
        }
        _ => None,
    }
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_headings_by_level() {
        let s = "# A\n## B\n### C\n## D\n";
        let stats = gather(s);
        assert_eq!(stats.heading_counts, [1, 2, 1, 0, 0, 0]);
    }

    #[test]
    fn fenced_code_block_recorded_with_language() {
        let s = "before\n```rust\nfn main() {}\n```\nafter\n";
        let stats = gather(s);
        assert_eq!(stats.code_block_count, 1);
        assert_eq!(stats.code_block_languages, vec!["rust"]);
    }

    #[test]
    fn headings_inside_fenced_code_are_ignored() {
        let s = "```\n# not a heading\n```\n# real\n";
        let stats = gather(s);
        assert_eq!(stats.heading_counts[0], 1);
        assert_eq!(stats.code_block_count, 1);
    }

    #[test]
    fn task_list_progress() {
        let s = "- [x] done\n- [ ] todo\n- [X] also done\n- not a task\n";
        let stats = gather(s);
        assert_eq!(stats.task_total, 3);
        assert_eq!(stats.task_done, 2);
        assert_eq!(stats.list_item_count, 4);
    }

    #[test]
    fn table_counted_once_per_run() {
        let s = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n| c | d |\n|---|---|\n";
        let stats = gather(s);
        assert_eq!(stats.table_count, 2);
    }

    #[test]
    fn links_and_images_distinguished() {
        let s = "See [docs](https://x) and ![logo](logo.png).\n";
        let stats = gather(s);
        assert_eq!(stats.link_count, 1);
        assert_eq!(stats.image_count, 1);
    }

    #[test]
    fn inline_code_counted_and_link_inside_skipped() {
        let s = "Use `[not](a-link)` here, but [yes](url) is.\n";
        let stats = gather(s);
        assert_eq!(stats.inline_code_count, 1);
        assert_eq!(stats.link_count, 1);
    }

    #[test]
    fn frontmatter_yaml_detected_and_skipped() {
        let s = "---\ntitle: x\n---\n# heading\n";
        let stats = gather(s);
        assert_eq!(stats.frontmatter, Some(FrontmatterKind::Yaml));
        assert_eq!(stats.heading_counts[0], 1);
    }

    #[test]
    fn reading_minutes_rounds_up() {
        // 250 words → 2 minutes at 230 wpm
        let words = "word ".repeat(250);
        let stats = gather(&words);
        assert_eq!(stats.reading_minutes, 2);
    }

    #[test]
    fn footnote_definition_counted() {
        let s = "Body[^a].\n\n[^a]: note\n";
        let stats = gather(s);
        assert_eq!(stats.footnote_def_count, 1);
    }
}
