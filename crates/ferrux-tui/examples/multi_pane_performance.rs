//! A/B streaming pane rendering with identical input and output buffers.
//! cargo run --release --locked -p ferrux-tui --example multi_pane_performance
use std::{hint::black_box, time::Instant};

use ferrux_term::Emulator;
use ferrux_tui::{
    ui::theme::{ColorMode, FerruxDark, Theme},
    widgets::pane_view::{render_screen_themed, TerminalScreen},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

const FRAMES: usize = 500;

fn draw(
    screen: &ferrux_term::vt100::Screen,
    area: Rect,
    buffer: &mut Buffer,
    theme: Theme,
    direct: bool,
) {
    if direct {
        TerminalScreen::new(screen, theme).render(area, buffer);
    } else {
        Paragraph::new(render_screen_themed(screen, theme))
            .style(theme.base())
            .render(area, buffer);
    }
}

fn measure(panes: usize, direct: bool) -> u128 {
    let theme = FerruxDark::theme(ColorMode::Rgb);
    let columns = if panes == 1 { 1 } else { 2 };
    let rows = panes.div_ceil(columns);
    let width = 160 / columns as u16;
    let height = 48 / rows as u16;
    let mut emulators: Vec<_> = (0..panes)
        .map(|_| {
            let mut emulator = Emulator::new(height, width, 10_000);
            emulator.process(
                "\x1b[32magent\x1b[0m: compiling 界 e\u{301} output\r\n"
                    .repeat(100)
                    .as_bytes(),
            );
            emulator
        })
        .collect();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 48));
    let start = Instant::now();
    for frame in 0..FRAMES {
        buffer.reset();
        for (index, emulator) in emulators.iter_mut().enumerate() {
            // Every pane emits colored output on every frame; no idle-pane shortcut.
            emulator.process(b"\x1b[36mstreaming\x1b[0m agent output\r\n");
            let area = Rect::new(
                (index % columns) as u16 * width,
                (index / columns) as u16 * height,
                width,
                height,
            );
            draw(
                black_box(emulator.screen()),
                area,
                &mut buffer,
                theme,
                direct,
            );
        }
        black_box((&buffer, frame));
    }
    start.elapsed().as_nanos()
}

fn main() {
    // Verify colored Unicode output against the former production path.
    let theme = FerruxDark::theme(ColorMode::Rgb);
    let area = Rect::new(0, 0, 80, 24);
    let mut emulator = Emulator::new(24, 80, 100);
    emulator.process(
        "\x1b[32magent\x1b[0m: 界 e\u{301}\r\n"
            .repeat(30)
            .as_bytes(),
    );
    let mut before = Buffer::empty(area);
    let mut after = before.clone();
    draw(emulator.screen(), area, &mut before, theme, false);
    draw(emulator.screen(), area, &mut after, theme, true);
    assert_eq!(before, after);
    for panes in [1, 2, 4, 8] {
        for sample in 0..7 {
            let (before, after) = if sample % 2 == 0 {
                (measure(panes, false), measure(panes, true))
            } else {
                let after = measure(panes, true);
                (measure(panes, false), after)
            };
            println!("panes={panes},frames={FRAMES},before_ns={before},after_ns={after}");
        }
    }
}
