mod blocks;
mod run_view;
mod thread_view;

pub(super) use run_view::render_detail_view;
pub(super) use thread_view::render_thread_detail_view;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::state::{BG, DIM_WHITE, SEPARATOR, TEAL};

/// Three-way vertical split used by both detail views: header (3) / body (min 3) / status (1).
fn compute_detail_layout(area: Rect) -> [Rect; 3] {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

fn visible_detail_lines(
    lines: Vec<Line<'static>>,
    area: Rect,
    scroll: &mut usize,
    auto_scroll: &mut bool,
    is_live: bool,
    detail_inner_height: &mut usize,
) -> Vec<Line<'static>> {
    let inner_height = area.height.saturating_sub(2) as usize;
    *detail_inner_height = inner_height;
    let max_scroll = lines.len().saturating_sub(inner_height);
    *scroll = (*scroll).min(max_scroll);
    if is_live && *scroll >= max_scroll {
        *auto_scroll = true;
    }
    lines.into_iter().skip(*scroll).take(inner_height).collect()
}

fn render_detail_body(frame: &mut ratatui::Frame, area: Rect, visible_lines: Vec<Line<'static>>) {
    let content = Paragraph::new(visible_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(SEPARATOR)),
    );
    frame.render_widget(content, area);
}

fn push_live_follow_spans(spans: &mut Vec<Span<'static>>, is_live: bool, follow_on: bool) {
    if is_live {
        spans.push(Span::styled("  G", Style::default().fg(TEAL)));
        spans.push(Span::styled(" follow  ", Style::default().fg(DIM_WHITE)));
        if follow_on {
            spans.push(Span::styled(
                " FOLLOW ",
                Style::default()
                    .fg(BG)
                    .bg(TEAL)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::compute_detail_layout;
    use ratatui::layout::Rect;

    #[test]
    fn detail_layout_splits_80x24_into_header_body_status() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [header, body, status] = compute_detail_layout(area);
        assert_eq!(header.height, 3);
        assert_eq!(body.height, 20);
        assert_eq!(status.height, 1);
        assert_eq!(
            header.height + body.height + status.height,
            area.height,
            "chunks should fully cover the area height",
        );
        assert_eq!(header.width, 80);
        assert_eq!(header.y, 0);
        assert_eq!(body.y, 3);
        assert_eq!(status.y, 23);
    }

    #[test]
    fn detail_layout_does_not_panic_on_tight_height() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 4,
        };
        let [_h, _b, _s] = compute_detail_layout(area);
    }
}
