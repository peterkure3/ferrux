use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use ferrux_core::domain::workspace::{PaneId, SplitNode};
use ferrux_core::layout::split_tree::Rect;
use ratatui::{buffer::Buffer, layout::Position, style::Modifier};

/// A selection is confined to the terminal viewport where the drag began.
/// Coordinates refer to the currently displayed grid (including scrollback).
#[derive(Clone, Copy, Debug)]
pub struct TextSelection {
    area: ratatui::layout::Rect,
    anchor: Position,
    end: Position,
    dragging: bool,
    extended: bool,
}

impl TextSelection {
    pub fn start(area: ratatui::layout::Rect, event: MouseEvent) -> Option<Self> {
        let anchor = Position::new(event.column, event.row);
        area.contains(anchor).then_some(Self {
            area,
            anchor,
            end: anchor,
            dragging: true,
            extended: false,
        })
    }

    pub fn update(&mut self, event: MouseEvent) {
        if !self.dragging {
            return;
        }
        if matches!(
            event.kind,
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        ) {
            self.end = Position::new(
                event.column.clamp(self.area.x, self.area.right() - 1),
                event.row.clamp(self.area.y, self.area.bottom() - 1),
            );
            self.extended |= self.end != self.anchor;
            if event.kind == MouseEventKind::Up(MouseButton::Left) {
                self.dragging = false;
            }
        }
    }

    pub fn highlight(&self, buffer: &mut Buffer) {
        if !self.extended {
            return;
        }
        let (start, end) = if (self.anchor.y, self.anchor.x) <= (self.end.y, self.end.x) {
            (self.anchor, self.end)
        } else {
            (self.end, self.anchor)
        };
        let visible = self.area.intersection(buffer.area);
        for y in visible.top()..visible.bottom() {
            for x in visible.left()..visible.right() {
                if (y, x) >= (start.y, start.x) && (y, x) <= (end.y, end.x) {
                    // Toggle inverse so already-inverted child text also changes,
                    // while preserving ANSI colors and other text attributes.
                    let cell = &mut buffer[(x, y)];
                    cell.modifier.toggle(Modifier::REVERSED);
                }
            }
        }
    }
}

/// Ignore hover and unused buttons, but retain the full left-drag lifecycle.
pub fn captures_event(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Up(MouseButton::Left)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
    )
}

/// Select the clicked pane using the same outer rectangles as rendering.
/// Borders belong to their pane; sidebar/footer clicks leave focus alone.
/// Returns true for a valid selection, including clicking the current pane.
pub fn focus_clicked_pane(
    event: MouseEvent,
    root: &SplitNode,
    area: Rect,
    focused: &mut Option<PaneId>,
) -> bool {
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return false;
    }
    let x = u32::from(event.column);
    let y = u32::from(event.row);
    for pane in crate::ui::layout::panes(root, area).into_iter().rev() {
        let rect = pane.rect;
        if x >= u32::from(rect.x)
            && x < u32::from(rect.x) + u32::from(rect.width)
            && y >= u32::from(rect.y)
            && y < u32::from(rect.y) + u32::from(rect.height)
        {
            *focused = Some(pane.pane);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ferrux_core::domain::workspace::SplitDirection;

    fn tree() -> SplitNode {
        SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Split {
                direction: SplitDirection::Horizontal,
                ratio_percent: 50,
                first: Box::new(SplitNode::Leaf(PaneId(2))),
                second: Box::new(SplitNode::Leaf(PaneId(3))),
            }),
        }
    }

    fn area() -> Rect {
        Rect {
            x: 22,
            y: 0,
            width: 100,
            height: 40,
        }
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn captures_drag_and_release_but_not_hover() {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::ScrollUp,
            MouseEventKind::ScrollDown,
        ] {
            assert!(captures_event(kind));
        }
        assert!(!captures_event(MouseEventKind::Moved));
        assert!(!captures_event(MouseEventKind::Drag(MouseButton::Right)));
    }

    #[test]
    fn drag_highlights_linear_text_in_either_direction_and_survives_release() {
        use ratatui::{layout::Rect, style::Color};
        let area = Rect::new(3, 2, 5, 3);
        for (start, end) in [((5, 2), (4, 4)), ((4, 4), (5, 2))] {
            let mut selection = TextSelection::start(area, click(start.0, start.1)).unwrap();
            selection.update(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                ..click(end.0, end.1)
            });
            selection.update(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..click(end.0, end.1)
            });
            // A stray drag after release cannot alter a completed selection.
            selection.update(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                ..click(7, 4)
            });
            let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 7));
            buffer[(6, 3)].set_symbol("X").set_fg(Color::Red);
            buffer[(7, 3)].modifier = Modifier::BOLD | Modifier::REVERSED;
            selection.highlight(&mut buffer);
            for y in 0..7 {
                for x in 0..12 {
                    if (x, y) == (7, 3) {
                        continue;
                    }
                    let expected =
                        area.contains((x, y).into()) && (y, x) >= (2, 5) && (y, x) <= (4, 4);
                    assert_eq!(
                        buffer[(x, y)].modifier.contains(Modifier::REVERSED),
                        expected,
                        "{x},{y}"
                    );
                }
            }
            assert_eq!(buffer[(6, 3)].symbol(), "X");
            assert_eq!(buffer[(6, 3)].fg, Color::Red);
            assert_eq!(buffer[(7, 3)].modifier, Modifier::BOLD);
        }
    }

    #[test]
    fn click_alone_does_not_highlight_and_drag_is_clamped_to_viewport() {
        use ratatui::layout::Rect;
        let area = Rect::new(3, 2, 5, 3);
        assert!(TextSelection::start(area, click(2, 2)).is_none());
        assert!(TextSelection::start(Rect::new(0, 0, 0, 0), click(0, 0)).is_none());
        let mut selection = TextSelection::start(area, click(3, 2)).unwrap();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 7));
        selection.highlight(&mut buffer);
        assert!(buffer.content.iter().all(|cell| cell.modifier.is_empty()));
        selection.update(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            ..click(u16::MAX, u16::MAX)
        });
        selection.highlight(&mut buffer);
        for y in 0..7 {
            for x in 0..12 {
                assert_eq!(
                    buffer[(x, y)].modifier.contains(Modifier::REVERSED),
                    area.contains((x, y).into())
                );
            }
        }
        // Drawing into a cropped destination must also stay in bounds.
        selection.highlight(&mut Buffer::empty(Rect::new(5, 3, 1, 1)));
    }

    #[test]
    fn selects_nested_panes_including_borders_and_shared_edges() {
        let mut focused = Some(PaneId(1));
        for (x, y, id) in [
            (72, 0, 2),
            (121, 19, 2),
            (72, 20, 3),
            (121, 39, 3),
            (71, 20, 1),
            (22, 0, 1),
        ] {
            assert!(focus_clicked_pane(
                click(x, y),
                &tree(),
                area(),
                &mut focused
            ));
            assert_eq!(focused, Some(PaneId(id)));
        }
    }

    #[test]
    fn ignores_sidebar_footer_and_positions_outside_layout() {
        let mut focused = Some(PaneId(2));
        for (x, y) in [(0, 0), (21, 5), (30, 40), (122, 0), (u16::MAX, u16::MAX)] {
            assert!(!focus_clicked_pane(
                click(x, y),
                &tree(),
                area(),
                &mut focused
            ));
            assert_eq!(focused, Some(PaneId(2)));
        }
    }

    #[test]
    fn ignores_hover_drag_release_wheel_and_other_buttons() {
        let mut focused = Some(PaneId(1));
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::ScrollDown,
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
        ] {
            let mut event = click(80, 30);
            event.kind = kind;
            assert!(!focus_clicked_pane(event, &tree(), area(), &mut focused));
            assert_eq!(focused, Some(PaneId(1)));
        }
    }

    #[test]
    fn uses_current_layout_after_resize_and_pane_removal() {
        let mut focused = None;
        let resized = Rect {
            width: 40,
            height: 10,
            ..area()
        };
        assert!(focus_clicked_pane(
            click(45, 8),
            &tree(),
            resized,
            &mut focused
        ));
        assert_eq!(focused, Some(PaneId(3)));
        let root = SplitNode::Leaf(PaneId(1));
        assert!(focus_clicked_pane(
            click(45, 8),
            &root,
            resized,
            &mut focused
        ));
        assert_eq!(focused, Some(PaneId(1)));
        assert!(!focus_clicked_pane(
            click(80, 30),
            &root,
            resized,
            &mut focused
        ));
    }

    #[test]
    fn zero_sized_panes_cannot_capture_clicks() {
        let mut focused = None;
        for area in [
            Rect { width: 0, ..area() },
            Rect {
                height: 0,
                ..area()
            },
        ] {
            assert!(!focus_clicked_pane(
                click(22, 0),
                &tree(),
                area,
                &mut focused
            ));
            assert_eq!(focused, None);
        }
    }
}
