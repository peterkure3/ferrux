use ratatui::style::{Color, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Rgb,
    Indexed,
    Mono,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub mode: ColorMode,
    pub background: Color,
    pub surface: Color,
    pub brand: Color,
    pub selection: Color,
    pub text: Color,
    pub secondary: Color,
    pub muted: Color,
    pub focus: Color,
    pub success: Color,
    pub info: Color,
    pub warning: Color,
    pub error: Color,
}

/// Football Heritage's terminal-specific dark palette. All UI colors live here.
pub struct FerruxDark;

impl FerruxDark {
    pub fn theme(mode: ColorMode) -> Theme {
        let color = |rgb, index| match mode {
            ColorMode::Rgb => {
                let (r, g, b) = rgb;
                Color::Rgb(r, g, b)
            }
            ColorMode::Indexed => Color::Indexed(index),
            ColorMode::Mono => Color::Reset,
        };
        Theme {
            mode,
            background: color((0x14, 0x1b, 0x17), 234),
            surface: color((0x20, 0x27, 0x23), 235),
            brand: color((0x14, 0x7b, 0x4c), 29),
            selection: color((0x1c, 0x51, 0x31), 22),
            text: color((0xf4, 0xf5, 0xee), 230),
            secondary: color((0xe1, 0xe6, 0xdd), 254),
            muted: color((0x69, 0x74, 0x6c), 244),
            focus: color((0x14, 0x7b, 0x4c), 29),
            success: color((0x4a, 0xde, 0x80), 84),
            info: color((0x67, 0xe8, 0xf9), 87),
            warning: color((0xf5, 0x9e, 0x0b), 214),
            error: color((0xf8, 0x71, 0x71), 203),
        }
    }
}

impl Theme {
    pub fn detect() -> Self {
        let mode = match std::env::var("FERRUX_COLOR_MODE").as_deref() {
            Ok("mono") => ColorMode::Mono,
            Ok("256") => ColorMode::Indexed,
            Ok("rgb") => ColorMode::Rgb,
            _ if std::env::var_os("NO_COLOR").is_some()
                || std::env::var("TERM").as_deref() == Ok("dumb") =>
            {
                ColorMode::Mono
            }
            _ if cfg!(windows)
                || std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit") =>
            {
                ColorMode::Rgb
            }
            _ => ColorMode::Indexed,
        };
        FerruxDark::theme(mode)
    }
    pub fn base(self) -> Style {
        Style::default().fg(self.text).bg(self.background)
    }
    pub fn bar(self) -> Style {
        Style::default().fg(self.secondary).bg(self.surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authoritative_brand_mapping_and_fallbacks() {
        let theme = FerruxDark::theme(ColorMode::Rgb);
        assert_eq!(theme.background, Color::Rgb(20, 27, 23));
        assert_eq!(theme.surface, Color::Rgb(32, 39, 35));
        assert_eq!(theme.focus, Color::Rgb(20, 123, 76));
        assert_eq!(theme.text, Color::Rgb(244, 245, 238));
        assert_eq!(theme.warning, Color::Rgb(245, 158, 11));
        assert!(matches!(
            FerruxDark::theme(ColorMode::Indexed).focus,
            Color::Indexed(_)
        ));
        assert_eq!(FerruxDark::theme(ColorMode::Mono).background, Color::Reset);
    }
}
