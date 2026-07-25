use std::collections::HashMap;
use std::io;

use crossterm::event::{self, Event, KeyEvent};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Position;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use ferrux_core::domain::agent::AgentKind;
use ferrux_core::domain::workspace::{PaneId, PaneRecord, SplitDirection, SplitNode, Workspace};
use ferrux_core::layout::split_tree::{self, Rect as CoreRect};
use ferrux_core::ports::session_store::SessionStore;
use ferrux_ipc::{read_message, write_message, Request, Response, PIPE_NAME};
use ferrux_term::Emulator;

use crate::daemon_client::{self, connect_with_retry};
use crate::input::keymap::{self, Action};
use crate::pane_session::{self, PaneSession};
use crate::session_store::TomlSessionStore;
use crate::widgets::{pane_view, sidebar, status_line};

const SCROLLBACK_LINES: usize = 1000;
// cmd.exe ships with every Windows install; pwsh.exe (PowerShell 7+) is
// an optional download and can't be assumed present.
const DEFAULT_SHELL: &str = "cmd.exe";
const SIDEBAR_WIDTH: u16 = 22;

/// Attaches to `pane_id` on the running daemon and renders its output
/// live via a `vt100` emulator + ratatui, forwarding key presses back to
/// the pane. Returns once the pane closes or `Ctrl+Q` is pressed.
///
/// This is the single-pane viewer (`ferrux view <id>`); [`run`] is the
/// full multi-pane workspace.
pub async fn view_pane(pane_id: u64) -> io::Result<()> {
    let mut client = connect_with_retry().await?;
    let _ = PIPE_NAME; // documents which pipe connect_with_retry targets
    write_message(&mut client, &Request::AttachPane { id: pane_id }).await?;
    match read_message(&mut client).await? {
        Response::Ok => {}
        Response::Error { message } => {
            eprintln!("error: {message}");
            return Ok(());
        }
        other => {
            eprintln!("unexpected response: {other:?}");
            return Ok(());
        }
    }

    let (cols, rows) = crossterm::terminal::size()?;
    let emulator = std::sync::Arc::new(std::sync::Mutex::new(Emulator::new(
        rows,
        cols,
        SCROLLBACK_LINES,
    )));

    crossterm::terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (mut pipe_read, mut pipe_write) = tokio::io::split(client);
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<KeyEvent>();
    std::thread::spawn(move || loop {
        match event::read() {
            // Windows reports both key-down and key-up events (unlike
            // Unix ttys); key-up must never reach the prefix-key state
            // machine or it clears `awaiting_prefix` right after Ctrl+B
            // sets it, before the next real keypress arrives.
            Ok(Event::Key(key)) if key.kind != crossterm::event::KeyEventKind::Release => {
                if input_tx.send(key).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    });

    let draw_single = |terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
                        emulator: &std::sync::Mutex<Emulator>|
     -> io::Result<()> {
        let emulator = emulator.lock().unwrap();
        let screen = emulator.screen();
        let text = pane_view::render_screen(screen);
        let cursor = pane_view::cursor_position(screen);
        terminal.draw(|frame| {
            frame.render_widget(Paragraph::new(text), frame.area());
            if let Some((x, y)) = cursor {
                frame.set_cursor_position(Position { x, y });
            }
        })?;
        Ok(())
    };

    draw_single(&mut terminal, &emulator)?;

    let mut buf = [0u8; 4096];
    let result: io::Result<()> = loop {
        tokio::select! {
            result = pipe_read.read(&mut buf) => {
                match result {
                    Ok(0) | Err(_) => break Ok(()),
                    Ok(n) => {
                        emulator.lock().unwrap().process(&buf[..n]);
                        if let Err(e) = draw_single(&mut terminal, &emulator) {
                            break Err(e);
                        }
                    }
                }
            }
            key = input_rx.recv() => {
                match key {
                    Some(key) => match keymap::translate(key, false) {
                        Some(Action::Bytes(bytes)) => {
                            if pipe_write.write_all(&bytes).await.is_err() {
                                break Ok(());
                            }
                        }
                        _ => break Ok(()),
                    },
                    None => break Ok(()),
                }
            }
        }
    };

    crossterm::terminal::disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

/// Opens (creating if it doesn't exist) the named workspace as a full
/// multi-pane session: split-tree layout, sidebar, `Ctrl+O` to cycle
/// focus, tmux-style `Ctrl+B` prefix keybindings (`v`/`h` split, `x`
/// close, `s` save), and TOML persistence under `~/.ferrux/sessions/`.
pub async fn run(name: String) -> io::Result<()> {
    let store = TomlSessionStore::new()?;
    let mut workspace = store.load(&name).unwrap_or_else(|_| Workspace::new(name));

    let (term_cols, term_rows) = crossterm::terminal::size()?;
    let main_area = CoreRect {
        x: SIDEBAR_WIDTH,
        y: 0,
        width: term_cols.saturating_sub(SIDEBAR_WIDTH),
        // Last row is reserved for the keybinding footer.
        height: term_rows.saturating_sub(1),
    };

    if let Some(old_root) = workspace.root.clone() {
        // A loaded workspace's pane ids are stale (the daemon that owned
        // them may not even be the one running now); respawn each leaf
        // and remap the tree onto the fresh ids.
        let mut id_map: HashMap<PaneId, PaneId> = HashMap::new();
        for old_id in split_tree::leaves(&old_root) {
            let shell = workspace.shell_for(old_id).unwrap_or(DEFAULT_SHELL).to_string();
            let new_id = daemon_client::spawn_pane(&shell, 80, 24).await?;
            id_map.insert(old_id, PaneId(new_id));
        }
        let new_panes = id_map
            .iter()
            .map(|(old_id, new_id)| PaneRecord {
                id: *new_id,
                shell: workspace.shell_for(*old_id).unwrap_or(DEFAULT_SHELL).to_string(),
            })
            .collect();
        workspace.root = Some(remap_ids(&old_root, &id_map));
        workspace.panes = new_panes;
    } else {
        let new_id = daemon_client::spawn_pane(DEFAULT_SHELL, 80, 24).await?;
        workspace.root = Some(SplitNode::Leaf(PaneId(new_id)));
        workspace.panes = vec![PaneRecord {
            id: PaneId(new_id),
            shell: DEFAULT_SHELL.to_string(),
        }];
    }

    let (redraw_tx, mut redraw_rx) = mpsc::unbounded_channel::<()>();
    let mut sessions: HashMap<PaneId, PaneSession> = HashMap::new();
    let mut last_sizes: HashMap<PaneId, (u16, u16)> = HashMap::new();
    let mut agent_status: HashMap<PaneId, Option<AgentKind>> = HashMap::new();

    sync_layout(
        workspace.root.as_ref().unwrap(),
        main_area,
        &mut sessions,
        &mut last_sizes,
        redraw_tx.clone(),
    )
    .await?;

    let mut focused = split_tree::leaves(workspace.root.as_ref().unwrap())
        .first()
        .copied();

    crossterm::terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<KeyEvent>();
    std::thread::spawn(move || loop {
        match event::read() {
            // Windows reports both key-down and key-up events (unlike
            // Unix ttys); key-up must never reach the prefix-key state
            // machine or it clears `awaiting_prefix` right after Ctrl+B
            // sets it, before the next real keypress arrives.
            Ok(Event::Key(key)) if key.kind != crossterm::event::KeyEventKind::Release => {
                if input_tx.send(key).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    });

    draw(&mut terminal, &workspace, &sessions, &agent_status, focused, main_area)?;

    let mut awaiting_prefix = false;
    let mut agent_refresh = tokio::time::interval(std::time::Duration::from_secs(2));
    let result: io::Result<()> = 'outer: loop {
        tokio::select! {
            key = input_rx.recv() => {
                let Some(key) = key else { break 'outer Ok(()) };
                let action = keymap::translate(key, awaiting_prefix);
                awaiting_prefix = matches!(action, Some(Action::EnterPrefix));

                match action {
                    Some(Action::Bytes(bytes)) => {
                        if let Some(id) = focused {
                            if let Some(session) = sessions.get(&id) {
                                let _ = session.write_tx.send(bytes);
                            }
                        }
                    }
                    Some(Action::Detach) => break 'outer Ok(()),
                    Some(Action::EnterPrefix) | None => {}
                    Some(Action::SplitVertical) => {
                        if let Err(e) = do_split(
                            &mut workspace, SplitDirection::Vertical, main_area,
                            &mut sessions, &mut last_sizes, &mut focused, redraw_tx.clone(),
                        ).await {
                            break 'outer Err(e);
                        }
                    }
                    Some(Action::SplitHorizontal) => {
                        if let Err(e) = do_split(
                            &mut workspace, SplitDirection::Horizontal, main_area,
                            &mut sessions, &mut last_sizes, &mut focused, redraw_tx.clone(),
                        ).await {
                            break 'outer Err(e);
                        }
                    }
                    Some(Action::FocusNext) => focus_next(&workspace, &mut focused),
                    Some(Action::ClosePane) => {
                        match do_close(
                            &mut workspace, main_area,
                            &mut sessions, &mut last_sizes, &mut focused, redraw_tx.clone(),
                        ).await {
                            Ok(true) => break 'outer Ok(()),
                            Ok(false) => {}
                            Err(e) => break 'outer Err(e),
                        }
                    }
                    Some(Action::SaveWorkspace) => {
                        let _ = store.save(&workspace);
                    }
                }
                draw(&mut terminal, &workspace, &sessions, &agent_status, focused, main_area)?;
            }
            Some(()) = redraw_rx.recv() => {
                draw(&mut terminal, &workspace, &sessions, &agent_status, focused, main_area)?;
            }
            _ = agent_refresh.tick() => {
                if let Ok(panes) = daemon_client::list_panes().await {
                    agent_status = panes
                        .into_iter()
                        .map(|pane| (PaneId(pane.id), pane.agent_kind))
                        .collect();
                }
                draw(&mut terminal, &workspace, &sessions, &agent_status, focused, main_area)?;
            }
        }
    };

    crossterm::terminal::disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn remap_ids(node: &SplitNode, map: &HashMap<PaneId, PaneId>) -> SplitNode {
    match node {
        SplitNode::Leaf(id) => SplitNode::Leaf(*map.get(id).unwrap_or(id)),
        SplitNode::Split {
            direction,
            ratio_percent,
            first,
            second,
        } => SplitNode::Split {
            direction: *direction,
            ratio_percent: *ratio_percent,
            first: Box::new(remap_ids(first, map)),
            second: Box::new(remap_ids(second, map)),
        },
    }
}

/// Attaches any pane in the layout that isn't attached yet, and resizes
/// (daemon-side PTY + local emulator) any pane whose rect changed since
/// the last call.
async fn sync_layout(
    root: &SplitNode,
    main_area: CoreRect,
    sessions: &mut HashMap<PaneId, PaneSession>,
    last_sizes: &mut HashMap<PaneId, (u16, u16)>,
    redraw_tx: mpsc::UnboundedSender<()>,
) -> io::Result<()> {
    for pane_rect in split_tree::layout(root, main_area) {
        let inner = (
            pane_rect.rect.width.saturating_sub(2).max(1),
            pane_rect.rect.height.saturating_sub(2).max(1),
        );
        if let Some(session) = sessions.get(&pane_rect.pane) {
            if last_sizes.get(&pane_rect.pane) != Some(&inner) {
                let _ = daemon_client::resize_pane(pane_rect.pane.0, inner.0, inner.1).await;
                session.emulator.lock().unwrap().set_size(inner.1, inner.0);
                last_sizes.insert(pane_rect.pane, inner);
            }
        } else {
            let session =
                pane_session::attach(pane_rect.pane.0, inner.0, inner.1, redraw_tx.clone())
                    .await?;
            let _ = daemon_client::resize_pane(pane_rect.pane.0, inner.0, inner.1).await;
            sessions.insert(pane_rect.pane, session);
            last_sizes.insert(pane_rect.pane, inner);
        }
    }
    Ok(())
}

async fn do_split(
    workspace: &mut Workspace,
    direction: SplitDirection,
    main_area: CoreRect,
    sessions: &mut HashMap<PaneId, PaneSession>,
    last_sizes: &mut HashMap<PaneId, (u16, u16)>,
    focused: &mut Option<PaneId>,
    redraw_tx: mpsc::UnboundedSender<()>,
) -> io::Result<()> {
    let Some(target) = *focused else { return Ok(()) };
    let Some(root) = workspace.root.clone() else { return Ok(()) };
    let shell = workspace.shell_for(target).unwrap_or(DEFAULT_SHELL).to_string();
    let new_daemon_id = daemon_client::spawn_pane(&shell, 80, 24).await?;
    let new_id = PaneId(new_daemon_id);

    let Some(new_root) = split_tree::split_leaf(&root, target, new_id, direction, 50) else {
        let _ = daemon_client::kill_pane(new_daemon_id).await;
        return Ok(());
    };

    workspace.root = Some(new_root);
    workspace.panes.push(PaneRecord { id: new_id, shell });
    sync_layout(
        workspace.root.as_ref().unwrap(),
        main_area,
        sessions,
        last_sizes,
        redraw_tx,
    )
    .await?;
    *focused = Some(new_id);
    Ok(())
}

/// Returns `Ok(true)` if closing left the workspace empty (caller should
/// stop the session).
async fn do_close(
    workspace: &mut Workspace,
    main_area: CoreRect,
    sessions: &mut HashMap<PaneId, PaneSession>,
    last_sizes: &mut HashMap<PaneId, (u16, u16)>,
    focused: &mut Option<PaneId>,
    redraw_tx: mpsc::UnboundedSender<()>,
) -> io::Result<bool> {
    let Some(target) = *focused else { return Ok(false) };
    let Some(root) = workspace.root.clone() else { return Ok(true) };

    let _ = daemon_client::kill_pane(target.0).await;
    sessions.remove(&target);
    last_sizes.remove(&target);
    workspace.panes.retain(|record| record.id != target);

    match split_tree::remove_leaf(&root, target) {
        split_tree::RemoveOutcome::Empty => {
            workspace.root = None;
            *focused = None;
            Ok(true)
        }
        split_tree::RemoveOutcome::Removed(new_root) => {
            workspace.root = Some(new_root);
            *focused = split_tree::leaves(workspace.root.as_ref().unwrap())
                .first()
                .copied();
            sync_layout(
                workspace.root.as_ref().unwrap(),
                main_area,
                sessions,
                last_sizes,
                redraw_tx,
            )
            .await?;
            Ok(false)
        }
        split_tree::RemoveOutcome::NotFound => Ok(false),
    }
}

fn focus_next(workspace: &Workspace, focused: &mut Option<PaneId>) {
    let Some(root) = &workspace.root else {
        *focused = None;
        return;
    };
    let ids = split_tree::leaves(root);
    if ids.is_empty() {
        *focused = None;
        return;
    }
    let current_idx = focused
        .and_then(|f| ids.iter().position(|id| *id == f))
        .unwrap_or(0);
    *focused = Some(ids[(current_idx + 1) % ids.len()]);
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    workspace: &Workspace,
    sessions: &HashMap<PaneId, PaneSession>,
    agent_status: &HashMap<PaneId, Option<AgentKind>>,
    focused: Option<PaneId>,
    main_area: CoreRect,
) -> io::Result<()> {
    let root = workspace
        .root
        .as_ref()
        .expect("draw is never called once the workspace is empty");
    let pane_rects = split_tree::layout(root, main_area);
    let sidebar_entries: Vec<sidebar::SidebarEntry> = workspace
        .panes
        .iter()
        .map(|record| sidebar::SidebarEntry {
            id: record.id,
            shell: record.shell.clone(),
            agent_kind: agent_status.get(&record.id).copied().flatten(),
        })
        .collect();

    terminal.draw(|frame| {
        let sidebar_rect = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: main_area.x,
            height: main_area.height,
        };
        frame.render_widget(
            sidebar::render(&workspace.name, &sidebar_entries, focused),
            sidebar_rect,
        );

        let mut cursor_to_set = None;
        for pane_rect in &pane_rects {
            let Some(session) = sessions.get(&pane_rect.pane) else { continue };
            let emulator = session.emulator.lock().unwrap();
            let screen = emulator.screen();
            let is_focused = Some(pane_rect.pane) == focused;
            let title = format!(" {} ", pane_rect.pane.0);
            let widget = pane_view::render_bordered(screen, &title, is_focused);
            let rect = ratatui::layout::Rect {
                x: pane_rect.rect.x,
                y: pane_rect.rect.y,
                width: pane_rect.rect.width,
                height: pane_rect.rect.height,
            };
            frame.render_widget(widget, rect);

            if is_focused {
                if let Some((col, row)) = pane_view::cursor_position(screen) {
                    cursor_to_set = Some(Position {
                        x: rect.x + 1 + col,
                        y: rect.y + 1 + row,
                    });
                }
            }
        }
        if let Some(pos) = cursor_to_set {
            frame.set_cursor_position(pos);
        }

        let footer_rect = ratatui::layout::Rect {
            x: 0,
            y: main_area.height,
            width: main_area.x + main_area.width,
            height: 1,
        };
        frame.render_widget(status_line::render(), footer_rect);
    })?;
    Ok(())
}
