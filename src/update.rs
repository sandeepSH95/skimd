use std::path::PathBuf;

use iced::Task;
use iced::widget::markdown;

use crate::parse;
use crate::state::{Message, State};

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
            if state.path.as_deref() == Some(&path) {
                state.content = markdown::Content::parse(&state.source);
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

    let stripped = parse::strip_fence_languages(&source);
    // Equal lengths means nothing was stripped: no highlighting owed.
    state.needs_highlight = stripped.len() != source.len();
    state.content = markdown::Content::parse(&stripped);
    state.source = source;
    state.path = Some(path);
    state.error = None;
}
