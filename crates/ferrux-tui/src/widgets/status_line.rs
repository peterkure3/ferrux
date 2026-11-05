use crate::ui::theme::Theme;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
pub fn render(
    width: u16,
    mode: &str,
    panes: usize,
    theme: Theme,
    notice: Option<&str>,
) -> Paragraph<'static> {
    let mode_style = Style::default()
        .fg(theme.background)
        .bg(if mode == "PREFIX" {
            theme.warning
        } else {
            theme.text
        })
        .add_modifier(Modifier::BOLD);
    let mut line = Line::from(Span::styled(format!(" {mode} "), mode_style));
    let commands = if width >= 115 {
        vec![
            ("^B v", "Split V"),
            ("^B h", "Split H"),
            ("^B x", "Close"),
            ("^B s", "Save"),
            ("^Q", "Detach"),
            ("^O", "Focus"),
            ("^B ?", "Help"),
        ]
    } else {
        vec![
            ("^Q", "out"),
            ("^B ?", "help"),
            ("^B v", "V"),
            ("^B h", "H"),
            ("^O", "next"),
            ("^B s", "save"),
            ("^B x", "close"),
        ]
    };
    for (key, label) in commands {
        if line.width() + key.len() + label.len() + 5 > usize::from(width) {
            continue;
        }
        line.spans.extend([
            Span::styled(" │ ", Style::default().fg(theme.muted)),
            Span::styled(
                key,
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {label}"), Style::default().fg(theme.text)),
        ]);
    }
    let metadata = notice
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{panes}P"));
    if line.width() + metadata.chars().count() + 2 <= usize::from(width) {
        line.spans.push(Span::raw(" ".repeat(
            usize::from(width) - line.width() - metadata.chars().count() - 1,
        )));
        line.spans
            .push(Span::styled(metadata, Style::default().fg(theme.warning)));
    }
    Paragraph::new(line).style(theme.bar())
}
