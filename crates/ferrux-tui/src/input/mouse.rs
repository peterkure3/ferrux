use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use ferrux_core::domain::workspace::{PaneId, SplitNode};
use ferrux_core::layout::split_tree::Rect;

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
