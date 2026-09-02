use std::ops::Range;
use std::path::PathBuf;

use iced::Task;
use iced::theme::Mode;
use iced::widget::{markdown, text_editor};

/// The entire application state, visible in one place.
pub struct State {
    pub path: Option<PathBuf>,
    /// Source of truth for the file's text.
    pub source: String,
    /// Derived from `source`; rebuilt on load and on every commit.
    pub blocks: Vec<Block>,
    /// `Some` while one block shows its raw markdown in an editor.
    pub editing: Option<Editing>,
    /// Unsaved changes exist.
    pub dirty: bool,
    pub mode: Mode,
    pub error: Option<String>,
    /// The current file has language-tagged code fences rendered without
    /// highlighting; a highlighted reparse is owed after the next frame.
    pub needs_highlight: bool,
    /// The file changed on disk while we hold unsaved edits; the user must
    /// pick a side (banner with Reload / Keep mine).
    pub disk_changed: bool,
    /// A quit was requested with unsaved changes; quitting again discards.
    pub quit_armed: bool,
    /// `Some` while the find bar is open.
    pub find: Option<Find>,
    /// `Some` while the whole document is shown as one raw text editor
    /// (the block view and `editing` are inactive meanwhile).
    pub raw: Option<text_editor::Content>,
    /// A file over the eager-render size limit awaits confirmation
    /// (path, size in bytes). The prompt replaces the document view.
    pub large_pending: Option<(PathBuf, u64)>,
}

/// One top-level markdown block: its byte range in `source` (the ranges
/// tile the source exactly) and its parsed render content.
pub struct Block {
    pub range: Range<usize>,
    pub content: markdown::Content,
}

/// The active raw-markdown editor.
pub struct Editing {
    pub index: usize,
    pub editor: text_editor::Content,
    /// Trailing newlines of the block's slice (the inter-block gap), held
    /// out of the editor and re-attached verbatim on commit.
    pub suffix: String,
}

pub struct Find {
    pub query: String,
    /// Indices of blocks containing the query, in document order.
    pub matches: Vec<usize>,
    /// Position in `matches` of the match jumped to last.
    pub current: usize,
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
    BlockClicked(usize),
    Edit(text_editor::Action),
    /// Splice the active editor's text back into `source` and re-render.
    CommitActive,
    Save,
    Saved(Result<(), String>),
    /// The watched file changed on disk.
    DiskChanged,
    ReloadFromDisk,
    KeepMine,
    Quit,
    FindOpen,
    FindInput(String),
    FindNext,
    FindClose,
    /// Flip between the rendered block view and one whole-document editor.
    ToggleRaw,
    OpenLargeAnyway,
    OpenLargeCancel,
}

pub fn boot(path: Option<PathBuf>) -> (State, Task<Message>) {
    let mut state = State {
        path: None,
        source: String::new(),
        blocks: Vec::new(),
        editing: None,
        dirty: false,
        mode: Mode::None,
        error: None,
        needs_highlight: false,
        disk_changed: false,
        quit_armed: false,
        find: None,
        raw: None,
        large_pending: None,
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
