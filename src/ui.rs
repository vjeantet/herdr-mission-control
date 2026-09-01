use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use ratatui::Frame;

use crate::{App, Tile};

/// Floor under which tiles stop shrinking (borders included).
const MIN_TILE_W: u16 = 60;
const MIN_TILE_H: u16 = 15;

// Same status semantics as herdr's own UI (src/ui/status.rs).
fn status_style(status: &str) -> (&'static str, Color) {
    match status {
        "blocked" => ("●", Color::Red),
        "working" => ("●", Color::Yellow),
        "done" => ("●", Color::Green),
        "idle" => ("○", Color::DarkGray),
        _ => ("○", Color::DarkGray),
    }
}

enum RowKind {
    Header(usize),
    Tiles {
        section: usize,
        start: usize,
        end: usize,
        columns: usize,
    },
}

struct VisualRow {
    y: u16,
    height: u16,
    kind: RowKind,
}

struct Plan {
    rows: Vec<VisualRow>,
    total_height: u16,
    scrolling: bool,
}

/// Shrink-to-fit, then scroll: tiles share the space evenly; below the
/// floor they keep the floor size behind a scroll instead of shrinking
/// further (a degraded header-only mode was tried and dropped: it wasted
/// most of the screen).
fn plan_rows(app: &App, body: Rect) -> Plan {
    let width = body.width.max(1);
    let mut columns_per_section = Vec::with_capacity(app.sections.len());
    let mut tile_row_count: u16 = 0;
    for section in &app.sections {
        let columns = ((width / MIN_TILE_W).max(1) as usize).min(section.tiles.len().max(1));
        columns_per_section.push(columns);
        tile_row_count += section.tiles.len().div_ceil(columns) as u16;
    }

    let header_count = app.sections.len() as u16;
    let avail = body.height.saturating_sub(header_count);
    let fit_h = if tile_row_count == 0 {
        MIN_TILE_H
    } else {
        avail / tile_row_count
    };

    let (tile_h, mut extra, scrolling) = if fit_h >= MIN_TILE_H {
        (fit_h, avail % tile_row_count.max(1), false)
    } else {
        (MIN_TILE_H, 0, true)
    };

    let mut rows = Vec::new();
    let mut y: u16 = 0;
    for (section_index, section) in app.sections.iter().enumerate() {
        rows.push(VisualRow {
            y,
            height: 1,
            kind: RowKind::Header(section_index),
        });
        y += 1;
        let columns = columns_per_section[section_index];
        let mut start = 0;
        while start < section.tiles.len() {
            let end = (start + columns).min(section.tiles.len());
            let height = tile_h + u16::from(extra > 0);
            extra = extra.saturating_sub(1);
            rows.push(VisualRow {
                y,
                height,
                kind: RowKind::Tiles {
                    section: section_index,
                    start,
                    end,
                    columns,
                },
            });
            y += height;
            start = end;
        }
    }

    Plan {
        rows,
        total_height: y,
        scrolling,
    }
}

/// Scroll into view: the persistent offset moves only when the selected
/// tile row (plus its section header when it sits just above) would leave
/// the viewport. A selection change inside the visible area moves the
/// cursor alone, not the content.
fn ensure_selected_visible(app: &mut App, plan: &Plan, viewport: u16) -> u16 {
    if plan.total_height <= viewport {
        app.scroll = 0;
        return 0;
    }
    let mut offset = app.scroll.min(plan.total_height - viewport);
    if !app.follow_selection {
        app.scroll = offset;
        return offset;
    }
    let selected = plan.rows.iter().position(|row| {
        matches!(
            row.kind,
            RowKind::Tiles { section, start, end, .. }
                if section == app.selected.0 && (start..end).contains(&app.selected.1)
        )
    });
    if let Some(index) = selected {
        let row = &plan.rows[index];
        let top = match index.checked_sub(1).map(|i| &plan.rows[i]) {
            Some(prev) if matches!(prev.kind, RowKind::Header(s) if s == app.selected.0) => prev.y,
            _ => row.y,
        };
        let bottom = row.y + row.height;
        if bottom > offset + viewport {
            offset = bottom - viewport;
        }
        if top < offset {
            offset = top;
        }
    }
    app.scroll = offset;
    offset
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [body, footer] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

    let plan = plan_rows(app, body);
    let offset = ensure_selected_visible(app, &plan, body.height);
    app.tile_rects.clear();

    // Rows are rendered on a virtual canvas, then the viewport is blitted
    // onto the frame: partially visible tiles scroll line by line instead
    // of popping in and out whole.
    let mut scratch = Buffer::empty(Rect {
        x: 0,
        y: 0,
        width: body.width,
        height: plan.total_height.max(1),
    });

    let mut badge_base = 0usize;
    for row in &plan.rows {
        let badge_start = badge_base;
        if let RowKind::Tiles { start, end, .. } = row.kind {
            badge_base += end - start;
        }
        if row.y + row.height <= offset || row.y >= offset + body.height {
            continue;
        }
        let area = Rect {
            x: 0,
            y: row.y,
            width: body.width,
            height: row.height,
        };
        match row.kind {
            RowKind::Header(section_index) => {
                draw_header(app, section_index, area, &mut scratch)
            }
            RowKind::Tiles {
                section,
                start,
                end,
                columns,
            } => {
                let cells = Layout::horizontal(vec![Constraint::Fill(1); columns]).split(area);
                for (offset_in_row, tile_index) in (start..end).enumerate() {
                    let cell = cells[offset_in_row];
                    // Visible slice of the cell in screen coordinates, for
                    // mouse hit-testing.
                    let top = cell.y.max(offset);
                    let bottom = (cell.y + cell.height).min(offset + body.height);
                    if bottom > top {
                        app.tile_rects.push((
                            Rect {
                                x: body.x + cell.x,
                                y: body.y + top - offset,
                                width: cell.width,
                                height: bottom - top,
                            },
                            (section, tile_index),
                        ));
                    }
                    let tile = &app.sections[section].tiles[tile_index];
                    let selected = app.selected == (section, tile_index);
                    draw_tile(
                        tile,
                        badge_start + offset_in_row + 1,
                        selected,
                        cell,
                        &mut scratch,
                    );
                }
            }
        }
    }

    let frame_buf = frame.buffer_mut();
    for dy in 0..body.height {
        let sy = offset + dy;
        if sy >= plan.total_height {
            break;
        }
        for dx in 0..body.width {
            if let (Some(dst), Some(src)) = (
                frame_buf.cell_mut((body.x + dx, body.y + dy)),
                scratch.cell((dx, sy)),
            ) {
                *dst = src.clone();
            }
        }
    }

    let mut help = vec![
        Span::styled(" ←↓↑→/hjkl ", Style::default().fg(Color::Cyan)),
        Span::raw("navigate  "),
        Span::styled("⏎", Style::default().fg(Color::Cyan)),
        Span::raw(" switch  "),
        Span::styled("1-9", Style::default().fg(Color::Cyan)),
        Span::raw(" jump  "),
        Span::styled("⌫", Style::default().fg(Color::Cyan)),
        Span::raw(" close pane  "),
        Span::styled("esc", Style::default().fg(Color::Cyan)),
        Span::raw(" quit"),
    ];
    if plan.scrolling {
        help.push(Span::styled(
            "  ‹scrolling›",
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(help)), footer);

    if app.confirming_close {
        draw_confirm_close(frame, app);
    }
}

fn draw_confirm_close(frame: &mut Frame, app: &App) {
    let Some(tile) = app
        .sections
        .get(app.selected.0)
        .and_then(|s| s.tiles.get(app.selected.1))
    else {
        return;
    };
    let screen = frame.area();
    let width = 50.min(screen.width.saturating_sub(4)).max(20);
    let area = Rect {
        x: screen.x + (screen.width.saturating_sub(width)) / 2,
        y: screen.y + (screen.height.saturating_sub(5)) / 2,
        width,
        height: 5.min(screen.height),
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(Line::from(Span::styled(
            " Close this pane? ",
            Style::default().add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let body = vec![
        Line::from(vec![
            Span::styled(
                tile.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", tile.pane_id),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("⏎/y", Style::default().fg(Color::Red)),
            Span::raw(" close   "),
            Span::styled("esc/n", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel"),
        ]),
    ];
    frame.render_widget(Paragraph::new(body), inner);
}

fn draw_header(app: &App, section_index: usize, area: Rect, buf: &mut Buffer) {
    let section = &app.sections[section_index];
    let title = Line::from(vec![
        Span::styled(
            format!(" t{} ", section.number),
            Style::default()
                .fg(Color::Black)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", section.label),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if section.zoomed { "  [zoom]" } else { "" },
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    Paragraph::new(title).render(area, buf);
}

fn draw_tile(tile: &Tile, badge: usize, selected: bool, area: Rect, buf: &mut Buffer) {
    let (icon, color) = status_style(&tile.status);
    let border_style = if selected {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut title_spans = vec![
        Span::styled(format!(" {icon} "), Style::default().fg(color)),
        Span::styled(
            tile.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(agent) = &tile.agent {
        title_spans.push(Span::styled(
            format!(" [{agent}]"),
            Style::default().fg(Color::Magenta),
        ));
    }
    if tile.title != tile.pane_id {
        title_spans.push(Span::styled(
            format!(" {} ", tile.pane_id),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        title_spans.push(Span::raw(" "));
    }
    if badge <= 9 {
        title_spans.push(Span::styled(
            format!("({badge}) "),
            Style::default().fg(Color::Cyan),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Line::from(title_spans));

    let inner = block.inner(area);
    block.render(area, buf);
    if inner.height == 0 {
        return;
    }

    // Show the tail of the preview: the bottom of a terminal is where the
    // action is. Trailing blank lines are excluded first, otherwise a pane
    // whose content sits at the top of a mostly empty screen (editor, fresh
    // process) renders as an empty tile.
    let end = tile
        .preview
        .iter()
        .rposition(|line| line.spans.iter().any(|s| !s.content.trim().is_empty()))
        .map_or(0, |i| i + 1);
    let tail_start = end.saturating_sub(inner.height as usize);
    let text: Vec<Line> = tile.preview[tail_start..end].to_vec();
    Paragraph::new(text).render(inner, buf);
}
