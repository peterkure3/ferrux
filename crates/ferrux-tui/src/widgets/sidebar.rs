use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};

use ferrux_core::domain::agent::AgentKind;
use ferrux_core::domain::workspace::PaneId;

pub struct SidebarEntry {
    pub id: PaneId,
    pub shell: String,
    pub agent_kind: Option<AgentKind>,
}

fn agent_tag(kind: AgentKind) -> (&'static str, Color) {
    match kind {
        AgentKind::ClaudeCode => ("claude", Color::Magenta),
        AgentKind::Codex => ("codex", Color::Green),
        AgentKind::Unknown => ("agent", Color::Yellow),
    }
}

/// A narrow pane list: workspace name as the title, one line per pane —
/// id, shell, and a colored agent tag when a known agent CLI is
/// detected running under it — the focused pane highlighted.
pub fn render<'a>(
    workspace_name: &'a str,
    entries: &'a [SidebarEntry],
    focused: Option<PaneId>,
) -> List<'a> {
    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            let is_focused = Some(entry.id) == focused;
            let marker = if is_focused { "> " } else { "  " };
            let base_style = if is_focused {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
            };

            let mut spans = vec![Span::styled(
                format!("{marker}{} {}", entry.id.0, entry.shell),
                base_style,
            )];
            if let Some(kind) = entry.agent_kind {
                let (label, color) = agent_tag(kind);
                spans.push(Span::styled(
                    format!(" [{label}]"),
                    base_style.fg(color).add_modifier(Modifier::BOLD),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    List::new(items).block(
        Block::default()
            .title(workspace_name.to_string())
            .borders(Borders::ALL),
    )
}
