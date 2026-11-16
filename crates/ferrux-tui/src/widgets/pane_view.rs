use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use crate::ui::{layout, theme::Theme};
use ferrux_term::vt100;

fn color_to_ratatui(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut modifiers = Modifier::empty();
    if cell.bold() {
        modifiers |= Modifier::BOLD;
    }
    if cell.italic() {
        modifiers |= Modifier::ITALIC;
    }
    if cell.underline() {
        modifiers |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        modifiers |= Modifier::REVERSED;
    }

    Style::default()
        .fg(color_to_ratatui(cell.fgcolor()))
        .bg(color_to_ratatui(cell.bgcolor()))
        .add_modifier(modifiers)
}

/// Renders a `vt100::Screen` grid as a ratatui `Text`, one `Line` per row
/// and one borrowed span per cell. Continuation cells of wide glyphs are
/// already occupied by their leading glyph and must not add a space.
pub fn render_screen(screen: &vt100::Screen) -> Text<'_> {
    let (rows, cols) = screen.size();
    let mut lines = Vec::with_capacity(rows as usize);

    for row in 0..rows {
        let mut spans = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if !cell.contents().is_empty() => {
                    spans.push(Span::styled(cell.contents(), cell_style(cell)));
                }
                Some(cell) => spans.push(Span::styled(" ", cell_style(cell))),
                None => spans.push(Span::raw(" ")),
            }
        }
        lines.push(Line::from(spans));
    }

    Text::from(lines)
}

/// Cursor position to hand to `Frame::set_cursor_position`, or `None` if
/// the pane has hidden its cursor.
pub fn cursor_position(screen: &vt100::Screen) -> Option<(u16, u16)> {
    if screen.hide_cursor() {
        return None;
    }
    let (row, col) = screen.cursor_position();
    Some((col, row))
}

/// Only terminal defaults inherit the theme; explicit child ANSI colors stay exact.
pub fn render_screen_themed(screen: &vt100::Screen, theme: Theme) -> Text<'_> {
    let mut text = render_screen(screen);
    for line in &mut text.lines {
        for span in &mut line.spans {
            if span.style.fg == Some(Color::Reset) {
                span.style.fg = Some(theme.text);
            }
            if span.style.bg == Some(Color::Reset) {
                span.style.bg = Some(theme.background);
            }
        }
    }
    text
}

/// Copies the terminal's already-laid-out cells straight into the frame.
/// Avoid allocating spans and repeating Unicode layout for every pane on
/// every draw. The emulator remains the authority for cell positions.
pub struct TerminalScreen<'a> {
    screen: &'a vt100::Screen,
    theme: Theme,
}

impl<'a> TerminalScreen<'a> {
    pub fn new(screen: &'a vt100::Screen, theme: Theme) -> Self {
        Self { screen, theme }
    }
}

impl Widget for TerminalScreen<'_> {
    fn render(self, area: ratatui::layout::Rect, buffer: &mut ratatui::buffer::Buffer) {
        let visible = area.intersection(buffer.area);
        for y in visible.top()..visible.bottom() {
            for x in visible.left()..visible.right() {
                let target = &mut buffer[(x, y)];
                target.reset();
                target.set_style(self.theme.base());
                let Some(cell) = self.screen.cell(y - area.y, x - area.x) else {
                    continue;
                };
                // Never draw half a wide glyph across a pane boundary.
                if cell.is_wide_continuation() || (cell.is_wide() && x + 1 >= visible.right()) {
                    continue;
                }
                let mut style = cell_style(cell);
                if style.fg == Some(Color::Reset) {
                    style.fg = Some(self.theme.text);
                }
                if style.bg == Some(Color::Reset) {
                    style.bg = Some(self.theme.background);
                }
                target
                    .set_symbol(if cell.contents().is_empty() {
                        " "
                    } else {
                        cell.contents()
                    })
                    .set_style(style);
            }
        }
    }
}

pub fn draw_panel(
    frame: &mut ratatui::Frame<'_>,
    outer: ferrux_core::layout::split_tree::Rect,
    screen: &vt100::Screen,
    title: &str,
    focused: bool,
    theme: Theme,
) {
    let area = layout::rect(outer);
    let inner = layout::viewport(outer);
    frame.render_widget(
        Block::default()
            .style(theme.base())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused { theme.focus } else { theme.muted })),
        area,
    );
    frame.render_widget(TerminalScreen::new(screen, theme), inner);
    if area.width > 2 && area.height > 0 {
        let header = ratatui::layout::Rect::new(area.x + 1, area.y, area.width - 2, 1);
        let style = Style::default().fg(theme.text).bg(if focused {
            theme.selection
        } else {
            theme.surface
        });
        let marker = if !focused {
            ""
        } else if header.width >= 14 {
            " ● ACTIVE "
        } else {
            " * "
        };
        let marker_w = (marker.chars().count() as u16).min(header.width);
        frame.render_widget(
            Paragraph::new(title.to_owned()).style(style.add_modifier(if focused {
                Modifier::BOLD
            } else {
                Modifier::empty()
            })),
            ratatui::layout::Rect {
                width: header.width - marker_w,
                ..header
            },
        );
        frame.render_widget(
            Paragraph::new(marker).style(style.fg(theme.warning).add_modifier(Modifier::BOLD)),
            ratatui::layout::Rect::new(header.right() - marker_w, header.y, marker_w, 1),
        );
    }
}

/// A pane rendered inside a titled border, highlighted when focused. The
/// border eats one cell on each side, so the pane's own `Screen` should
/// have been sized to the inner area, not the full rect it's drawn into.
pub fn render_bordered<'a>(
    screen: &'a vt100::Screen,
    title: &'a str,
    focused: bool,
) -> Paragraph<'a> {
    let theme = Theme::detect();
    let border_style = if focused {
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(theme.focus)
    } else {
        Style::default().fg(theme.muted)
    };
    let title = if focused {
        Line::from(format!("{title}active "))
    } else {
        Line::from(title)
    };
    let title_style = if focused {
        Style::default()
            .fg(theme.text)
            .bg(theme.selection)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    Paragraph::new(render_screen(screen)).block(
        Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_type(if focused {
                BorderType::Double
            } else {
                BorderType::Plain
            })
            .border_style(border_style),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrux_term::Emulator;
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    #[test]
    fn direct_grid_matches_text_renderer_for_colors_unicode_and_clipping() {
        use crate::ui::theme::{ColorMode, FerruxDark};
        for output in [
            "agent: compiling\r\nnext line",
            "\x1b[1;3;4;7;31;44mstyled\x1b[0m plain\r\n\x1b[48;2;18;52;86m\x1b[K",
            "界e\u{301}\x1b[38;2;18;52;86mX\x1b[0m 😀 end\r\n界界界",
            "\x1b[44m\x1b[2J\x1b[?1049hALT\r\n\x1b[32magent",
        ] {
            let mut emulator = Emulator::new(4, 24, 100);
            emulator.process(output.as_bytes());
            for mode in [ColorMode::Rgb, ColorMode::Indexed, ColorMode::Mono] {
                let theme = FerruxDark::theme(mode);
                for width in [0, 1, 2, 3, 8, 24, 30] {
                    // Paragraph reflows narrow Unicode rows by dropping wide
                    // glyphs. A terminal must instead preserve grid positions.
                    if output.contains('界') && width < 24 {
                        continue;
                    }
                    let area = Rect::new(2, 1, width, 5);
                    let mut expected = Buffer::empty(Rect::new(0, 0, 34, 8));
                    let mut actual = expected.clone();
                    Paragraph::new(render_screen_themed(emulator.screen(), theme))
                        .style(theme.base())
                        .render(area, &mut expected);
                    TerminalScreen::new(emulator.screen(), theme).render(area, &mut actual);
                    assert_eq!(
                        actual, expected,
                        "output={output:?}, mode={mode:?}, width={width}"
                    );
                }
            }
        }
    }

    #[test]
    fn direct_grid_clears_old_wide_glyphs_and_stays_inside_buffer() {
        let theme = Theme::detect();
        let mut emulator = Emulator::new(2, 8, 10);
        emulator.process("界e\u{301}X".as_bytes());
        let area = Rect::new(0, 0, 8, 2);
        let mut buffer = Buffer::empty(area);
        TerminalScreen::new(emulator.screen(), theme).render(area, &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "界");
        assert_eq!(buffer[(2, 0)].symbol(), "e\u{301}");
        let mut narrow = Buffer::empty(Rect::new(0, 0, 1, 1));
        TerminalScreen::new(emulator.screen(), theme).render(narrow.area, &mut narrow);
        assert_eq!(narrow[(0, 0)].symbol(), " ");
        let mut narrow = Buffer::empty(Rect::new(0, 0, 3, 1));
        TerminalScreen::new(emulator.screen(), theme).render(narrow.area, &mut narrow);
        assert_eq!(narrow[(0, 0)].symbol(), "界");
        assert_eq!(narrow[(2, 0)].symbol(), "e\u{301}");
        emulator.process(b"\r\x1b[2Kdone");
        TerminalScreen::new(emulator.screen(), theme).render(area, &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "d");
        assert_eq!(buffer[(1, 0)].symbol(), "o");
        assert_eq!(buffer[(4, 0)].symbol(), " ");
        // A cropped destination must retain the source grid's coordinates.
        let mut cropped = Buffer::empty(Rect::new(2, 0, 3, 1));
        TerminalScreen::new(emulator.screen(), theme).render(area, &mut cropped);
        assert_eq!(cropped[(2, 0)].symbol(), "n");
        assert_eq!(cropped[(3, 0)].symbol(), "e");
    }

    #[test]
    fn active_pane_has_distinct_outline_and_label_without_recoloring_terminal() {
        let mut emulator = Emulator::new(3, 16, 0);
        emulator.process(b"\x1b[31mX");
        let theme = Theme::detect();
        let area = Rect::new(0, 0, 18, 5);
        let mut active = Buffer::empty(area);
        let mut inactive = Buffer::empty(area);
        render_bordered(emulator.screen(), " 1 ", true).render(area, &mut active);
        render_bordered(emulator.screen(), " 1 ", false).render(area, &mut inactive);
        assert_eq!(active[(0, 0)].symbol(), "╔");
        assert_eq!(inactive[(0, 0)].symbol(), "┌");
        assert_eq!(active[(0, 1)].fg, theme.focus);
        assert_eq!(inactive[(0, 1)].fg, theme.muted);
        let active_title: String = (0..area.width).map(|x| active[(x, 0)].symbol()).collect();
        let inactive_title: String = (0..area.width).map(|x| inactive[(x, 0)].symbol()).collect();
        assert!(active_title.contains("active"));
        assert!(!inactive_title.contains("active"));
        assert_eq!(active[(1, 1)].symbol(), "X");
        assert_eq!(active[(1, 1)].fg, Color::Indexed(1));
        assert_eq!(active[(1, 1)], inactive[(1, 1)]);
    }

    #[test]
    fn keeps_wide_combining_text_and_style_positions() {
        let mut emulator = Emulator::new(1, 10, 0);
        emulator.process("界e\u{301}\x1b[31mX".as_bytes());
        let text = render_screen(emulator.screen());
        assert_eq!(text.lines[0].to_string(), "界e\u{301}X      ");
        assert_eq!(
            text.lines[0]
                .spans
                .iter()
                .find(|span| span.content == "X")
                .unwrap()
                .style
                .fg,
            Some(Color::Indexed(1))
        );
    }

    #[test]
    fn preserves_blank_background() {
        let mut emulator = Emulator::new(2, 80, 0);
        emulator.process(b"\x1b[44m\x1b[2J");
        let text = render_screen(emulator.screen());
        assert_eq!(text.lines.len(), 2);
        assert!(text.lines[0]
            .spans
            .iter()
            .all(|span| span.style.bg == Some(Color::Indexed(4))));
        assert_eq!(text.lines[0].width(), 80);
    }
}
