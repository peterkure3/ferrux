use super::*;
use ferrux_core::domain::workspace::{PaneRecord, SplitDirection, SplitNode};
use ferrux_term::Emulator;
use ratatui::{backend::TestBackend, style::Color, Terminal};
use theme::{ColorMode, FerruxDark};

fn workspace() -> Workspace {
    Workspace {
        name: "ferrux 界".into(),
        root: Some(SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        }),
        panes: vec![
            PaneRecord {
                id: PaneId(1),
                shell: "powershell.exe".into(),
            },
            PaneRecord {
                id: PaneId(2),
                shell: "cmd.exe".into(),
            },
        ],
    }
}

#[test]
fn full_frame_resizes_without_gutters_and_focus_never_changes_geometry() {
    for (width, height) in [
        (80, 24),
        (100, 30),
        (120, 40),
        (160, 48),
        (40, 8),
        (3, 2),
        (1, 1),
        (0, 0),
    ] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let workspace = workspace();
        let mut state = UiState {
            theme: FerruxDark::theme(ColorMode::Rgb),
            connection: Connection::Connected,
            ..UiState::default()
        };
        let geometry = layout::shell(Rect::new(0, 0, width, height), true);
        let panes = layout::panes(workspace.root.as_ref().unwrap(), geometry.panes);
        for focused in [PaneId(1), PaneId(2)] {
            terminal
                .draw(|frame| {
                    render(
                        frame,
                        &workspace,
                        Some(focused),
                        &mut state,
                        |frame, pane, theme| {
                            if let Some((cols, rows)) = layout::pty_size(pane.rect) {
                                let mut emulator = Emulator::new(rows, cols, 0);
                                emulator.process(b"hello");
                                crate::widgets::pane_view::draw_panel(
                                    frame,
                                    pane.rect,
                                    emulator.screen(),
                                    " terminal ",
                                    pane.pane == focused,
                                    theme,
                                );
                            }
                        },
                    )
                })
                .unwrap();
            if width >= 80 {
                assert_eq!(terminal.backend().buffer()[(0, 0)].bg, state.theme.surface);
                let shared = panes[1].rect.x;
                for y in geometry.panes.y + 1..geometry.panes.y + geometry.panes.height - 1 {
                    assert_eq!(terminal.backend().buffer()[(shared, y)].symbol(), "│");
                }
                for (index, x) in [(0, panes[0].rect.x), (1, width - 1)] {
                    let expected = if panes[index].pane == focused {
                        state.theme.focus
                    } else {
                        state.theme.muted
                    };
                    assert_eq!(
                        terminal.backend().buffer()[(x, geometry.panes.y + 2)].fg,
                        expected
                    );
                }
                let row: String = (0..width)
                    .map(|x| terminal.backend().buffer()[(x, height - 1)].symbol())
                    .collect();
                assert!(row.contains("^Q"));
            }
        }
    }
}

#[test]
fn explicit_ansi_colors_unicode_and_alternate_screen_survive_every_ui_palette() {
    for mode in [ColorMode::Rgb, ColorMode::Indexed, ColorMode::Mono] {
        let mut emulator = Emulator::new(3, 16, 10);
        emulator.process("\x1b[38;2;18;52;86m\x1b[48;5;17m界e\u{301}".as_bytes());
        let text = crate::widgets::pane_view::render_screen_themed(
            emulator.screen(),
            FerruxDark::theme(mode),
        );
        assert_eq!(
            text.lines[0].spans[0].style.fg,
            Some(Color::Rgb(18, 52, 86))
        );
        assert_eq!(text.lines[0].spans[0].style.bg, Some(Color::Indexed(17)));
        assert_eq!(text.lines[0].spans[1].content, "e\u{301}");
        emulator.process(b"\x1b[?1049hALT\x1b[?1049l");
        assert!(emulator.screen().contents().contains('界'));
    }
}

#[test]
fn sidebar_selection_scroll_and_unknown_lifecycle_are_real_state() {
    let workspace = workspace();
    let mut state = UiState::default();
    let area = Rect::new(0, 2, 28, 21);
    assert_eq!(
        sidebar::clicked_session(area, &workspace, &state, 5, 6),
        Some(PaneId(2))
    );
    state.sessions_expanded = false;
    assert_eq!(
        sidebar::clicked_session(area, &workspace, &state, 5, 6),
        None
    );
    state.sessions_expanded = true;
    state.statuses.insert(
        PaneId(1),
        PaneStatus {
            running: Some(false),
            agent: None,
        },
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| render(frame, &workspace, Some(PaneId(2)), &mut state, |_, _, _| {}))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let geometry = layout::shell(Rect::new(0, 0, 120, 40), true);
    assert_eq!(
        buffer[(1, geometry.sidebar.y + 4)].bg,
        state.theme.selection
    );
    let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("unknown"));
    assert!(!text.contains("2 processes running"));
}

#[test]
fn responsive_header_and_footer_do_not_overflow_unicode_metadata() {
    for width in [20, 40, 80, 100, 120, 160] {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        let theme = FerruxDark::theme(ColorMode::Rgb);
        terminal
            .draw(|frame| {
                frame.render_widget(
                    status_line::render(width, "NORMAL", 2, theme, None),
                    frame.area(),
                )
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("^Q"));
        assert!(text.contains("NORMAL"));
        let line = header::render(
            width,
            "界".repeat(100).as_str(),
            2,
            Connection::Disconnected,
            &GitInfo::default(),
            theme,
        );
        assert!(line.width() <= usize::from(width));
    }
}

#[test]
fn nested_shared_edges_form_a_junction_without_changing_viewports() {
    let mut workspace = workspace();
    workspace.root = Some(SplitNode::Split {
        direction: SplitDirection::Vertical,
        ratio_percent: 50,
        first: Box::new(SplitNode::Leaf(PaneId(1))),
        second: Box::new(SplitNode::Split {
            direction: SplitDirection::Horizontal,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(2))),
            second: Box::new(SplitNode::Leaf(PaneId(3))),
        }),
    });
    let mut state = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let geometry = layout::shell(Rect::new(0, 0, 120, 40), true);
    let panes = layout::panes(workspace.root.as_ref().unwrap(), geometry.panes);
    terminal
        .draw(|frame| {
            render(
                frame,
                &workspace,
                Some(PaneId(2)),
                &mut state,
                |frame, pane, theme| {
                    let (cols, rows) = layout::pty_size(pane.rect).unwrap();
                    let emulator = Emulator::new(rows, cols, 0);
                    crate::widgets::pane_view::draw_panel(
                        frame,
                        pane.rect,
                        emulator.screen(),
                        " shell ",
                        pane.pane == PaneId(2),
                        theme,
                    );
                },
            )
        })
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(panes[2].rect.x, panes[2].rect.y)].symbol(),
        "├"
    );
}
