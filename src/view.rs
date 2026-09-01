use iced::keyboard::{Key, key};
use iced::theme::Mode;
use iced::widget::text_editor::{Binding, KeyPress};
use iced::widget::{
    column, container, markdown, mouse_area, rich_text, scrollable, text, text_editor,
};
use iced::{Element, Fill, Pixels, padding};
use pulldown_cmark::HeadingLevel;

use crate::state::{Editing, Message, State};
use crate::theme;
use crate::update::EDITOR_ID;

pub fn view(state: &State) -> Element<'_, Message> {
    if state.path.is_none() {
        let hint = match &state.error {
            Some(error) => text(error).style(text::danger),
            None => text("Drop a Markdown file here").style(text::secondary),
        };
        return container(hint).center(Fill).into();
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
    mouse_area(
        scrollable(
            container(page.spacing(settings.spacing).max_width(720).padding([48, 32]))
                .center_x(Fill),
        )
        .width(Fill)
        .height(Fill),
    )
    .on_press(Message::CommitActive)
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
    if key_press.modifiers.command()
        && matches!(key_press.key.as_ref(), Key::Character("s"))
    {
        return Some(Binding::Custom(Message::Save));
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
