use crate::ui::{shell_label, UiState};
use ferrux_core::domain::workspace::{PaneId, Workspace};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
pub fn capacity(area: Rect) -> usize {
    usize::from(if area.height >= 20 {
        area.height.saturating_sub(17)
    } else if area.height >= 16 {
        area.height.saturating_sub(12)
    } else {
        area.height.saturating_sub(4)
    })
    .max(1)
}
pub fn clicked_session(
    area: Rect,
    workspace: &Workspace,
    state: &UiState,
    column: u16,
    row: u16,
) -> Option<PaneId> {
    if area.contains((column, row).into())
        && state.sessions_expanded
        && row >= area.y + 3
        && usize::from(row - area.y - 3) < capacity(area)
    {
        workspace
            .panes
            .get(state.sidebar_scroll + usize::from(row - area.y - 3))
            .map(|pane| pane.id)
    } else {
        None
    }
}
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    workspace: &Workspace,
    focused: Option<PaneId>,
    state: &mut UiState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = state.theme;
    let rows = capacity(area);
    if state.follow_focus {
        if let Some(index) = workspace
            .panes
            .iter()
            .position(|pane| Some(pane.id) == focused)
        {
            if index < state.sidebar_scroll {
                state.sidebar_scroll = index;
            }
            if index >= state.sidebar_scroll + rows {
                state.sidebar_scroll = index + 1 - rows;
            }
        }
        state.follow_focus = false;
    }
    state.sidebar_scroll = state
        .sidebar_scroll
        .min(workspace.panes.len().saturating_sub(rows));
    frame.render_widget(
        Block::default()
            .style(theme.base())
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(theme.muted)),
        area,
    );
    let heading = |text: &str| {
        Line::from(Span::styled(
            text.to_owned(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ))
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                " SESSIONS ",
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("({})", workspace.panes.len()),
                Style::default().fg(theme.warning),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            format!(
                " {} {}",
                if state.sessions_expanded {
                    "▾"
                } else {
                    "▸"
                },
                workspace.name
            ),
            Style::default().fg(theme.secondary),
        )),
    ];
    if state.sessions_expanded {
        for pane in workspace.panes.iter().skip(state.sidebar_scroll).take(rows) {
            let selected = Some(pane.id) == focused;
            let status = state.statuses.get(&pane.id);
            let indicator = match status.and_then(|s| s.running) {
                Some(true) => "●",
                Some(false) => "×",
                None => "?",
            };
            let color = match status.and_then(|s| s.running) {
                Some(true) => theme.success,
                Some(false) => theme.muted,
                None => theme.warning,
            };
            let style = if selected {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD)
            } else {
                theme.base().fg(theme.secondary)
            };
            let agent = status
                .and_then(|s| s.agent)
                .map(|kind| format!(" {kind:?}"))
                .unwrap_or_default();
            lines.push(
                Line::from(vec![
                    Span::styled(if selected { " > " } else { "   " }, style),
                    Span::styled(indicator, style.fg(color)),
                    Span::styled(
                        format!(" {} {}{agent}", pane.id.0, shell_label(&pane.shell)),
                        style,
                    ),
                ])
                .style(style),
            );
        }
    }
    while lines.len() < 3 + rows {
        lines.push(Line::raw(""));
    }
    if area.height >= 16 {
        lines.push(Line::from(Span::styled(
            " ───────────────────────",
            Style::default().fg(theme.muted),
        )));
        lines.push(heading(" WORKSPACE"));
        lines.push(Line::from(format!(
            " ▣ {}",
            state.git.repository.as_deref().unwrap_or(&workspace.name)
        )));
        lines.push(Line::from(Span::styled(
            format!(
                " branch: {}",
                state
                    .git
                    .branch
                    .as_deref()
                    .unwrap_or("not a Git repository")
            ),
            Style::default().fg(theme.info),
        )));
        lines.push(Line::from(Span::styled(
            match state.git.dirty {
                Some(true) => " ! working tree modified",
                Some(false) => " ✓ working tree clean",
                None => " Git status unavailable",
            },
            Style::default().fg(match state.git.dirty {
                Some(true) => theme.warning,
                Some(false) => theme.success,
                None => theme.muted,
            }),
        )));
    }
    if area.height >= 20 {
        lines.push(Line::raw(""));
        lines.push(heading(" ACTIVITY"));
        let known = workspace
            .panes
            .iter()
            .filter_map(|p| state.statuses.get(&p.id).and_then(|s| s.running))
            .count();
        let running = workspace
            .panes
            .iter()
            .filter(|p| {
                state
                    .statuses
                    .get(&p.id)
                    .is_some_and(|s| s.running == Some(true))
            })
            .count();
        lines.push(Line::from(Span::styled(
            if known == workspace.panes.len() {
                format!(" ● {running} processes running")
            } else {
                format!(
                    " ● {running} running / {} unknown",
                    workspace.panes.len() - known
                )
            },
            Style::default().fg(theme.success),
        )));
        let agents = workspace
            .panes
            .iter()
            .filter(|p| state.statuses.get(&p.id).is_some_and(|s| s.agent.is_some()))
            .count();
        lines.push(Line::from(format!(" {agents} agents detected")));
        lines.push(Line::from(Span::styled(
            " Approvals unavailable",
            Style::default().fg(theme.muted),
        )));
        lines.push(Line::from(Span::styled(
            " Notifications unavailable",
            Style::default().fg(theme.muted),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).style(theme.base()),
        Rect {
            width: area.width.saturating_sub(1),
            ..area
        },
    );
}
