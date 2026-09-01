use std::path::PathBuf;

use iced::Task;
use iced::theme::Mode;
use iced::widget::markdown;

/// The entire application state, visible in one place.
pub struct State {
    pub path: Option<PathBuf>,
    /// Source of truth for the file's text.
    pub source: String,
    pub content: markdown::Content,
    pub mode: Mode,
    pub error: Option<String>,
    /// The current file has language-tagged code fences rendered without
    /// highlighting; a highlighted reparse is owed after the next frame.
    pub needs_highlight: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    FileOpened(PathBuf),
    /// A frame was presented. Subscribed only while `needs_highlight` (or
    /// in bench builds): highlighting is sequenced strictly after first
    /// paint so syntect's ~40ms never delays it.
    FramePresented,
    /// Syntect's grammars are compiled (a background task waited on them);
    /// reparse the named file with highlighting.
    Rehighlight(PathBuf),
    LinkClicked(markdown::Uri),
    SystemThemeChanged(Mode),
}

pub fn boot(path: Option<PathBuf>) -> (State, Task<Message>) {
    let mut state = State {
        path: None,
        source: String::new(),
        content: markdown::Content::new(),
        mode: Mode::None,
        error: None,
        needs_highlight: false,
    };

    // Read synchronously: a typical markdown file loads in well under a
    // millisecond, so the first frame already shows the full document.
    if let Some(path) = path {
        crate::update::open_file(&mut state, path);
    }

    #[cfg(feature = "bench-first-frame")]
    if let Some(launch) = crate::LAUNCH.get() {
        eprintln!("boot done after {:?}", launch.elapsed());
    }

    (state, iced::system::theme().map(Message::SystemThemeChanged))
}
