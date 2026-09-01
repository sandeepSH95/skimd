use std::sync::OnceLock;

use iced::border;
use iced::font::{self, Font};
use iced::theme::{Mode, Palette};
use iced::widget::markdown;
use iced::{Pixels, Theme, color, padding};

pub const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
pub const INTER_BOLD: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
pub const INTER_ITALIC: &[u8] = include_bytes!("../assets/fonts/Inter-Italic.ttf");
pub const INTER_BOLD_ITALIC: &[u8] = include_bytes!("../assets/fonts/Inter-BoldItalic.ttf");
pub const JBMONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

pub const BODY: Font = Font::with_name("Inter");
pub const MONO: Font = Font::with_name("JetBrains Mono");
pub const HEADING: Font = Font {
    weight: font::Weight::Bold,
    ..BODY
};

const LIGHT: Palette = Palette {
    background: color!(0xFAFAFA),
    text: color!(0x1A1A1A),
    primary: color!(0x0B66C3),
    ..Palette::LIGHT
};

const DARK: Palette = Palette {
    background: color!(0x1E1E20),
    text: color!(0xE6E6E6),
    primary: color!(0x58A6FF),
    ..Palette::DARK
};

pub fn app_theme(mode: Mode) -> Theme {
    static THEMES: OnceLock<(Theme, Theme)> = OnceLock::new();
    let (light, dark) = THEMES.get_or_init(|| {
        (
            Theme::custom("skimd light", LIGHT),
            Theme::custom("skimd dark", DARK),
        )
    });

    match mode {
        Mode::Dark => dark.clone(),
        Mode::Light | Mode::None => light.clone(),
    }
}

pub fn markdown_settings(mode: Mode) -> markdown::Settings {
    let dark = matches!(mode, Mode::Dark);
    let palette = if dark { DARK } else { LIGHT };

    let style = markdown::Style {
        font: BODY,
        inline_code_highlight: markdown::Highlight {
            background: if dark {
                color!(0x2C2C31)
            } else {
                color!(0xECECEC)
            }
            .into(),
            border: border::rounded(4),
        },
        inline_code_padding: padding::left(2).right(2),
        inline_code_color: palette.text,
        inline_code_font: MONO,
        code_block_font: MONO,
        link_color: palette.primary,
    };

    markdown::Settings {
        text_size: Pixels(16.0),
        h1_size: Pixels(30.0),
        h2_size: Pixels(24.0),
        h3_size: Pixels(20.0),
        h4_size: Pixels(17.0),
        h5_size: Pixels(16.0),
        h6_size: Pixels(16.0),
        code_size: Pixels(13.5),
        spacing: Pixels(14.0),
        style,
    }
}
