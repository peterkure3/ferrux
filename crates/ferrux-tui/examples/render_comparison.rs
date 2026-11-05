//! A/B the pre-optimization renderer and current renderer in one process.
//! The frozen reference stays in this benchmark only, never in Ferrux binaries.
use std::{hint::black_box, time::Instant};

use ferrux_term::{vt100, Emulator};
use ferrux_tui::widgets::pane_view::render_screen;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Widget},
};

fn color(value: vt100::Color) -> Color {
    match value {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

// Original per-cell span construction, copied from the audited revision.
fn reference(screen: &vt100::Screen) -> Text<'_> {
    let (rows, cols) = screen.size();
    let mut lines = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut spans = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            match screen.cell(row, col) {
                Some(cell) => {
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
                    let style = Style::default()
                        .fg(color(cell.fgcolor()))
                        .bg(color(cell.bgcolor()))
                        .add_modifier(modifiers);
                    spans.push(Span::styled(
                        if cell.contents().is_empty() {
                            " "
                        } else {
                            cell.contents()
                        },
                        style,
                    ));
                }
                None => spans.push(Span::raw(" ")),
            }
        }
        lines.push(Line::from(spans));
    }
    Text::from(lines)
}

fn measure(screen: &vt100::Screen, render: fn(&vt100::Screen) -> Text<'_>) -> u128 {
    let area = Rect::new(0, 0, 120, 40);
    let mut buffer = Buffer::empty(area);
    let start = Instant::now();
    for _ in 0..500 {
        buffer.reset();
        Paragraph::new(render(black_box(screen))).render(area, &mut buffer);
        black_box(&buffer);
    }
    start.elapsed().as_nanos()
}

fn main() {
    let mut emulator = Emulator::new(40, 120, 1000);
    emulator.process(
        "\x1b[32magent\x1b[0m: compiling representative output line\r\n"
            .repeat(4096)
            .as_bytes(),
    );
    // Check equality for the ASCII workload where the old renderer is correct.
    let area = Rect::new(0, 0, 120, 40);
    let mut old = Buffer::empty(area);
    let mut new = Buffer::empty(area);
    Paragraph::new(reference(emulator.screen())).render(area, &mut old);
    Paragraph::new(render_screen(emulator.screen())).render(area, &mut new);
    assert_eq!(old, new);
    for sample in 0..7 {
        let (before, after) = if sample % 2 == 0 {
            (
                measure(emulator.screen(), reference),
                measure(emulator.screen(), render_screen),
            )
        } else {
            let after = measure(emulator.screen(), render_screen);
            (measure(emulator.screen(), reference), after)
        };
        println!("frames=500,before_ns={before},after_ns={after}");
    }
}
