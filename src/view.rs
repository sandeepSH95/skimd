use iced::widget::{column, container, markdown, rich_text, scrollable, text};
use iced::{Element, Fill, Pixels, padding};
use pulldown_cmark::HeadingLevel;

use crate::state::{Message, State};
use crate::theme;

pub fn view(state: &State) -> Element<'_, Message> {
    if state.path.is_none() {
        let hint = match &state.error {
            Some(error) => text(error).style(text::danger),
            None => text("Drop a Markdown file here").style(text::secondary),
        };
        return container(hint).center(Fill).into();
    }

    let settings = theme::markdown_settings(state.mode);
    let doc = markdown::view_with(state.content.items(), settings, &SkimViewer)
        .map(Message::LinkClicked);

    let mut page = column![];
    if let Some(error) = &state.error {
        page = page.push(text(error).style(text::danger));
    }
    page = page.push(doc);

    scrollable(
        container(page.spacing(settings.spacing).max_width(720).padding([48, 32]))
            .center_x(Fill),
    )
    .width(Fill)
    .height(Fill)
    .into()
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
