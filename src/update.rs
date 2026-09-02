use std::path::{Path, PathBuf};

use iced::Task;
use iced::widget::text_editor;

use crate::parse;
use crate::state::{Editing, Find, Message, State};

pub const EDITOR_ID: &str = "active-block";
pub const FIND_ID: &str = "find-input";
pub const SCROLL_ID: &str = "page-scroll";

/// Everything renders eagerly for the first frame, and layout cost grows
/// superlinearly with document size (measured on an M-series MacBook:
/// 0.5MB ≈ 0.6s, 1MB ≈ 1.5s, 2MB ≈ 4.7s, 5MB ≈ 26s to first frame).
/// Beyond this limit the user confirms before we render.
pub const MAX_EAGER_BYTES: u64 = 500_000;

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
            if matches!(action, text_editor::Action::Edit(_)) {
                state.dirty = true;
                state.quit_armed = false;
            }
            if let Some(raw) = &mut state.raw {
                raw.perform(action);
            } else if let Some(editing) = &mut state.editing {
                editing.editor.perform(action);
            }
            Task::none()
        }
        Message::CommitActive => {
            // In raw mode Escape flips back to the rendered view instead.
            if state.raw.is_some() {
                exit_raw(state);
            } else {
                commit_active(state);
            }
            Task::none()
        }
        Message::ToggleRaw => {
            if state.raw.is_some() {
                exit_raw(state);
                Task::none()
            } else {
                commit_active(state);
                state.find = None;
                state.raw = Some(text_editor::Content::with_text(&state.source));
                iced::widget::operation::focus(EDITOR_ID)
            }
        }
        Message::Save => {
            sync_raw(state);
            commit_active(state);
            state.quit_armed = false;
            let Some(path) = state.path.clone() else {
                return Task::none();
            };
            if !state.dirty {
                return Task::none();
            }
            let contents = state.source.clone();
            Task::perform(
                async move { write_atomic(&path, &contents) },
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
        Message::DiskChanged => {
            let Some(path) = &state.path else {
                return Task::none();
            };
            let Ok(disk) = std::fs::read_to_string(path) else {
                return Task::none();
            };
            if disk == state.source {
                // Echo of our own save, or a touch without content change.
            } else if state.dirty || state.editing.is_some() {
                state.disk_changed = true;
            } else {
                load_source(state, disk);
            }
            Task::none()
        }
        Message::ReloadFromDisk => {
            state.disk_changed = false;
            if let Some(path) = state.path.clone() {
                match std::fs::read_to_string(&path) {
                    Ok(disk) => load_source(state, disk),
                    Err(error) => {
                        state.error =
                            Some(format!("Could not reload {}: {error}", path.display()));
                    }
                }
            }
            Task::none()
        }
        Message::KeepMine => {
            state.disk_changed = false;
            Task::none()
        }
        Message::Quit => {
            sync_raw(state);
            let editing_changes = state.editing.is_some();
            if (state.dirty || editing_changes) && !state.quit_armed {
                state.quit_armed = true;
                return Task::none();
            }
            iced::exit()
        }
        Message::FindOpen => {
            if state.raw.is_some() {
                return Task::none();
            }
            if state.find.is_none() {
                state.find = Some(Find {
                    query: String::new(),
                    matches: Vec::new(),
                    current: 0,
                });
            }
            iced::widget::operation::focus(FIND_ID)
        }
        Message::FindInput(query) => {
            let matches = find_matches(&state.source, &state.blocks, &query);
            state.find = Some(Find {
                query,
                matches,
                current: 0,
            });
            scroll_to_current_match(state)
        }
        Message::FindNext => {
            let Some(find) = &mut state.find else {
                return Task::none();
            };
            if find.matches.is_empty() {
                return Task::none();
            }
            find.current = (find.current + 1) % find.matches.len();
            scroll_to_current_match(state)
        }
        Message::FindClose => {
            state.find = None;
            Task::none()
        }
        Message::OpenLargeAnyway => {
            if let Some((path, _)) = state.large_pending.take() {
                open_file_unchecked(state, path);
            }
            Task::none()
        }
        Message::OpenLargeCancel => {
            state.large_pending = None;
            Task::none()
        }
    }
}

/// Opens `path` unless it exceeds the eager-render limit, in which case a
/// confirmation prompt is shown instead (see `MAX_EAGER_BYTES`).
pub fn open_file(state: &mut State, path: PathBuf) {
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > MAX_EAGER_BYTES
    {
        state.large_pending = Some((path, meta.len()));
        return;
    }
    open_file_unchecked(state, path);
}

/// Loads and shows `path` immediately, with code fences unhighlighted:
/// syntect grammar compilation costs ~40ms, so the highlighted reparse is
/// owed after the next frame instead (see `Message::FramePresented`).
fn open_file_unchecked(state: &mut State, path: PathBuf) {
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
    state.disk_changed = false;
    state.quit_armed = false;
    state.raw = None;
    state.large_pending = None;
    refresh_find(state);
}

/// Replaces the document with `source` (highlighted parse; used for disk
/// reloads, where syntect is warm long since).
fn load_source(state: &mut State, source: String) {
    (state.blocks, _) = parse::parse_blocks(&source, true);
    // A raw editor stays open across a disk reload, refreshed in place.
    if state.raw.is_some() {
        state.raw = Some(text_editor::Content::with_text(&source));
    }
    state.source = source;
    state.editing = None;
    state.dirty = false;
    refresh_find(state);
}

/// Pulls the raw editor's text into `source` (and reparses, so blocks are
/// never stale) without leaving raw mode. Used by Save and Quit so they
/// act on what's on screen.
fn sync_raw(state: &mut State) {
    let Some(raw) = &state.raw else { return };
    let text = raw.text();
    if text != state.source {
        state.source = text;
        (state.blocks, _) = parse::parse_blocks(&state.source, true);
        state.dirty = true;
        refresh_find(state);
    }
}

/// Leaves raw mode, committing its text.
fn exit_raw(state: &mut State) {
    sync_raw(state);
    state.raw = None;
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
    refresh_find(state);
}

/// Writes via a temp file in the same directory plus rename, so a crash
/// mid-write can never leave a truncated file.
fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".skimd-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, contents)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| e.to_string())
}

/// Case-insensitive substring search; returns indices of matching blocks.
fn find_matches(source: &str, blocks: &[crate::state::Block], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let query = query.to_lowercase();
    blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| {
            source[block.range.clone()].to_lowercase().contains(&query)
        })
        .map(|(i, _)| i)
        .collect()
}

fn refresh_find(state: &mut State) {
    if let Some(find) = &mut state.find {
        find.matches = find_matches(&state.source, &state.blocks, &find.query);
        find.current = 0;
    }
}

/// Jumps the scrollable to the current match's block, positioned by its
/// byte offset as a fraction of the document (approximate but cheap).
fn scroll_to_current_match(state: &State) -> Task<Message> {
    let Some(find) = &state.find else {
        return Task::none();
    };
    let Some(&block_index) = find.matches.get(find.current) else {
        return Task::none();
    };
    if state.source.is_empty() {
        return Task::none();
    }
    let fraction =
        state.blocks[block_index].range.start as f32 / state.source.len() as f32;
    iced::widget::operation::snap_to(
        SCROLL_ID,
        iced::widget::operation::RelativeOffset {
            x: 0.0,
            y: fraction,
        },
    )
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
            disk_changed: false,
            quit_armed: false,
            find: None,
            raw: None,
            large_pending: None,
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
        let source = SHOWCASE.repeat(100_000 / SHOWCASE.len() + 1);
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
        commit_active(&mut state);
        let elapsed = start.elapsed();

        println!("commit on {}KB: {elapsed:?}", source.len() / 1000);
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

    #[test]
    fn raw_mode_roundtrip() {
        let mut state = state_with("one\n\ntwo\n");

        // Toggle in and straight back out: byte-identical, not dirty.
        let _ = update(&mut state, Message::ToggleRaw);
        assert!(state.raw.is_some());
        let _ = update(&mut state, Message::ToggleRaw);
        assert!(state.raw.is_none());
        assert_eq!(state.source, "one\n\ntwo\n");
        assert!(!state.dirty);

        // Edit in raw mode, exit: source updated, blocks reparsed, dirty.
        let _ = update(&mut state, Message::ToggleRaw);
        let _ = update(
            &mut state,
            Message::Edit(text_editor::Action::Move(text_editor::Motion::DocumentEnd)),
        );
        let _ = update(
            &mut state,
            Message::Edit(text_editor::Action::Edit(text_editor::Edit::Insert('x'))),
        );
        let _ = update(&mut state, Message::ToggleRaw);
        assert_eq!(state.source, "one\n\ntwo\nx");
        assert_eq!(state.blocks.len(), 2);
        assert!(state.dirty);
    }

    #[test]
    fn find_matches_blocks_case_insensitively() {
        let state = state_with("Alpha beta\n\ngamma\n\nBETA again\n");
        let matches = find_matches(&state.source, &state.blocks, "beta");
        assert_eq!(matches, vec![0, 2]);
        assert!(find_matches(&state.source, &state.blocks, "").is_empty());
        assert!(find_matches(&state.source, &state.blocks, "zeta").is_empty());
    }

    #[test]
    fn quit_arms_when_dirty() {
        let mut state = state_with("hello\n");
        edit_block(&mut state, 0, "changed");
        assert!(state.dirty);
        let _ = update(&mut state, Message::Quit);
        assert!(state.quit_armed, "first quit should arm, not exit");
        // A new edit disarms.
        let _ = activate(&mut state, 0);
        let _ = update(
            &mut state,
            Message::Edit(text_editor::Action::Edit(text_editor::Edit::Insert('y'))),
        );
        assert!(!state.quit_armed);
    }

    #[test]
    fn large_file_prompts_before_loading() {
        let dir = std::env::temp_dir().join("skimd-test-large");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("big.md");
        std::fs::write(&path, "x".repeat(MAX_EAGER_BYTES as usize + 1)).unwrap();

        let mut state = state_with("existing\n");
        open_file(&mut state, path.clone());
        assert!(state.large_pending.is_some(), "should prompt, not load");
        assert_eq!(state.source, "existing\n", "current doc untouched");

        let _ = update(&mut state, Message::OpenLargeAnyway);
        assert!(state.large_pending.is_none());
        assert_eq!(state.path.as_deref(), Some(path.as_path()));
        assert!(state.source.starts_with("xxx"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_writes_and_replaces() {
        let dir = std::env::temp_dir().join("skimd-test-atomic");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("f.md");
        write_atomic(&path, "one").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one");
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
