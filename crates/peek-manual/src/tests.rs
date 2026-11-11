use peek_foundation::viewer::listing::ListSource;
use peek_foundation::viewer::modes::ExtractTarget;
use peek_theme::{PeekThemeName, StyleMode, make_peek_theme};

use super::preprocess::{SummaryItem, parse_summary, preprocess};
use super::*;

mod preprocessing {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        let src = "# Title\n\nSome *text* and [mise](https://mise.jdx.dev/).\n";
        assert_eq!(preprocess(src).unwrap(), src);
    }

    #[test]
    fn images_are_dropped() {
        let src = "Before.\n\n![A screenshot](../img/shot.svg)\n\nAfter.\n";
        assert_eq!(preprocess(src).unwrap(), "Before.\n\n\n\nAfter.\n");
    }

    #[test]
    fn relative_links_lose_their_target() {
        let src = "See [Themes](./viewer/themes.md) and [shells](./installation.md#shells).\n";
        assert_eq!(preprocess(src).unwrap(), "See [Themes]() and [shells]().\n");
    }

    #[test]
    fn external_links_keep_their_target() {
        let src = "Get [mise](https://mise.jdx.dev/) or [mail](mailto:a@b.c).\n";
        assert_eq!(preprocess(src).unwrap(), src);
    }

    #[test]
    fn peek_comment_is_unwrapped() {
        let src = "Intro.\n\n<!-- peek:\nPress `i` for **Info**.\n-->\n\nOutro.\n";
        assert_eq!(
            preprocess(src).unwrap(),
            "Intro.\n\nPress `i` for **Info**.\n\nOutro.\n"
        );
    }

    #[test]
    fn peek_comment_followed_directly_by_text_stays_a_separate_block() {
        let src = "<!-- peek: In app. -->\nWeb and app.\n";
        assert_eq!(preprocess(src).unwrap(), "In app.\n\nWeb and app.\n");
    }

    #[test]
    fn peek_comment_body_is_preprocessed() {
        let src = "<!-- peek:\nSee [Themes](./viewer/themes.md).\n-->\n";
        assert_eq!(preprocess(src).unwrap(), "See [Themes]().\n");
    }

    #[test]
    fn peek_skip_region_is_removed_with_everything_inside() {
        let src = "Keep.\n\n<!-- peek-skip -->\n\nWeb only ![img](a.svg).\n\n<!-- /peek-skip -->\n\nKeep too.\n";
        assert_eq!(preprocess(src).unwrap(), "Keep.\n\n\nKeep too.\n");
    }

    #[test]
    fn ordinary_comments_are_left_alone() {
        let src = "<!-- an authoring note -->\n\nText.\n";
        assert_eq!(preprocess(src).unwrap(), src);
    }

    #[test]
    fn bad_markers_are_errors() {
        for (src, why) in [
            ("<!-- peek-skip -->\n\nText.\n", "unclosed"),
            ("<!-- /peek-skip -->\n", "without an opening"),
            (
                "<!-- peek-skip -->\n\n<!-- peek-skip -->\n\n<!-- /peek-skip -->\n",
                "nested",
            ),
            ("<!-- peek: -->\n", "empty"),
            ("<!-- peek-only -->\n", "unknown"),
        ] {
            let err = preprocess(src).expect_err(src);
            assert!(err.contains(why), "{src:?}: {err}");
        }
    }
}

mod summary {
    use super::*;

    fn chapter(title: &str, path: &str, depth: usize) -> SummaryItem {
        SummaryItem::Chapter {
            title: title.into(),
            path: path.into(),
            depth,
        }
    }

    #[test]
    fn parses_title_prefix_parts_and_nesting() {
        let src = "# Summary\n\n[Intro](./intro.md)\n\n# Part A\n\n- [One](./one.md)\n  - [Sub `x`](./a/sub.md)\n- [Draft]()\n\n---\n\n# Part B\n\n- [Two](two.md)\n";
        assert_eq!(
            parse_summary(src).unwrap(),
            vec![
                chapter("Intro", "intro.md", 0),
                SummaryItem::Part("Part A".into()),
                chapter("One", "one.md", 0),
                chapter("Sub x", "a/sub.md", 1),
                SummaryItem::Part("Part B".into()),
                chapter("Two", "two.md", 0),
            ]
        );
    }
}

mod embedded {
    use super::*;

    #[test]
    fn every_chapter_has_a_body() {
        assert!(chapters().count() > 10);
        for c in chapters() {
            assert!(!c.body.trim().is_empty(), "{} is empty", c.path);
            assert!(c.path.ends_with(".md"), "{}", c.path);
        }
    }

    #[test]
    fn no_images_survive() {
        for c in chapters() {
            assert!(!c.body.contains("!["), "{} still has an image", c.path);
        }
    }

    #[test]
    fn extract_finds_chapter_by_path() {
        let e = extract("./viewer/themes.md").unwrap();
        assert_eq!(e.suggested_name, "themes.md");
        assert_eq!(e.source.name(), "viewer/themes.md");
    }

    #[test]
    fn extract_unknown_key_is_not_found() {
        assert!(matches!(extract("nope.md"), Err(ExtractError::NotFound(_))));
    }
}

mod toc {
    use super::*;

    fn row_index(src: &ManualTocSource, title: &str) -> usize {
        (0..src.len()).find(|&i| src.name(i) == title).unwrap()
    }

    #[test]
    fn follows_summary_order_with_parts_as_parents() {
        let src = ManualTocSource::new();
        assert_eq!(src.name(0), "Introduction");
        assert_eq!(src.initial_selection(), Some(0));

        let part = row_index(&src, "File Types");
        let overview = row_index(&src, "Overview");
        let csv = row_index(&src, "CSV / TSV");
        assert_eq!(src.parent(overview), Some(part));
        assert_eq!(src.parent(csv), Some(overview));
        assert!(src.extract_target(part).is_none());
    }

    #[test]
    fn parent_chapters_extract_too() {
        let src = ManualTocSource::new();
        let overview = row_index(&src, "Overview");
        match src.extract_target(overview) {
            Some(ExtractTarget::EntryPath(p)) => assert_eq!(p, "file-types/index.md"),
            other => panic!("expected EntryPath, got {other:?}"),
        }
    }

    #[test]
    fn flat_line_is_path_tab_title_and_skips_parts() {
        let src = ManualTocSource::new();
        let theme = make_peek_theme(PeekThemeName::IdeaDark, StyleMode::Plain);
        let part = row_index(&src, "Reference");
        let cli = row_index(&src, "CLI options");
        assert_eq!(src.flat_line(part, &theme), None);
        assert_eq!(
            src.flat_line(cli, &theme).as_deref(),
            Some("cli-reference.md\tCLI options")
        );
    }
}
