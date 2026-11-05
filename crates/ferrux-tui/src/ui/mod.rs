pub mod header;
pub mod layout;
#[cfg(test)]
mod tests;
pub mod theme;

use crate::{
    metadata::GitInfo,
    widgets::{sidebar, status_line},
};
use ferrux_core::{
    domain::{
        agent::AgentKind,
        workspace::{PaneId, Workspace},
    },
    layout::split_tree::PaneRect,
};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::collections::HashMap;
use theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected,
    Disconnected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Config,
    Workspaces,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneStatus {
    pub running: Option<bool>,
    pub agent: Option<AgentKind>,
}
pub struct UiState {
    pub theme: Theme,
    pub sidebar_visible: bool,
    pub sessions_expanded: bool,
    pub sidebar_scroll: usize,
    pub follow_focus: bool,
    pub sidebar_focus: bool,
    pub connection: Connection,
    pub statuses: HashMap<PaneId, PaneStatus>,
    pub git: GitInfo,
    pub overlay: Option<Overlay>,
    pub workspaces: Vec<String>,
    pub workspace_selection: usize,
    pub notice: Option<String>,
    pub prefix: bool,
}
impl Default for UiState {
    fn default() -> Self {
        Self {
            theme: Theme::detect(),
            sidebar_visible: true,
            sessions_expanded: true,
            sidebar_scroll: 0,
            follow_focus: true,
            sidebar_focus: false,
            connection: Connection::Connecting,
            statuses: HashMap::new(),
            git: GitInfo::default(),
            overlay: None,
            workspaces: Vec::new(),
            workspace_selection: 0,
            notice: None,
            prefix: false,
        }
    }
}
impl UiState {
    pub fn mode(&self) -> &'static str {
        match self.overlay {
            Some(Overlay::Help) => "HELP",
            Some(Overlay::Config) => "CONFIG",
            Some(Overlay::Workspaces) => "WORKSPACES",
            None if self.sidebar_focus => "SESSIONS",
            None if self.prefix => "PREFIX",
            None => "NORMAL",
        }
    }
}

pub fn render(
    frame: &mut Frame<'_>,
    workspace: &Workspace,
    focused: Option<PaneId>,
    state: &mut UiState,
    mut draw_pane: impl FnMut(&mut Frame<'_>, &PaneRect, Theme),
) {
    let geometry = layout::shell(frame.area(), state.sidebar_visible);
    let theme = state.theme;
    frame.render_widget(Block::default().style(theme.base()), frame.area());
    frame.render_widget(
        Paragraph::new(header::render(
            geometry.header.width,
            &workspace.name,
            workspace.panes.len(),
            state.connection,
            &state.git,
            theme,
        ))
        .style(theme.bar()),
        geometry.header,
    );
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(geometry.divider.width)))
            .style(Style::default().fg(theme.brand).bg(theme.background)),
        geometry.divider,
    );
    sidebar::render(frame, geometry.sidebar, workspace, focused, state);
    if let Some(root) = &workspace.root {
        let panes = layout::panes(root, geometry.panes);
        for pane in &panes {
            if pane.rect.width > 0 && pane.rect.height > 0 {
                draw_pane(frame, pane, theme);
            }
        }
        merge_junctions(frame, &panes, focused, theme);
    }
    frame.render_widget(
        status_line::render(
            geometry.status.width,
            state.mode(),
            workspace.panes.len(),
            theme,
            state.notice.as_deref(),
        ),
        geometry.status,
    );
    if state.overlay.is_some() {
        render_overlay(frame, state);
    }
}

fn merge_junctions(
    frame: &mut Frame<'_>,
    panes: &[PaneRect],
    focused: Option<PaneId>,
    theme: Theme,
) {
    let area = frame.area();
    let width = usize::from(area.width);
    if width == 0 || area.height == 0 {
        return;
    }
    let mut masks = vec![0u8; width * usize::from(area.height)];
    let mut active = vec![false; masks.len()];
    let mut add = |x: u16, y: u16, mask: u8, is_active: bool| {
        if area.contains((x, y).into()) {
            let index = usize::from(y - area.y) * width + usize::from(x - area.x);
            masks[index] |= mask;
            active[index] |= is_active;
        }
    };
    for pane in panes {
        let r = layout::rect(pane.rect);
        if r.width < 2 || r.height < 2 {
            continue;
        }
        let right = r.right() - 1;
        let bottom = r.bottom() - 1;
        let selected = focused == Some(pane.pane);
        for x in r.x..=right {
            let mask = if x == r.x {
                2
            } else if x == right {
                1
            } else {
                3
            };
            add(
                x,
                r.y,
                mask | if x == r.x || x == right { 8 } else { 0 },
                selected,
            );
            add(
                x,
                bottom,
                mask | if x == r.x || x == right { 4 } else { 0 },
                selected,
            );
        }
        for y in r.y + 1..bottom {
            add(r.x, y, 12, selected);
            add(right, y, 12, selected);
        }
    }
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let index = usize::from(y - area.y) * width + usize::from(x - area.x);
            let mask = masks[index];
            let cell = &mut frame.buffer_mut()[(x, y)];
            let is_border = matches!(
                cell.symbol(),
                "─" | "│" | "┌" | "┐" | "└" | "┘" | "├" | "┤" | "┬" | "┴" | "┼"
            );
            let symbol = match mask {
                7 => Some("┴"),
                11 => Some("┬"),
                13 => Some("┤"),
                14 => Some("├"),
                15 => Some("┼"),
                _ => None,
            };
            if let Some(symbol) = symbol {
                cell.set_symbol(symbol);
            }
            if mask != 0 && (is_border || symbol.is_some()) {
                cell.set_fg(if active[index] {
                    theme.focus
                } else {
                    theme.muted
                });
            }
        }
    }
}

fn render_overlay(frame: &mut Frame<'_>, state: &UiState) {
    let available = frame.area();
    let width = available.width.min(70);
    let height = available.height.min(23);
    let area = Rect::new(
        available.x + (available.width - width) / 2,
        available.y + (available.height - height) / 2,
        width,
        height,
    );
    let Some(overlay) = state.overlay else {
        return;
    };
    let theme = state.theme;
    let (title, lines) = match overlay {
        Overlay::Help => (
            " Keyboard & mouse ",
            vec![
                "Ctrl+B then v / h   Split vertical / horizontal".to_owned(),
                "Ctrl+B then x       Close the active pane".into(),
                "Ctrl+B then s       Save this workspace".into(),
                "Ctrl+O              Focus the next pane".into(),
                "Left click          Focus a pane / session".into(),
                "Ctrl+B then j / k   Next / previous session".into(),
                "Ctrl+B then Tab     Keyboard session navigation".into(),
                "Ctrl+B then b       Show / hide sidebar".into(),
                "Ctrl+B then w       Open a saved workspace".into(),
                "Ctrl+B then ? / ,   Help / configuration".into(),
                "Mouse wheel         Scroll terminal / session list".into(),
                "Shift+PgUp/PgDn     Scroll pane history by page".into(),
                "Shift+Home/End      Oldest history / live output".into(),
                "Shift+wheel         Force local pane scrollback".into(),
                "Ctrl+Q              Detach; shells keep running".into(),
                "Esc                 Close this overlay".into(),
            ],
        ),
        Overlay::Config => (
            " Configuration ",
            vec![
                "Theme: FerruxDark / Football Heritage".into(),
                format!("Color mode: {:?}", theme.mode),
                "FERRUX_COLOR_MODE=rgb | 256 | mono".into(),
                "NO_COLOR disables UI color accents".into(),
                format!(
                    "Scrollback: {} lines per terminal",
                    crate::pane_session::scrollback_lines()
                ),
                "Child ANSI colors are preserved".into(),
                format!(
                    "Sidebar: {}",
                    if state.sidebar_visible {
                        "visible (responsive)"
                    } else {
                        "hidden"
                    }
                ),
                "Existing shortcuts remain unchanged".into(),
                "Esc closes this read-only view".into(),
            ],
        ),
        Overlay::Workspaces => {
            let mut lines = vec![
                "Select a saved layout with ↑/↓ and Enter".into(),
                "Existing daemon panes remain alive".into(),
                String::new(),
            ];
            if state.workspaces.is_empty() {
                lines.push("No saved workspaces. Use Ctrl+B then s.".into());
            }
            for (index, name) in state
                .workspaces
                .iter()
                .enumerate()
                .skip(state.workspace_selection.saturating_sub(8))
                .take(12)
            {
                lines.push(format!(
                    "{} {name}",
                    if index == state.workspace_selection {
                        ">"
                    } else {
                        " "
                    }
                ));
            }
            (" Saved workspaces ", lines)
        }
    };
    frame.render_widget(Clear, area);
    let text: Vec<Line<'_>> = lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line, theme.base())))
        .collect();
    frame.render_widget(
        Paragraph::new(text).style(theme.base()).block(
            Block::default()
                .title(title)
                .title_style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.focus)),
        ),
        area,
    );
}

pub fn shell_label(shell: &str) -> &str {
    shell.rsplit(['\\', '/']).next().unwrap_or(shell)
}
