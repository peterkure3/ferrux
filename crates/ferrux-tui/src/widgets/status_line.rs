use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const BINDINGS: &[(&str, &str)] = &[
    ("^B %", "split-v"),
    ("^B \"", "split-h"),
    ("^B o", "focus"),
    ("^B x", "close"),
    ("^B s", "save"),
    ("^Q", "detach"),
];

/// A one-line footer listing the active keybindings.
pub fn render() -> Paragraph<'static> {
    let key_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::Gray);
    let sep_style = Style::default().fg(Color::DarkGray);

    let mut spans = Vec::with_capacity(BINDINGS.len() * 3);
    for (i, (key, label)) in BINDINGS.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", sep_style));
        }
        spans.push(Span::styled(format!(" {key} "), key_style));
        spans.push(Span::styled(format!(" {label}"), label_style));
    }

    Paragraph::new(Line::from(spans))
}
