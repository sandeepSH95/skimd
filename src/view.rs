use iced::keyboard::{Key, key};
use iced::theme::Mode;
use iced::widget::text_editor::{Binding, KeyPress};
use iced::widget::{
    button, column, container, markdown, mouse_area, rich_text, row, scrollable, text,
    text_editor, text_input,
};
use iced::{Element, Fill, Pixels, padding};
use pulldown_cmark::HeadingLevel;

use crate::state::{Editing, Find, Message, State};
use crate::theme;
use crate::update::{EDITOR_ID, FIND_ID, SCROLL_ID};

pub fn view(state: &State) -> Element<'_, Message> {
    if state.path.is_none() {
        let hint = match &state.error {
            Some(error) => text(error).style(text::danger),
            None => text("Drop a Markdown file here").style(text::secondary),
        };
        return container(hint).center(Fill).into();
    }

    if let Some(raw) = &state.raw {
        return with_mode_toggle(raw_view(raw, state.mode), true, state.mode);
    }

    let settings = theme::markdown_settings(state.mode);
    let blocks = state.blocks.iter().enumerate().map(|(i, block)| {
        match &state.editing {
            Some(editing) if editing.index == i => editor_view(editing, state.mode),
            _ => mouse_area(
                markdown::view_with(block.content.items(), settings, &SkimViewer)
                    .map(Message::LinkClicked),
            )
            .on_press(Message::BlockClicked(i))
            .into(),
        }
    });

    let mut page = column![];
    if let Some(error) = &state.error {
        page = page.push(text(error).style(text::danger));
    }
    page = page.extend(blocks);

    // Blocks and the editor capture their own clicks; anything that falls
    // through hit empty background, which commits the active edit.
    let doc = mouse_area(
        scrollable(
            container(page.spacing(settings.spacing).max_width(720).padding([48, 32]))
                .center_x(Fill),
        )
        .id(SCROLL_ID)
        .width(Fill)
        .height(Fill),
    )
    .on_press(Message::CommitActive);

    let mut root = column![];
    if let Some(find) = &state.find {
        root = root.push(find_bar(find));
    }
    if state.disk_changed {
        root = root.push(banner(
            "File changed on disk",
            row![
                button(text("Reload").size(13)).on_press(Message::ReloadFromDisk),
                button(text("Keep mine").size(13)).on_press(Message::KeepMine),
            ]
            .spacing(8)
            .into(),
        ));
    }
    if state.quit_armed {
        root = root.push(banner(
            "Unsaved changes. Save with Cmd+S, or quit again to discard.",
            text("").into(),
        ));
    }
    with_mode_toggle(root.push(doc).into(), false, state.mode)
}

/// Overlays the raw/rendered toggle button in the top-right corner:
/// grey while viewing, ink while the raw editor is active.
fn with_mode_toggle(
    content: Element<'_, Message>,
    raw_active: bool,
    mode: Mode,
) -> Element<'_, Message> {
    let base = if matches!(mode, Mode::Dark) {
        iced::Color::WHITE
    } else {
        iced::color!(0x1A1A1A)
    };
    let toggle = button(text("#").font(theme::BODY_ITALIC).size(20))
        .style(move |_theme, status| {
            let hovered = matches!(
                status,
                button::Status::Hovered | button::Status::Pressed
            );
            let alpha = match (raw_active, hovered) {
                (false, false) => 0.2,
                (false, true) => 0.55,
                (true, false) => 0.7,
                (true, true) => 1.0,
            };
            button::Style {
                text_color: base.scale_alpha(alpha),
                ..button::Style::default()
            }
        })
        .padding(4)
        .on_press(Message::ToggleRaw);

    iced::widget::stack![
        content,
        container(toggle)
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            // Optically equidistant: the text box carries line-height
            // whitespace above the glyph, so the top inset is smaller.
            .padding(iced::padding::top(5).right(16)),
    ]
    .into()
}

/// The whole document as one raw markdown editor.
fn raw_view(raw: &iced::widget::text_editor::Content, mode: Mode) -> Element<'_, Message> {
    let hl_theme = match mode {
        Mode::Dark => iced::highlighter::Theme::Base16Ocean,
        Mode::Light | Mode::None => iced::highlighter::Theme::InspiredGitHub,
    };

    text_editor(raw)
        .id(EDITOR_ID)
        .on_action(Message::Edit)
        .key_binding(key_binding)
        .highlight("markdown", hl_theme)
        .wrapping(text::Wrapping::Word)
        .font(theme::MONO)
        .size(14)
        .padding(24)
        .height(Fill)
        .into()
}

fn banner<'a>(message: &'a str, actions: Element<'a, Message>) -> Element<'a, Message> {
    container(
        row![text(message).size(14).width(Fill), actions]
            .spacing(12)
            .align_y(iced::Alignment::Center),
    )
    .padding([8, 16])
    .width(Fill)
    .style(container::warning)
    .into()
}

fn find_bar(find: &Find) -> Element<'_, Message> {
    let counter = if find.query.is_empty() {
        String::new()
    } else if find.matches.is_empty() {
        "0 matches".to_owned()
    } else {
        format!("{}/{}", find.current + 1, find.matches.len())
    };

    container(
        row![
            text_input("Find in document", &find.query)
                .id(FIND_ID)
                .on_input(Message::FindInput)
                .on_submit(Message::FindNext)
                .size(14)
                .width(Fill),
            text(counter).size(13).style(text::secondary),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding([6, 16])
    .width(Fill)
    .into()
}

/// The active block as a raw-markdown editor: mono, markdown-highlighted,
/// Escape commits, Cmd+S commits and saves.
fn editor_view(editing: &Editing, mode: Mode) -> Element<'_, Message> {
    let hl_theme = match mode {
        Mode::Dark => iced::highlighter::Theme::Base16Ocean,
        Mode::Light | Mode::None => iced::highlighter::Theme::InspiredGitHub,
    };

    text_editor(&editing.editor)
        .id(EDITOR_ID)
        .on_action(Message::Edit)
        .key_binding(key_binding)
        .highlight("markdown", hl_theme)
        .wrapping(text::Wrapping::Word)
        .font(theme::MONO)
        .size(14)
        .padding(10)
        .into()
}

fn key_binding(key_press: KeyPress) -> Option<Binding<Message>> {
    if let Key::Named(key::Named::Escape) = key_press.key.as_ref() {
        return Some(Binding::Custom(Message::CommitActive));
    }
    if key_press.modifiers.command() {
        match key_press.key.as_ref() {
            Key::Character("s") => return Some(Binding::Custom(Message::Save)),
            Key::Character("f") => return Some(Binding::Custom(Message::FindOpen)),
            Key::Character("q") | Key::Character("w") => {
                return Some(Binding::Custom(Message::Quit));
            }
            _ => {}
        }
    }
    Binding::from_key_press(key_press)
}

/// Default markdown rendering, except headings are bold — iced's default
/// draws them at heading size but normal weight.
struct SkimViewer;

impl<'a> markdown::Viewer<'a, markdown::Uri> for SkimViewer {
    fn on_link_click(url: markdown::Uri) -> markdown::Uri {
        url
    }

    fn heading(
        &self,
        settings: markdown::Settings,
        level: &'a HeadingLevel,
        text: &'a markdown::Text,
        index: usize,
    ) -> Element<'a, markdown::Uri> {
        let size = match level {
            HeadingLevel::H1 => settings.h1_size,
            HeadingLevel::H2 => settings.h2_size,
            HeadingLevel::H3 => settings.h3_size,
            HeadingLevel::H4 => settings.h4_size,
            HeadingLevel::H5 => settings.h5_size,
            HeadingLevel::H6 => settings.h6_size,
        };
        let style = markdown::Style {
            font: theme::HEADING,
            ..settings.style
        };

        container(
            rich_text(text.spans(style))
                .on_link_click(Self::on_link_click)
                .size(size),
        )
        .padding(padding::top(if index > 0 {
            settings.spacing * 1.25
        } else {
            Pixels::ZERO
        }))
        .into()
    }
}
