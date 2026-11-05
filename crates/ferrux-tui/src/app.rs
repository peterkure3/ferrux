use std::collections::HashMap;
use std::io;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEventKind,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Position;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use ferrux_core::domain::workspace::{PaneId, PaneRecord, SplitDirection, SplitNode, Workspace};
use ferrux_core::layout::split_tree::{self, Rect as CoreRect};
use ferrux_core::ports::session_store::SessionStore;
use ferrux_ipc::{read_message, write_message, Request, Response, PIPE_NAME};
use ferrux_term::Emulator;

use crate::daemon_client::{self, connect_with_retry};
use crate::input::keymap::{self, Action};
use crate::input::mouse;
use crate::pane_session::{self, PaneSession};
use crate::session_store::TomlSessionStore;
use crate::widgets::{pane_view, sidebar};

const SCROLLBACK_LINES: usize = 1000;
// powershell.exe (Windows PowerShell) ships with every Windows install,
// unlike pwsh.exe (PowerShell 7+, an optional download) — safe default.
const DEFAULT_SHELL: &str = "powershell.exe";

struct TerminalMode {
    mouse_capture: bool,
}

impl Drop for TerminalMode {
    fn drop(&mut self) {
        if self.mouse_capture {
            let _ = io::stdout().execute(DisableMouseCapture);
        }
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
    }
}

fn input_events() -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel(64);
    std::thread::spawn(move || {
        while !tx.is_closed() {
            match event::poll(std::time::Duration::from_millis(100)) {
                Ok(false) => continue,
                Err(_) => break,
                Ok(true) => {}
            }
            match event::read() {
                Ok(Event::Key(key)) if key.kind == crossterm::event::KeyEventKind::Release => {}
                // Capture is for click-to-focus; hover/drag/release events
                // must not fill the bounded queue ahead of keyboard input.
                Ok(Event::Mouse(mouse))
                    if !matches!(
                        mouse.kind,
                        MouseEventKind::Down(MouseButton::Left)
                            | MouseEventKind::ScrollUp
                            | MouseEventKind::ScrollDown
                    ) => {}
                Ok(event) => {
                    if tx.blocking_send(event).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

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
    let _terminal_mode = TerminalMode {
        mouse_capture: false,
    };
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (mut pipe_read, mut pipe_write) = tokio::io::split(client);
    let mut input_rx = input_events();

    let theme = crate::ui::theme::Theme::detect();
    let draw_single = |terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
                       emulator: &std::sync::Mutex<Emulator>|
     -> io::Result<()> {
        let emulator = emulator.lock().unwrap();
        let screen = emulator.screen();
        let text = pane_view::render_screen_themed(screen, theme);
        let cursor = pane_view::cursor_position(screen);
        terminal.draw(|frame| {
            frame.render_widget(Paragraph::new(text).style(theme.base()), frame.area());
            if let Some((x, y)) = cursor {
                frame.set_cursor_position(Position { x, y });
            }
        })?;
        Ok(())
    };

    draw_single(&mut terminal, &emulator)?;

    let mut buf = [0u8; 4096];
    let mut dirty = false;
    let mut next_frame = tokio::time::Instant::now();
    let result: io::Result<()> = loop {
        tokio::select! {
            result = pipe_read.read(&mut buf) => {
                match result {
                    Ok(0) | Err(_) => break Ok(()),
                    Ok(n) => {
                        emulator.lock().unwrap().process(&buf[..n]);
                        dirty = true;
                    }
                }
            }
            key = input_rx.recv() => {
                match key {
                    Some(Event::Resize(cols, rows)) => {
                        if let Err(error) = daemon_client::resize_pane(pane_id, cols.max(1), rows.max(1)).await {
                            break Err(error);
                        }
                        emulator.lock().unwrap().set_size(rows.max(1), cols.max(1));
                        dirty = true;
                    }
                    Some(Event::Key(key)) => match keymap::translate(key, false) {
                        Some(Action::Bytes(bytes)) => {
                            if pipe_write.write_all(&bytes).await.is_err() {
                                break Ok(());
                            }
                        }
                        _ => break Ok(()),
                    },
                    Some(_) => {},
                    None => break Ok(()),
                }
            }
            _ = tokio::time::sleep_until(next_frame), if dirty => {
                draw_single(&mut terminal, &emulator)?;
                dirty = false;
                next_frame = tokio::time::Instant::now() + std::time::Duration::from_millis(16);
            }
        }
    };

    result
}

/// Opens (creating if it doesn't exist) the named workspace as a full
/// multi-pane session: split-tree layout, sidebar, `Ctrl+O` to cycle
/// focus, tmux-style `Ctrl+B` prefix keybindings (`v`/`h` split, `x`
/// close, `s` save), and TOML persistence under `~/.ferrux/sessions/`.
pub async fn run(name: String, default_shell: String) -> io::Result<()> {
    use crate::ui::{layout, Connection, Overlay, PaneStatus, UiState};
    use crossterm::event::{KeyCode, KeyModifiers};
    let directory = std::env::current_dir()?;
    let cwd = Some(directory.to_string_lossy().into_owned());
    let store = TomlSessionStore::new()?;
    let mut workspace = restore_workspace(&store, name, &default_shell, cwd.clone()).await?;
    let mut state = UiState::default();
    let mut git = crate::metadata::GitMonitor::start(directory);
    let (cols, rows) = crossterm::terminal::size()?;
    let mut terminal_area = ratatui::layout::Rect::new(0, 0, cols, rows);
    let mut main_area = layout::shell(terminal_area, state.sidebar_visible).panes;
    let (redraw_tx, mut redraw_rx) = mpsc::channel(1);
    let mut sessions = HashMap::new();
    let mut last_sizes = HashMap::new();
    sync_layout(
        workspace.root.as_ref().unwrap(),
        main_area,
        &mut sessions,
        &mut last_sizes,
        redraw_tx.clone(),
    )
    .await?;
    let mut focused = workspace.panes.first().map(|pane| pane.id);
    crossterm::terminal::enable_raw_mode()?;
    let _terminal_mode = TerminalMode {
        mouse_capture: true,
    };
    io::stdout()
        .execute(EnterAlternateScreen)?
        .execute(EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut input_rx = input_events();
    let mut dirty = true;
    let mut next_frame = tokio::time::Instant::now();
    let mut pane_monitor = daemon_client::PaneMonitor::start();
    let result: io::Result<()> = 'outer: loop {
        tokio::select! {
            event=input_rx.recv()=>{
                let Some(event)=event else{break Ok(());};
                match event {
                    Event::Resize(cols,rows)=>{
                        terminal_area=ratatui::layout::Rect::new(0,0,cols,rows);
                        main_area=layout::shell(terminal_area,state.sidebar_visible).panes;
                        sync_layout(workspace.root.as_ref().unwrap(),main_area,&mut sessions,&mut last_sizes,redraw_tx.clone()).await?;
                        dirty=true;
                    }
                    Event::Mouse(event) if state.overlay.is_none()=>{
                        let shell=layout::shell(terminal_area,state.sidebar_visible);
                        match event.kind {
                            MouseEventKind::Down(MouseButton::Left)=>{
                                if shell.sidebar.contains((event.column,event.row).into()) && (event.row==shell.sidebar.y || event.row==shell.sidebar.y+2) {
                                    state.sessions_expanded= !state.sessions_expanded; dirty=true;
                                }else if let Some(id)=sidebar::clicked_session(shell.sidebar,&workspace,&state,event.column,event.row) {
                                    focused=Some(id); state.follow_focus=true; state.prefix=false; state.sidebar_focus=false; dirty=true;
                                }else if mouse::focus_clicked_pane(event,workspace.root.as_ref().unwrap(),main_area,&mut focused) {
                                    state.follow_focus=true; state.prefix=false; state.sidebar_focus=false; dirty=true;
                                }
                            }
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown=>{
                                let up=event.kind==MouseEventKind::ScrollUp;
                                if shell.sidebar.contains((event.column,event.row).into()) {
                                    state.sidebar_scroll=if up{state.sidebar_scroll.saturating_sub(3)}else{state.sidebar_scroll.saturating_add(3)};
                                    state.follow_focus=false; dirty=true;
                                }else{
                                    let mut target=focused;
                                    let click=crossterm::event::MouseEvent{kind:MouseEventKind::Down(MouseButton::Left),..event};
                                    if mouse::focus_clicked_pane(click,workspace.root.as_ref().unwrap(),main_area,&mut target) {
                                        if let Some(session)=target.and_then(|id|sessions.get(&id)) {session.emulator.lock().unwrap().scroll(if up{3}else{-3});dirty=true;}
                                    }
                                }
                            }
                            _=>{}
                        }
                    }
                    Event::Key(key)=>{
                        if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) { break Ok(()); }
                        if let Some(overlay)=state.overlay {
                            match key.code {
                                KeyCode::Esc=>state.overlay=None,
                                KeyCode::Up if overlay==Overlay::Workspaces=>state.workspace_selection=state.workspace_selection.saturating_sub(1),
                                KeyCode::Down if overlay==Overlay::Workspaces=>state.workspace_selection=(state.workspace_selection+1).min(state.workspaces.len().saturating_sub(1)),
                                KeyCode::Enter if overlay==Overlay::Workspaces=>{
                                    if let Some(name)=state.workspaces.get(state.workspace_selection).cloned() {
                                        match restore_workspace(&store,name,&default_shell,cwd.clone()).await {
                                            Ok(new_workspace)=>{
                                                for (_,session) in sessions.drain() {session.detach().await?;}
                                                last_sizes.clear(); workspace=new_workspace; focused=workspace.panes.first().map(|pane|pane.id);
                                                sync_layout(workspace.root.as_ref().unwrap(),main_area,&mut sessions,&mut last_sizes,redraw_tx.clone()).await?;
                                                state.sidebar_scroll=0;state.follow_focus=true;state.overlay=None;
                                            }
                                            Err(error)=>state.notice=Some(format!("Workspace: {error}")),
                                        }
                                    }
                                }
                                _=>{}
                            }
                            dirty=true; continue;
                        }
                        if state.sidebar_focus && !state.prefix && !key.modifiers.contains(KeyModifiers::CONTROL) {
                            match key.code {
                                KeyCode::Up=>focus_session(&workspace,&mut focused,false),
                                KeyCode::Down=>focus_session(&workspace,&mut focused,true),
                                KeyCode::Left=>state.sessions_expanded=false,
                                KeyCode::Right=>state.sessions_expanded=true,
                                KeyCode::Enter|KeyCode::Esc=>state.sidebar_focus=false,
                                _=>{}
                            }
                            state.follow_focus=true;dirty=true;continue;
                        }
                        let action=keymap::translate(key,state.prefix);
                        state.prefix=matches!(action,Some(Action::EnterPrefix));
                        match action {
                            Some(Action::Bytes(bytes))=>{
                                if let Some(session)=focused.and_then(|id|sessions.get(&id)) {
                                    session.emulator.lock().unwrap().scroll(isize::MIN);
                                    if session.write_tx.send(bytes).await.is_err(){break 'outer Err(io::Error::new(io::ErrorKind::BrokenPipe,"pane input closed"));}
                                }
                            }
                            Some(Action::Detach)=>break Ok(()),
                            Some(Action::SplitVertical)|Some(Action::SplitHorizontal)=>{
                                let direction=if matches!(action,Some(Action::SplitVertical)){SplitDirection::Vertical}else{SplitDirection::Horizontal};
                                do_split(&mut workspace,direction,main_area,&mut sessions,&mut last_sizes,&mut focused,redraw_tx.clone(),cwd.clone()).await?;
                                state.follow_focus=true;
                            }
                            Some(Action::ClosePane)=>{
                                if do_close(&mut workspace,main_area,&mut sessions,&mut last_sizes,&mut focused,redraw_tx.clone()).await?{break Ok(());}
                                state.follow_focus=true;
                            }
                            Some(Action::FocusNext)=>{focus_next(&workspace,&mut focused);state.follow_focus=true;}
                            Some(Action::FocusPrevious)=>{focus_previous(&workspace,&mut focused);state.follow_focus=true;}
                            Some(Action::ToggleSidebar)=>{
                                state.sidebar_visible= !state.sidebar_visible;
                                main_area=layout::shell(terminal_area,state.sidebar_visible).panes;
                                sync_layout(workspace.root.as_ref().unwrap(),main_area,&mut sessions,&mut last_sizes,redraw_tx.clone()).await?;
                            }
                            Some(Action::SidebarFocus)=>{state.sidebar_focus= !state.sidebar_focus;state.follow_focus=true;}
                            Some(Action::Help)=>state.overlay=Some(Overlay::Help),
                            Some(Action::Config)=>state.overlay=Some(Overlay::Config),
                            Some(Action::Workspaces)=>{
                                match tokio::task::spawn_blocking(||TomlSessionStore::new()?.list()).await {
                                    Ok(Ok(names))=>{state.workspaces=names;state.workspace_selection=0;state.overlay=Some(Overlay::Workspaces);}
                                    _=>state.notice=Some("Could not read saved workspaces".into()),
                                }
                            }
                            Some(Action::SaveWorkspace)=>match store.save(&workspace) {Ok(())=>state.notice=Some("Workspace saved".into()),Err(error)=>state.notice=Some(format!("Save: {error}"))},
                            Some(Action::EnterPrefix)|None=>{}
                        }
                        dirty=true;
                    }
                    _=>{}
                }
            }
            Some(())=redraw_rx.recv()=>dirty=true,
            Ok(())=git.receiver.changed()=>{state.git=git.receiver.borrow().clone();dirty=true;}
            Ok(())=pane_monitor.receiver.changed()=>{
                use daemon_client::PaneSnapshot;
                let (statuses,connection)=match pane_monitor.receiver.borrow().clone() {
                    PaneSnapshot::Connected(panes)=>(panes.into_iter().map(|pane|(PaneId(pane.id),PaneStatus{running:pane.running,agent:pane.agent_kind})).collect(),Connection::Connected),
                    PaneSnapshot::Connecting=>(state.statuses.clone(),Connection::Connecting),
                    PaneSnapshot::Disconnected=>(state.statuses.keys().map(|id|(*id,PaneStatus{running:None,agent:None})).collect(),Connection::Disconnected),
                };
                if state.statuses!=statuses || state.connection!=connection{state.statuses=statuses;state.connection=connection;dirty=true;}
            }
            _=tokio::time::sleep_until(next_frame),if dirty=>{
                draw(&mut terminal,&workspace,&sessions,focused,&mut state)?;
                dirty=false;next_frame=tokio::time::Instant::now()+std::time::Duration::from_millis(16);
            }
        }
    };
    if result.is_ok() {
        for (_, session) in sessions {
            session.detach().await?;
        }
    }
    result
}

async fn restore_workspace(
    store: &TomlSessionStore,
    name: String,
    default_shell: &str,
    cwd: Option<String>,
) -> io::Result<Workspace> {
    let mut workspace = match store.load(&name) {
        Ok(workspace) => workspace,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Workspace::new(name),
        Err(error) => return Err(error),
    };
    if let Some(root) = workspace.root.clone() {
        let mut map = HashMap::new();
        let mut records = Vec::new();
        for old_id in split_tree::leaves(&root) {
            let shell = workspace
                .shell_for(old_id)
                .unwrap_or(default_shell)
                .to_owned();
            let id = PaneId(daemon_client::spawn_pane(&shell, 80, 24, cwd.clone()).await?);
            map.insert(old_id, id);
            records.push(PaneRecord { id, shell });
        }
        workspace.root = Some(remap_ids(&root, &map));
        workspace.panes = records;
    } else {
        let id = PaneId(daemon_client::spawn_pane(default_shell, 80, 24, cwd).await?);
        workspace.root = Some(SplitNode::Leaf(id));
        workspace.panes = vec![PaneRecord {
            id,
            shell: default_shell.to_owned(),
        }];
    }
    Ok(workspace)
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
    redraw_tx: mpsc::Sender<()>,
) -> io::Result<()> {
    for pane_rect in crate::ui::layout::panes(root, main_area) {
        let Some(inner) = crate::ui::layout::pty_size(pane_rect.rect) else {
            continue;
        };
        if let Some(session) = sessions.get(&pane_rect.pane) {
            if last_sizes.get(&pane_rect.pane) != Some(&inner) {
                daemon_client::resize_pane(pane_rect.pane.0, inner.0, inner.1).await?;
                session.emulator.lock().unwrap().set_size(inner.1, inner.0);
                last_sizes.insert(pane_rect.pane, inner);
            }
        } else {
            let session =
                pane_session::attach(pane_rect.pane.0, inner.0, inner.1, redraw_tx.clone()).await?;
            daemon_client::resize_pane(pane_rect.pane.0, inner.0, inner.1).await?;
            sessions.insert(pane_rect.pane, session);
            last_sizes.insert(pane_rect.pane, inner);
        }
    }
    Ok(())
}

// Preserve the existing orchestration boundary rather than hide mutable
// workspace/session/focus state in a larger abstraction for a lint.
#[allow(clippy::too_many_arguments)]
async fn do_split(
    workspace: &mut Workspace,
    direction: SplitDirection,
    main_area: CoreRect,
    sessions: &mut HashMap<PaneId, PaneSession>,
    last_sizes: &mut HashMap<PaneId, (u16, u16)>,
    focused: &mut Option<PaneId>,
    redraw_tx: mpsc::Sender<()>,
    cwd: Option<String>,
) -> io::Result<()> {
    let Some(target) = *focused else {
        return Ok(());
    };
    let Some(root) = workspace.root.clone() else {
        return Ok(());
    };
    let shell = workspace
        .shell_for(target)
        .unwrap_or(DEFAULT_SHELL)
        .to_string();
    let new_daemon_id = daemon_client::spawn_pane(&shell, 80, 24, cwd).await?;
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
    redraw_tx: mpsc::Sender<()>,
) -> io::Result<bool> {
    let Some(target) = *focused else {
        return Ok(false);
    };
    let Some(root) = workspace.root.clone() else {
        return Ok(true);
    };

    daemon_client::kill_pane(target.0).await?;
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

fn focus_session(workspace: &Workspace, focused: &mut Option<PaneId>, next: bool) {
    let count = workspace.panes.len();
    if count == 0 {
        return;
    }
    let index = workspace
        .panes
        .iter()
        .position(|pane| Some(pane.id) == *focused)
        .unwrap_or(0);
    let index = if next {
        (index + 1) % count
    } else {
        (index + count - 1) % count
    };
    *focused = Some(workspace.panes[index].id);
}
fn focus_previous(workspace: &Workspace, focused: &mut Option<PaneId>) {
    let Some(root) = &workspace.root else {
        return;
    };
    let ids = split_tree::leaves(root);
    if ids.is_empty() {
        return;
    }
    let current = focused
        .and_then(|id| ids.iter().position(|other| *other == id))
        .unwrap_or(0);
    *focused = Some(ids[(current + ids.len() - 1) % ids.len()]);
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    workspace: &Workspace,
    sessions: &HashMap<PaneId, PaneSession>,
    focused: Option<PaneId>,
    state: &mut crate::ui::UiState,
) -> io::Result<()> {
    terminal.draw(|frame| {
        crate::ui::render(frame, workspace, focused, state, |frame, pane, theme| {
            if let Some(session) = sessions.get(&pane.pane) {
                let emulator = session.emulator.lock().unwrap();
                let screen = emulator.screen();
                let shell = workspace.shell_for(pane.pane).unwrap_or(DEFAULT_SHELL);
                let title = format!(" {} {} ", pane.pane.0, crate::ui::shell_label(shell));
                let selected = focused == Some(pane.pane);
                pane_view::draw_panel(frame, pane.rect, screen, &title, selected, theme);
                if selected && screen.scrollback() == 0 {
                    if let Some((col, row)) = pane_view::cursor_position(screen) {
                        let inner = crate::ui::layout::viewport(pane.rect);
                        if col < inner.width && row < inner.height {
                            frame.set_cursor_position(Position {
                                x: inner.x + col,
                                y: inner.y + row,
                            });
                        }
                    }
                }
            }
        });
    })?;
    Ok(())
}
