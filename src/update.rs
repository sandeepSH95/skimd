use std::path::PathBuf;

use iced::Task;
use iced::widget::text_editor;

use crate::parse;
use crate::state::{Editing, Message, State};

pub const EDITOR_ID: &str = "active-block";

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::FileOpened(path) => {
            open_file(state, path);
            Task::none()
        }
        Message::FramePresented => {
            #[cfg(feature = "bench-first-frame")]
            {
                if let Some(launch) = crate::LAUNCH.get() {
                    eprintln!("first frame after {:?}", launch.elapsed());
                }
                std::process::exit(0);
            }
            #[cfg(not(feature = "bench-first-frame"))]
            {
                if !state.needs_highlight {
                    return Task::none();
                }
                state.needs_highlight = false;
                let Some(path) = state.path.clone() else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        crate::warm_highlighter();
                        path
                    },
                    Message::Rehighlight,
                )
            }
        }
        Message::Rehighlight(path) => {
            // Ignore results for a file we've since navigated away from.
            // Ranges only depend on the source, which is reparsed as it
            // stands now, so an active editor's block index stays valid.
            if state.path.as_deref() == Some(&path) {
                (state.blocks, _) = parse::parse_blocks(&state.source, true);
            }
            Task::none()
        }
        Message::LinkClicked(url) => {
            let _ = open::that_detached(url);
            Task::none()
        }
        Message::SystemThemeChanged(mode) => {
            state.mode = mode;
            Task::none()
        }
        Message::BlockClicked(clicked) => {
            let index = match &state.editing {
                Some(editing) if editing.index == clicked => {
                    return Task::none();
                }
                Some(editing) => {
                    // Committing may resize and re-segment everything before
                    // the click position; chase the clicked block's first
                    // byte through the splice.
                    let clicked_start = state.blocks[clicked].range.start;
                    let edited_end = state.blocks[editing.index].range.end;
                    let old_len = state.source.len();
                    commit_active(state);
                    let delta = state.source.len() as isize - old_len as isize;
                    let offset = if clicked_start >= edited_end {
                        clicked_start.saturating_add_signed(delta)
                    } else {
                        clicked_start
                    };
                    state.blocks.iter().position(|b| b.range.contains(&offset))
                }
                None => Some(clicked),
            };
            match index {
                Some(index) => activate(state, index),
                None => Task::none(),
            }
        }
        Message::Edit(action) => {
            if let Some(editing) = &mut state.editing {
                if matches!(action, text_editor::Action::Edit(_)) {
                    state.dirty = true;
                }
                editing.editor.perform(action);
            }
            Task::none()
        }
        Message::CommitActive => {
            commit_active(state);
            Task::none()
        }
        Message::Save => {
            commit_active(state);
            let Some(path) = state.path.clone() else {
                return Task::none();
            };
            if !state.dirty {
                return Task::none();
            }
            let contents = state.source.clone();
            Task::perform(
                async move { std::fs::write(&path, contents).map_err(|e| e.to_string()) },
                Message::Saved,
            )
        }
        Message::Saved(result) => {
            match result {
                Ok(()) => state.dirty = false,
                Err(error) => state.error = Some(format!("Could not save: {error}")),
            }
            Task::none()
        }
    }
}

/// Loads and shows `path` immediately, with code fences unhighlighted:
/// syntect grammar compilation costs ~40ms, so the highlighted reparse is
/// owed after the next frame instead (see `Message::FramePresented`).
pub fn open_file(state: &mut State, path: PathBuf) {
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            state.error = Some(format!("Could not open {}: {error}", path.display()));
            return;
        }
    };

    (state.blocks, state.needs_highlight) = parse::parse_blocks(&source, false);
    state.source = source;
    state.path = Some(path);
    state.editing = None;
    state.dirty = false;
    state.error = None;
}

/// Swaps the block at `index` to a raw-markdown editor and focuses it.
fn activate(state: &mut State, index: usize) -> Task<Message> {
    let slice = &state.source[state.blocks[index].range.clone()];
    // Hold the inter-block gap (trailing newlines) out of the editor.
    let body = slice.trim_end_matches('\n');
    let suffix = slice[body.len()..].to_owned();

    let mut editor = text_editor::Content::with_text(body);
    editor.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));

    state.editing = Some(Editing {
        index,
        editor,
        suffix,
    });

    iced::widget::operation::focus(EDITOR_ID)
}

/// Splices the active editor's text back into `source` and reparses.
/// A no-op when nothing is being edited or the text is unchanged.
fn commit_active(state: &mut State) {
    let Some(editing) = state.editing.take() else {
        return;
    };
    let range = state.blocks[editing.index].range.clone();

    let mut edited = editing.editor.text();
    edited.push_str(&editing.suffix);

    if edited == state.source[range.clone()] {
        return;
    }

    let source = parse::splice(&state.source, range.clone(), &edited);
    // Re-segmentation handles edits that split or merge blocks; parsed
    // content is reused for every block outside the edited region.
    state.blocks = parse::reparse_spliced(
        std::mem::take(&mut state.blocks),
        &state.source,
        &source,
        range,
        edited.len(),
    );
    state.source = source;
    state.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::markdown;

    const SHOWCASE: &str = include_str!("../samples/showcase.md");

    fn state_with(source: &str) -> State {
        let (blocks, _) = parse::parse_blocks(source, true);
        State {
            path: Some(PathBuf::from("/test.md")),
            source: source.to_owned(),
            blocks,
            editing: None,
            dirty: false,
            mode: iced::theme::Mode::None,
            error: None,
            needs_highlight: false,
        }
    }

    /// `text_editor::Content` must round-trip text exactly — commit
    /// correctness depends on it.
    #[test]
    fn editor_content_roundtrips() {
        for text in [
            "plain",
            "two\nlines",
            "trailing newline typed by user\n\n",
            "",
            "| a | b |\n|---|---|",
        ] {
            let content: text_editor::Content = text_editor::Content::with_text(text);
            assert_eq!(content.text(), text);
        }
    }

    /// Activating any block and committing it unchanged must leave the
    /// source byte-identical and not mark the file dirty.
    #[test]
    fn commit_unchanged_is_identity() {
        let block_count = state_with(SHOWCASE).blocks.len();
        assert!(block_count > 10);

        for index in 0..block_count {
            let mut state = state_with(SHOWCASE);
            let _ = activate(&mut state, index);
            commit_active(&mut state);
            assert_eq!(state.source, SHOWCASE, "block {index} not identity");
            assert!(!state.dirty, "block {index} wrongly dirtied");
        }
    }

    fn edit_block(state: &mut State, index: usize, new_text: &str) {
        let _ = activate(state, index);
        let editing = state.editing.as_mut().unwrap();
        editing.editor.perform(text_editor::Action::SelectAll);
        editing
            .editor
            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                std::sync::Arc::new(new_text.to_owned()),
            )));
        commit_active(state);
    }

    #[test]
    fn commit_splits_block() {
        let mut state = state_with("one\n\ntwo\n");
        assert_eq!(state.blocks.len(), 2);
        edit_block(&mut state, 0, "one\n\nextra");
        assert_eq!(state.source, "one\n\nextra\n\ntwo\n");
        assert_eq!(state.blocks.len(), 3);
        assert!(state.dirty);
    }

    #[test]
    fn commit_merges_blocks() {
        let mut state = state_with("one\n\ntwo\n");
        // Replace block 0 including its gap with a line continuing into
        // block 1's text: "one\ntwo" becomes a single paragraph.
        let _ = activate(&mut state, 0);
        let editing = state.editing.as_mut().unwrap();
        editing.suffix = "\n".to_owned(); // collapse the blank-line gap
        commit_active(&mut state);
        assert_eq!(state.source, "one\ntwo\n");
        assert_eq!(state.blocks.len(), 1);
    }

    #[test]
    fn commit_toggles_checkbox() {
        let source = "- [ ] open\n- [x] done\n";
        let mut state = state_with(source);
        edit_block(&mut state, 0, "- [x] open\n- [x] done");
        assert_eq!(state.source, "- [x] open\n- [x] done\n");
        let markdown::Item::List { bullets, .. } = &state.blocks[0].content.items()[0]
        else {
            panic!("expected list");
        };
        assert!(bullets
            .iter()
            .all(|b| matches!(b, markdown::Bullet::Task { done: true, .. })));
    }

    /// M2 acceptance: commit under 16ms on a ~100KB document. Timing test,
    /// so ignored by default — run with:
    /// cargo test --release commit_latency -- --ignored --nocapture
    #[test]
    #[ignore]
    fn commit_latency_100kb() {
        let source: String = std::iter::repeat(SHOWCASE)
            .take(100_000 / SHOWCASE.len() + 1)
            .collect();
        let mut state = state_with(&source);

        // First editor Content in the process initializes the global font
        // system (~40ms) — warm in the app long before any click, so keep
        // it out of the measurement.
        let _warm: text_editor::Content = text_editor::Content::with_text("warm");

        let start = std::time::Instant::now();
        let _ = activate(&mut state, 3);
        let editing = state.editing.as_mut().unwrap();
        editing
            .editor
            .perform(text_editor::Action::Edit(text_editor::Edit::Insert('x')));
        let activated = start.elapsed();

        let seg_start = std::time::Instant::now();
        let seg = parse::segment(&state.source);
        let seg_time = seg_start.elapsed();
        println!("segment: {seg_time:?} ({} blocks)", seg.len());

        let commit_start = std::time::Instant::now();
        commit_active(&mut state);
        let committed = commit_start.elapsed();
        let elapsed = start.elapsed();

        println!(
            "commit on {}KB: {elapsed:?} (activate+insert {activated:?}, commit {committed:?})",
            source.len() / 1000
        );
        assert!(elapsed.as_millis() < 16, "commit took {elapsed:?}");
    }

    #[test]
    fn click_other_block_commits_and_remaps() {
        // Editing block 0 grows it; clicking block 2 must land on the block
        // that was block 2, at its shifted position.
        let mut state = state_with("aa\n\nbb\n\ncc\n");
        let _ = activate(&mut state, 0);
        state
            .editing
            .as_mut()
            .unwrap()
            .editor
            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                std::sync::Arc::new("longer first block".to_owned()),
            )));
        let _ = update(&mut state, Message::BlockClicked(2));
        let editing = state.editing.as_ref().unwrap();
        assert_eq!(editing.editor.text(), "cc");
        // Activation places the cursor at the block's end, so the paste
        // extends "aa" rather than preceding it.
        assert!(state.source.starts_with("aalonger first block"));
    }
}
