use ferrux_core::{
    domain::workspace::SplitNode,
    layout::split_tree::{PaneRect, Rect as CoreRect},
};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy)]
pub struct ShellLayout {
    pub header: Rect,
    pub divider: Rect,
    pub sidebar: Rect,
    pub panes: CoreRect,
    pub status: Rect,
}

pub fn shell(area: Rect, sidebar_visible: bool) -> ShellLayout {
    let header_h = u16::from(area.height >= 4);
    let divider_h = u16::from(area.height >= 6);
    let status_h = u16::from(area.height > 0);
    let top = header_h + divider_h;
    let height = area.height.saturating_sub(top + status_h);
    let sidebar_w = if !sidebar_visible || area.width < 60 || height < 4 {
        0
    } else if area.width >= 100 {
        28
    } else if area.width >= 80 {
        25
    } else {
        20
    };
    ShellLayout {
        header: Rect::new(area.x, area.y, area.width, header_h),
        divider: Rect::new(area.x, area.y + header_h, area.width, divider_h),
        sidebar: Rect::new(area.x, area.y + top, sidebar_w, height),
        panes: CoreRect {
            x: area.x + sidebar_w,
            y: area.y + top,
            width: area.width.saturating_sub(sidebar_w),
            height,
        },
        status: Rect::new(
            area.x,
            area.y + area.height.saturating_sub(1),
            area.width,
            status_h,
        ),
    }
}

/// Split children share exactly one border cell. There are no gutter cells.
pub fn panes(node: &SplitNode, area: CoreRect) -> Vec<PaneRect> {
    use ferrux_core::domain::workspace::SplitDirection;
    match node {
        SplitNode::Leaf(pane) => vec![PaneRect {
            pane: *pane,
            rect: area,
        }],
        SplitNode::Split {
            direction,
            ratio_percent,
            first,
            second,
        } => {
            let ratio = u32::from((*ratio_percent).min(100));
            let (a, b) = match direction {
                SplitDirection::Vertical => {
                    let cut = ((u32::from(area.width) * ratio / 100) as u16)
                        .min(area.width.saturating_sub(1));
                    (
                        CoreRect {
                            width: (cut + u16::from(area.width > 0)).min(area.width),
                            ..area
                        },
                        CoreRect {
                            x: area.x.saturating_add(cut),
                            width: area.width.saturating_sub(cut),
                            ..area
                        },
                    )
                }
                SplitDirection::Horizontal => {
                    let cut = ((u32::from(area.height) * ratio / 100) as u16)
                        .min(area.height.saturating_sub(1));
                    (
                        CoreRect {
                            height: (cut + u16::from(area.height > 0)).min(area.height),
                            ..area
                        },
                        CoreRect {
                            y: area.y.saturating_add(cut),
                            height: area.height.saturating_sub(cut),
                            ..area
                        },
                    )
                }
            };
            let mut output = panes(first, a);
            output.extend(panes(second, b));
            output
        }
    }
}

pub fn rect(area: CoreRect) -> Rect {
    Rect::new(area.x, area.y, area.width, area.height)
}
pub fn viewport(area: CoreRect) -> Rect {
    rect(area).inner(ratatui::layout::Margin::new(1, 1))
}
pub fn pty_size(area: CoreRect) -> Option<(u16, u16)> {
    let inner = viewport(area);
    (inner.width > 0 && inner.height > 0).then_some((inner.width, inner.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrux_core::domain::workspace::{PaneId, SplitDirection};
    #[test]
    fn responsive_shell_and_shared_borders_have_no_gaps() {
        let root = SplitNode::Split {
            direction: SplitDirection::Vertical,
            ratio_percent: 50,
            first: Box::new(SplitNode::Leaf(PaneId(1))),
            second: Box::new(SplitNode::Leaf(PaneId(2))),
        };
        for (w, h) in [
            (80, 24),
            (100, 30),
            (120, 40),
            (160, 48),
            (40, 8),
            (1, 1),
            (0, 0),
        ] {
            let layout = shell(Rect::new(0, 0, w, h), true);
            assert_eq!(layout.sidebar.height, layout.panes.height);
            let regions = panes(&root, layout.panes);
            if layout.panes.width > 0 {
                assert_eq!(
                    regions[0].rect.x + regions[0].rect.width - 1,
                    regions[1].rect.x
                );
            }
            for row in layout.panes.y..layout.panes.y + layout.panes.height {
                for col in layout.panes.x..layout.panes.x + layout.panes.width {
                    assert!(regions.iter().any(|p| col >= p.rect.x
                        && col < p.rect.x + p.rect.width
                        && row >= p.rect.y
                        && row < p.rect.y + p.rect.height));
                }
            }
            for pane in regions {
                if let Some((cols, rows)) = pty_size(pane.rect) {
                    assert!(cols > 0 && rows > 0);
                }
            }
        }
    }
}
