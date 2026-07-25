use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

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
/// and one `Span` per cell, carrying over each cell's colors/attributes.
pub fn render_screen(screen: &vt100::Screen) -> Text<'_> {
    let (rows, cols) = screen.size();
    let mut lines = Vec::with_capacity(rows as usize);

    for row in 0..rows {
        let mut spans = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            match screen.cell(row, col) {
                Some(cell) if !cell.contents().is_empty() => {
                    spans.push(Span::styled(cell.contents(), cell_style(cell)));
                }
                Some(cell) => {
                    spans.push(Span::styled(" ", cell_style(cell)));
                }
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

/// A pane rendered inside a titled border, highlighted when focused. The
/// border eats one cell on each side, so the pane's own `Screen` should
/// have been sized to the inner area, not the full rect it's drawn into.
pub fn render_bordered<'a>(screen: &'a vt100::Screen, title: &'a str, focused: bool) -> Paragraph<'a> {
    let border_style = if focused {
        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
    } else {
        Style::default()
    };
    Paragraph::new(render_screen(screen)).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style),
    )
}
