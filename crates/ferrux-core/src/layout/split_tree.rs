use crate::domain::workspace::{PaneId, SplitDirection, SplitNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRect {
    pub pane: PaneId,
    pub rect: Rect,
}

/// Walks a `SplitNode` tree and computes the screen rect for every leaf pane.
pub fn layout(node: &SplitNode, area: Rect) -> Vec<PaneRect> {
    match node {
        SplitNode::Leaf(pane) => vec![PaneRect {
            pane: pane.clone(),
            rect: area,
        }],
        SplitNode::Split {
            direction,
            ratio_percent,
            first,
            second,
        } => {
            let (first_area, second_area) = split_rect(area, *direction, *ratio_percent);
            let mut rects = layout(first, first_area);
            rects.extend(layout(second, second_area));
            rects
        }
    }
}

/// Collects the `PaneId` of every leaf, in tree order.
pub fn leaves(node: &SplitNode) -> Vec<PaneId> {
    match node {
        SplitNode::Leaf(pane) => vec![*pane],
        SplitNode::Split { first, second, .. } => {
            let mut ids = leaves(first);
            ids.extend(leaves(second));
            ids
        }
    }
}

/// Replaces the `target` leaf with a split holding `target` (as `first`)
/// and `new_pane` (as `second`). Returns `None` if `target` isn't in the
/// tree.
pub fn split_leaf(
    node: &SplitNode,
    target: PaneId,
    new_pane: PaneId,
    direction: SplitDirection,
    ratio_percent: u8,
) -> Option<SplitNode> {
    match node {
        SplitNode::Leaf(pane) if *pane == target => Some(SplitNode::Split {
            direction,
            ratio_percent,
            first: Box::new(SplitNode::Leaf(target)),
            second: Box::new(SplitNode::Leaf(new_pane)),
        }),
        SplitNode::Leaf(_) => None,
        SplitNode::Split {
            direction: d,
            ratio_percent: r,
            first,
            second,
        } => {
            if let Some(replaced) = split_leaf(first, target, new_pane, direction, ratio_percent) {
                return Some(SplitNode::Split {
                    direction: *d,
                    ratio_percent: *r,
                    first: Box::new(replaced),
                    second: second.clone(),
                });
            }
            split_leaf(second, target, new_pane, direction, ratio_percent).map(|replaced| {
                SplitNode::Split {
                    direction: *d,
                    ratio_percent: *r,
                    first: first.clone(),
                    second: Box::new(replaced),
                }
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// `target` was the only leaf; the tree has nothing left in it.
    Empty,
    /// `target` was removed; the remaining tree replaces the parent split
    /// with whichever sibling subtree survived.
    Removed(SplitNode),
    NotFound,
}

/// Removes the `target` leaf from the tree.
pub fn remove_leaf(node: &SplitNode, target: PaneId) -> RemoveOutcome {
    match node {
        SplitNode::Leaf(pane) if *pane == target => RemoveOutcome::Empty,
        SplitNode::Leaf(_) => RemoveOutcome::NotFound,
        SplitNode::Split {
            direction,
            ratio_percent,
            first,
            second,
        } => match remove_leaf(first, target) {
            RemoveOutcome::Empty => RemoveOutcome::Removed((**second).clone()),
            RemoveOutcome::Removed(replaced) => RemoveOutcome::Removed(SplitNode::Split {
                direction: *direction,
                ratio_percent: *ratio_percent,
                first: Box::new(replaced),
                second: second.clone(),
            }),
            RemoveOutcome::NotFound => match remove_leaf(second, target) {
                RemoveOutcome::Empty => RemoveOutcome::Removed((**first).clone()),
                RemoveOutcome::Removed(replaced) => RemoveOutcome::Removed(SplitNode::Split {
                    direction: *direction,
                    ratio_percent: *ratio_percent,
                    first: first.clone(),
                    second: Box::new(replaced),
                }),
                RemoveOutcome::NotFound => RemoveOutcome::NotFound,
            },
        },
    }
}

fn split_rect(area: Rect, direction: SplitDirection, ratio_percent: u8) -> (Rect, Rect) {
    let ratio = ratio_percent.min(100) as u32;

    match direction {
        SplitDirection::Horizontal => {
            let first_height = ((area.height as u32 * ratio) / 100) as u16;
            let first = Rect {
                height: first_height,
                ..area
            };
            let second = Rect {
                y: area.y + first_height,
                height: area.height.saturating_sub(first_height),
                ..area
            };
            (first, second)
        }
        SplitDirection::Vertical => {
            let first_width = ((area.width as u32 * ratio) / 100) as u16;
            let first = Rect {
                width: first_width,
                ..area
            };
            let second = Rect {
                x: area.x + first_width,
                width: area.width.saturating_sub(first_width),
                ..area
            };
            (first, second)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 40,
        }
    }

    #[test]
    fn leaf_fills_whole_area() {
        let node = SplitNode::Leaf(PaneId(1));
        let rects = layout(&node, area());

        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].rect, area());
    }

    #[test]
    fn vertical_split_divides_width() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        };
        let rects = layout(&node, area());

        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].rect.width, 50);
        assert_eq!(rects[1].rect.width, 50);
        assert_eq!(rects[1].rect.x, 50);
    }

    #[test]
    fn horizontal_split_divides_height() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            ratio_percent: 25,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        };
        let rects = layout(&node, area());

        assert_eq!(rects[0].rect.height, 10);
        assert_eq!(rects[1].rect.height, 30);
        assert_eq!(rects[1].rect.y, 10);
    }

    #[test]
    fn nested_splits_produce_all_leaves() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Split {
                direction: SplitDirection::Horizontal,
                ratio_percent: 50,
                first: Box::new(SplitNode::Leaf(PaneId(2))),
                second: Box::new(SplitNode::Leaf(PaneId(3))),
            }),
        };
        let rects = layout(&node, area());
        assert_eq!(rects.len(), 3);
    }

    #[test]
    fn leaves_lists_ids_in_tree_order() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        };
        assert_eq!(leaves(&node), vec![PaneId(1), PaneId(2)]);
    }

    #[test]
    fn split_leaf_replaces_target_with_a_split() {
        let node = SplitNode::Leaf(PaneId(1));
        let result =
            split_leaf(&node, PaneId(1), PaneId(2), SplitDirection::Vertical, 50).unwrap();

        assert_eq!(leaves(&result), vec![PaneId(1), PaneId(2)]);
        match result {
            SplitNode::Split { direction, .. } => assert_eq!(direction, SplitDirection::Vertical),
            _ => panic!("expected a split"),
        }
    }

    #[test]
    fn split_leaf_finds_target_nested_in_tree() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        };
        let result =
            split_leaf(&node, PaneId(2), PaneId(3), SplitDirection::Horizontal, 50).unwrap();

        assert_eq!(leaves(&result), vec![PaneId(1), PaneId(2), PaneId(3)]);
    }

    #[test]
    fn split_leaf_returns_none_when_target_missing() {
        let node = SplitNode::Leaf(PaneId(1));
        assert!(split_leaf(&node, PaneId(99), PaneId(2), SplitDirection::Vertical, 50).is_none());
    }

    #[test]
    fn remove_leaf_of_single_pane_tree_is_empty() {
        let node = SplitNode::Leaf(PaneId(1));
        assert_eq!(remove_leaf(&node, PaneId(1)), RemoveOutcome::Empty);
    }

    #[test]
    fn remove_leaf_collapses_split_to_sibling() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        };
        assert_eq!(
            remove_leaf(&node, PaneId(1)),
            RemoveOutcome::Removed(SplitNode::Leaf(PaneId(2)))
        );
        assert_eq!(
            remove_leaf(&node, PaneId(2)),
            RemoveOutcome::Removed(SplitNode::Leaf(PaneId(1)))
        );
    }

    #[test]
    fn remove_leaf_not_found_leaves_tree_untouched() {
        let node = SplitNode::Leaf(PaneId(1));
        assert_eq!(remove_leaf(&node, PaneId(99)), RemoveOutcome::NotFound);
    }

    #[test]
    fn remove_leaf_nested_preserves_remaining_shape() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Split {
                direction: SplitDirection::Horizontal,
                ratio_percent: 50,
                first: Box::new(SplitNode::Leaf(PaneId(2))),
                second: Box::new(SplitNode::Leaf(PaneId(3))),
            }),
        };
        let RemoveOutcome::Removed(result) = remove_leaf(&node, PaneId(2)) else {
            panic!("expected Removed");
        };
        assert_eq!(leaves(&result), vec![PaneId(1), PaneId(3)]);
    }
}
