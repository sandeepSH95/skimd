mod parse;
mod platform;
mod state;
mod theme;
mod update;
mod view;

use std::path::PathBuf;

use iced::{Size, Subscription};

use state::{Message, State};

#[cfg(feature = "bench-first-frame")]
pub static LAUNCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn main() -> iced::Result {
    #[cfg(feature = "bench-first-frame")]
    {
        let _ = LAUNCH.set(std::time::Instant::now());
        env_logger::init();
    }

    // Warm the expensive global caches concurrently with app startup:
    // cosmic-text's system font scan (~21ms) and syntect's grammar set
    // (~40ms, needed before the highlighted reparse).
    std::thread::spawn(|| {
        let _ = iced::advanced::graphics::text::font_system();
        warm_highlighter();
    });

    // Must precede the event loop: catches "Open With" Apple Events that
    // arrive during app launch.
    platform::install_open_handler();

    #[cfg(feature = "bench-first-frame")]
    if let Some(launch) = LAUNCH.get() {
        eprintln!("open handler installed after {:?}", launch.elapsed());
    }

    let path = std::env::args().nth(1).map(PathBuf::from);

    iced::application(move || state::boot(path.clone()), update::update, view::view)
        .title(title)
        .theme(|state: &State| theme::app_theme(state.mode))
        .subscription(subscription)
        .font(theme::INTER_REGULAR)
        .font(theme::INTER_BOLD)
        .font(theme::INTER_ITALIC)
        .font(theme::INTER_BOLD_ITALIC)
        .font(theme::JBMONO_REGULAR)
        .default_font(theme::BODY)
        .window_size(Size::new(820.0, 900.0))
        .centered()
        .run()
}

/// Forces syntect's lazily-compiled grammar set. Idempotent: the first
/// caller compiles (~40ms), concurrent callers block until it's done.
pub fn warm_highlighter() {
    let _ = iced::highlighter::Stream::new(&iced::highlighter::Settings {
        theme: iced::highlighter::Theme::Base16Ocean,
        token: "rust".to_owned(),
    });
}

fn title(state: &State) -> String {
    let name = match &state.path {
        Some(path) => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "skimd".to_owned()),
        None => "skimd".to_owned(),
    };
    if state.dirty {
        format!("{name} — Edited")
    } else {
        name
    }
}

fn subscription(state: &State) -> Subscription<Message> {
    // Frame ticks are only subscribed while owed a highlight pass (or when
    // benchmarking first paint); afterwards the subscription drops away.
    let frames = if state.needs_highlight || cfg!(feature = "bench-first-frame") {
        iced::window::frames().map(|_| Message::FramePresented)
    } else {
        Subscription::none()
    };

    let watch = match &state.path {
        Some(path) => file_watch(path.clone()),
        None => Subscription::none(),
    };

    Subscription::batch([
        frames,
        watch,
        platform::file_opens().map(Message::FileOpened),
        platform::quit_requests().map(|()| Message::Quit),
        iced::system::theme_changes().map(Message::SystemThemeChanged),
        iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                Some(Message::FileOpened(path))
            }
            iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::Quit),
            _ => None,
        }),
        // Global shortcuts, outside the editor (inside, the editor's
        // key_binding wins because captured events never reach this).
        iced::keyboard::listen().filter_map(|event| {
            let iced::keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
                return None;
            };
            if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) =
                key.as_ref()
            {
                return Some(Message::FindClose);
            }
            if !modifiers.command() {
                return None;
            }
            match key.as_ref() {
                iced::keyboard::Key::Character("s") => Some(Message::Save),
                iced::keyboard::Key::Character("f") => Some(Message::FindOpen),
                iced::keyboard::Key::Character("q")
                | iced::keyboard::Key::Character("w") => Some(Message::Quit),
                _ => None,
            }
        }),
    ])
}

/// Watches the open file for external changes. Keyed by path, so switching
/// files restarts the watcher; the notify watcher lives inside the stream
/// state to stay alive for the subscription's lifetime.
fn file_watch(path: std::path::PathBuf) -> Subscription<Message> {
    use iced::futures::StreamExt;
    use notify::Watcher;

    Subscription::run_with(path, |path| {
        let (tx, rx) = iced::futures::channel::mpsc::unbounded();
        let mut watcher = notify::recommended_watcher(
            move |result: Result<notify::Event, notify::Error>| {
                if let Ok(event) = result
                    && matches!(
                        event.kind,
                        notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                    )
                {
                    let _ = tx.unbounded_send(());
                }
            },
        )
        .ok();
        if let Some(watcher) = &mut watcher {
            let _ = watcher.watch(path, notify::RecursiveMode::NonRecursive);
        }

        iced::futures::stream::unfold((watcher, rx), |(watcher, mut rx)| async move {
            rx.next().await?;
            Some((Message::DiskChanged, (watcher, rx)))
        })
    })
}
