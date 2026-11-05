use super::{theme::Theme, Connection};
use crate::metadata::GitInfo;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

pub fn render(
    width: u16,
    workspace: &str,
    sessions: usize,
    connection: Connection,
    git: &GitInfo,
    theme: Theme,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled(" ◆ ", Style::default().fg(theme.brand)),
        Span::styled(
            "Ferrux",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
    ];
    let right = match connection {
        Connection::Connecting => "◌ Connecting",
        Connection::Connected => "● Connected",
        Connection::Disconnected => "! Disconnected",
    };
    let right_color = match connection {
        Connection::Connected => theme.success,
        Connection::Connecting => theme.warning,
        Connection::Disconnected => theme.error,
    };
    let right = if width >= 120 {
        format!("{right}  ^B ? Help  ^B , Config ")
    } else if width >= 65 {
        format!("{right} ")
    } else {
        String::new()
    };
    let available = usize::from(width).saturating_sub(right.chars().count() + 12);
    let label = if width >= 80 {
        format!(" │ Workspace: {workspace}")
    } else {
        format!(" │ {workspace}")
    };
    spans.push(Span::styled(
        label.chars().take(available / 2).collect::<String>(),
        Style::default().fg(theme.secondary),
    ));
    let mut line = Line::from(spans);
    for (value, color) in [
        (git.branch.as_ref().map(|b| format!(" │ {b}")), theme.info),
        (Some(format!(" │ {sessions} Sessions")), theme.warning),
    ] {
        if let Some(value) = value {
            if line.width() + Span::raw(&value).width() + right.chars().count() + 2
                <= usize::from(width)
            {
                line.spans
                    .push(Span::styled(value, Style::default().fg(color)));
            }
        }
    }
    let padding = usize::from(width).saturating_sub(line.width() + right.chars().count());
    line.spans.push(Span::raw(" ".repeat(padding)));
    line.spans
        .push(Span::styled(right, Style::default().fg(right_color)));
    line
}
