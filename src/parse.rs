//! Pure source ↔ block mapping for block-swap editing.

use std::collections::HashMap;
use std::ops::Range;

use iced::widget::markdown;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

use crate::state::Block;

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

/// Parses source into rendered blocks; ranges index into `source`.
///
/// With `highlight: false`, fence languages are stripped per block (slice
/// coordinates are untouched, so ranges stay valid for `splice`); the
/// returned bool reports whether anything was stripped — i.e. whether a
/// highlighted reparse is owed.
///
/// Reference-link definitions ([label]: url) emit no parse events and can
/// live in a different block than their uses, so every collected definition
/// is appended to every block's parse input. Unused definitions render
/// nothing; within a block, its own (earlier) definition wins over an
/// appended duplicate.
pub fn parse_blocks(source: &str, highlight: bool) -> (Vec<Block>, bool) {
    let defs = reference_definitions(source);
    let mut any_stripped = false;

    let blocks = segment(source)
        .into_iter()
        .map(|range| {
            let slice = &source[range.clone()];
            let stripped;
            let text = if highlight {
                slice
            } else {
                stripped = strip_fence_languages(slice);
                any_stripped |= stripped.len() != slice.len();
                &stripped
            };

            let content = parse_one(text, &defs);
            Block { range, content }
        })
        .collect();

    (blocks, any_stripped)
}

/// Rebuilds blocks after `splice(old_source, range, edited)` produced
/// `source`, reusing parsed content wherever possible: blocks entirely
/// before the edit keep identical ranges, blocks entirely after it shift
/// by a constant delta with identical bytes — only the edited region needs
/// parsing. If the edit changed the document's reference definitions,
/// everything reparses (a definition can affect any block).
pub fn reparse_spliced(
    old_blocks: Vec<Block>,
    old_source: &str,
    source: &str,
    range: Range<usize>,
    edited_len: usize,
) -> Vec<Block> {
    let defs = reference_definitions(source);
    if defs != reference_definitions(old_source) {
        return parse_blocks(source, true).0;
    }

    let delta = edited_len as isize - range.len() as isize;
    let edited_end = range.start + edited_len;
    let mut old: HashMap<usize, Block> = old_blocks
        .into_iter()
        .map(|block| (block.range.start, block))
        .collect();

    segment(source)
        .into_iter()
        .map(|new_range| {
            let reused = if new_range.end <= range.start {
                // Entirely before the edit: same offsets, same bytes.
                old.remove(&new_range.start)
                    .filter(|b| b.range == new_range)
            } else if new_range.start >= edited_end {
                // Entirely after: same bytes at offsets shifted by delta.
                new_range
                    .start
                    .checked_add_signed(-delta)
                    .and_then(|old_start| old.remove(&old_start))
                    .filter(|b| b.range.len() == new_range.len())
            } else {
                None
            };

            match reused {
                Some(block) => Block {
                    range: new_range,
                    content: block.content,
                },
                None => {
                    let content = parse_one(&source[new_range.clone()], &defs);
                    Block {
                        range: new_range,
                        content,
                    }
                }
            }
        })
        .collect()
}

fn parse_one(text: &str, defs: &str) -> markdown::Content {
    if defs.is_empty() {
        markdown::Content::parse(text)
    } else {
        let mut with_defs = String::with_capacity(text.len() + defs.len() + 2);
        with_defs.push_str(text);
        with_defs.push_str("\n\n");
        with_defs.push_str(defs);
        markdown::Content::parse(&with_defs)
    }
}

/// Collects reference-link definition lines from the whole document.
///
/// A plain line scan: a matching line inside a code fence is collected too,
/// but a false definition is only visible if a real block references its
/// exact label — accepted for now.
fn reference_definitions(source: &str) -> String {
    let mut defs = String::new();
    for line in source.lines() {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() <= 3
            && trimmed.starts_with('[')
            && trimmed.contains("]:")
        {
            defs.push_str(line);
            defs.push('\n');
        }
    }
    defs
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
