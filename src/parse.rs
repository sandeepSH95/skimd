//! Pure source ↔ block mapping for block-swap editing (wired into the view
//! in M2; tested standalone until then).
#![allow(dead_code)]

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

/// Must match the options `iced::widget::markdown::Content::parse` enables,
/// so a block renders the same in isolation as in the full document.
pub const OPTIONS: Options = Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
    .union(Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS)
    .union(Options::ENABLE_TABLES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);

/// Splits source into top-level block byte-ranges.
///
/// Invariant: the ranges tile `0..source.len()` exactly — every gap byte
/// (blank lines, link reference definitions, which emit no events) belongs
/// to the block before it, and the first block absorbs any leading gap.
/// This makes `splice` lossless by construction.
pub fn segment(source: &str) -> Vec<Range<usize>> {
    let mut blocks: Vec<Range<usize>> = Vec::new();
    let mut depth = 0usize;

    for (event, range) in Parser::new_ext(source, OPTIONS).into_offset_iter() {
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    blocks.push(range);
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            // Rules are the one top-level event with no Start/End pair.
            Event::Rule if depth == 0 => blocks.push(range),
            _ => {}
        }
    }

    if let Some(first) = blocks.first_mut() {
        first.start = 0;
    }
    for i in 0..blocks.len() {
        let end = if i + 1 < blocks.len() {
            blocks[i + 1].start
        } else {
            source.len()
        };
        blocks[i].end = end;
    }

    blocks
}

/// Removes the language from every fenced code block (```rust → ```).
///
/// Syntax highlighting costs ~40ms of syntect grammar compilation inside
/// `markdown::Content::parse`; the first frame parses this stripped copy
/// instead, and the highlighted parse swaps in from a background task.
/// Offsets come from pulldown-cmark itself, so this can't mangle content —
/// a fence we fail to strip merely stays highlighted (slow), never wrong.
pub fn strip_fence_languages(source: &str) -> String {
    let mut cuts: Vec<Range<usize>> = Vec::new();

    for (event, range) in Parser::new_ext(source, OPTIONS).into_offset_iter() {
        let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = event else {
            continue;
        };
        if info.is_empty() {
            continue;
        }
        let line_end = source[range.clone()]
            .find('\n')
            .map_or(range.end, |i| range.start + i);
        let line = &source[range.start..line_end];
        let Some(fence_char) = line.trim_start().chars().next() else {
            continue;
        };
        if fence_char != '`' && fence_char != '~' {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let marker_len = line[indent..].chars().take_while(|&c| c == fence_char).count();
        cuts.push(range.start + indent + marker_len..line_end);
    }

    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for cut in cuts {
        out.push_str(&source[cursor..cut.start]);
        cursor = cut.end;
    }
    out.push_str(&source[cursor..]);
    out
}

/// Replaces `range` in `source` with `edited`.
pub fn splice(source: &str, range: Range<usize>, edited: &str) -> String {
    let mut out = String::with_capacity(source.len() - range.len() + edited.len());
    out.push_str(&source[..range.start]);
    out.push_str(edited);
    out.push_str(&source[range.end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWCASE: &str = include_str!("../samples/showcase.md");

    fn assert_tiles(source: &str) {
        let blocks = segment(source);
        if source.is_empty() {
            assert!(blocks.is_empty());
            return;
        }
        assert_eq!(blocks.first().map(|b| b.start), Some(0));
        assert_eq!(blocks.last().map(|b| b.end), Some(source.len()));
        for pair in blocks.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        let rebuilt: String = blocks.iter().map(|b| &source[b.clone()]).collect();
        assert_eq!(rebuilt, source);
    }

    #[test]
    fn tiles_showcase_file() {
        assert_tiles(SHOWCASE);
    }

    #[test]
    fn tiles_edge_cases() {
        assert_tiles("");
        assert_tiles("just one paragraph");
        assert_tiles("# heading only");
        assert_tiles("\n\n\nleading blanks\n\n\n");
        assert_tiles("---\n");
        assert_tiles("a\n\n---\n\nb\n");
        assert_tiles("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert_tiles("```\nfence\n```");
        assert_tiles("text\n\n[ref]: https://example.com\n");
    }

    #[test]
    fn splice_identity_roundtrip() {
        let blocks = segment(SHOWCASE);
        assert!(blocks.len() > 10, "showcase file should have many blocks");
        for block in blocks {
            let slice = &SHOWCASE[block.clone()];
            assert_eq!(splice(SHOWCASE, block, slice), SHOWCASE);
        }
    }

    #[test]
    fn strips_fence_languages() {
        let stripped = strip_fence_languages(SHOWCASE);
        for lang in ["```rust", "```python", "```json", "```sh"] {
            assert!(SHOWCASE.contains(lang), "showcase file should contain {lang}");
            assert!(!stripped.contains(lang), "stripped should not contain {lang}");
        }
        // Code content and everything else survives untouched.
        assert!(stripped.contains("fn segment(source: &str)"));
        assert_eq!(
            stripped.len(),
            SHOWCASE.len() - "rust".len() - "python".len() - "json".len() - "sh".len()
        );
        // A file with no fences is returned verbatim.
        assert_eq!(strip_fence_languages("plain\n\ntext"), "plain\n\ntext");
    }

    #[test]
    fn splice_replaces_block() {
        let source = "first\n\nsecond\n\nthird\n";
        let blocks = segment(source);
        assert_eq!(blocks.len(), 3);
        let spliced = splice(source, blocks[1].clone(), "SECOND\n\n");
        assert_eq!(spliced, "first\n\nSECOND\n\nthird\n");
    }
}
