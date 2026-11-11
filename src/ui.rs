//! Screen drawing: editor, synced preview, scrollbars, status bar.

use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};
use ratatui_image::Image;

use crate::{i18n::tr, markdown::render_markdown, next_scroll_top, App, Dialog, ScreenLink, View};

pub(crate) fn draw(f: &mut Frame, app: &mut App) {
    let [main, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
    let (editor_area, preview_area) = match app.view {
        View::Editor => (Some(main), None),
        View::Preview => (None, Some(main)),
        View::Split => {
            let dir = if app.side_by_side {
                Direction::Horizontal
            } else {
                Direction::Vertical
            };
            let [a, b] = Layout::new(dir, [Constraint::Percentage(50); 2]).areas(main);
            (Some(a), Some(b))
        }
    };

    let cursor_row = app.editor.cursor().0;
    app.preview_inner = None;
    app.links.clear();
    if let Some(area) = editor_area {
        app.editor_inner = area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        app.editor_h = area.height.saturating_sub(2).max(1) as usize;
        app.editor_top = next_scroll_top(app.editor_top, cursor_row, app.editor_h);
        app.editor_left = next_scroll_top(
            app.editor_left,
            app.editor.cursor().1,
            app.editor_inner.width.max(1) as usize,
        );
        f.render_widget(&app.editor, area);
        // always on the editor's right border: in the side-by-side split it is the border between the panes
        scrollbar(f, area, app.editor.lines().len(), app.editor_top, app.editor_h);
    }

    if let Some(area) = preview_area {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", tr().preview));
        let inner = block.inner(area);
        app.preview_inner = Some(inner);
        f.render_widget(block, area);
        // ponytail: re-parses the whole document every frame; cache on the text if huge files get slow
        app.images.max_rows = (inner.height / 2).max(3);
        let pv = render_markdown(&app.editor.lines().join("\n"), inner.width, Some(&mut app.images));
        let mut pad = 0;
        if app.view == View::Split {
            // the preview row of the cursor's line goes at the cursor's screen height;
            // if there is not enough content above, the preview is shifted down
            let target = (cursor_row - app.editor_top).min((inner.height as usize).saturating_sub(1));
            let row = pv.row_for(cursor_row);
            app.preview_scroll = row.saturating_sub(target);
            pad = target.saturating_sub(row);
            f.buffer_mut()[(area.x, inner.y + target as u16)].set_symbol("▶");
        } else {
            app.preview_scroll = app
                .preview_scroll
                .min(pv.lines.len().saturating_sub(inner.height as usize));
        }
        let content = Rect {
            y: inner.y + pad as u16,
            height: inner.height - pad as u16,
            ..inner
        };
        let total = pv.lines.len();
        let (top, h) = (app.preview_scroll, content.height as usize);
        app.links = pv
            .links
            .iter()
            .filter(|l| (top..top + h).contains(&l.line))
            .map(|l| {
                let rect = Rect::new(content.x + l.x, content.y + (l.line - top) as u16, l.width, 1);
                ScreenLink {
                    rect,
                    id: l.id,
                    dest: l.dest.clone(),
                    target: l.target.map(|t| (t, pv.row_for(t))),
                }
            })
            .collect();
        let lines = pv
            .lines
            .into_iter()
            .skip(app.preview_scroll)
            .take(content.height as usize)
            .collect::<Vec<_>>();
        f.render_widget(Paragraph::new(lines), content);
        // link under the mouse, in all its pieces: light blue if internal, magenta if it opens the browser
        if let Some(hl) = app.hover.and_then(|h| app.links.iter().find(|l| l.rect.contains(h))) {
            let color = if hl.dest.starts_with('#') {
                Color::LightBlue
            } else {
                Color::Magenta
            };
            for l in app.links.iter().filter(|l| l.id == hl.id) {
                f.buffer_mut().set_style(l.rect, Style::new().fg(color));
            }
        }
        // images over their reserved rows; for those cut by scrolling, only the visible part is drawn
        for slot in &pv.images {
            let (first, last) = (slot.line, slot.line + slot.rows as usize);
            let (from, to) = (first.max(top), last.min(top + h));
            if from >= to {
                continue;
            }
            let rect = Rect {
                x: content.x + slot.x,
                y: content.y + (from - top) as u16,
                width: slot.cols,
                height: (to - from) as u16,
            };
            if let Some(p) = app.images.protocol(slot, (from - first) as u16, (to - from) as u16) {
                f.render_widget(Image::new(p), rect);
            }
        }
        // in the side-by-side split the editor's scrollbar in the middle is enough: both panes scroll together
        if !(app.view == View::Split && app.side_by_side) {
            scrollbar(f, area, total, app.preview_scroll, inner.height as usize);
        }
    }

    let t = tr();
    let view = match app.view {
        View::Editor => t.editor.to_lowercase(),
        View::Preview => format!("{} ({})", t.preview.to_lowercase(), t.preview_keys),
        View::Split if app.side_by_side => t.split_side.into(),
        View::Split => t.split_stacked.into(),
    };
    let text = format!(
        " {}{} | {view} | img: {:?} | {}  {}",
        app.path,
        if app.dirty { " [+]" } else { "" },
        app.images.picker.protocol_type(),
        t.keys,
        app.status
    );
    let pos = format!(
        " {} {}/{} · {} ",
        t.line,
        cursor_row + 1,
        app.editor.lines().len(),
        if app.eol == "\r\n" { "CRLF" } else { "LF" }
    );
    let [left, right] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(pos.chars().count() as u16)]).areas(status);
    let bar = Style::new().add_modifier(Modifier::REVERSED);
    f.render_widget(Paragraph::new(text).style(bar), left);
    f.render_widget(Paragraph::new(pos).style(bar), right);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    match &app.dialog {
        Some(Dialog::OpenLink(url)) => confirm_dialog(
            f,
            " Link ",
            vec![
                Line::from(t.open_link),
                Line::default(),
                Line::styled(url.as_str(), Style::new().fg(Color::Magenta)),
                Line::default(),
                Line::from(format!("Enter {}   Esc {}", t.ok, t.cancel)).style(bold),
            ],
        ),
        Some(Dialog::Reload(_)) => confirm_dialog(
            f,
            &format!(" {} ", app.path),
            vec![
                Line::from(t.changed_on_disk),
                Line::default(),
                Line::from(format!("Enter {}   Esc {}", t.reload, t.keep)).style(bold),
            ],
        ),
        Some(Dialog::Quit) => confirm_dialog(
            f,
            &format!(" {} ", app.path),
            vec![
                Line::from(t.unsaved),
                Line::default(),
                // one option per line: three of them do not fit on one line in most languages
                Line::from(format!("Enter  {}", t.save_quit)).style(bold),
                Line::from(format!("^Q  {}", t.quit_anyway)).style(bold),
                Line::from(format!("Esc  {}", t.cancel)).style(bold),
            ],
        ),
        None => {}
    }
}

/// Scrollbar drawn over the right border of `area` (a bordered pane).
fn scrollbar(f: &mut Frame, area: Rect, total: usize, top: usize, view: usize) {
    // ratatui wants the number of scroll positions: with `top` at the last one the thumb reaches the bottom
    let mut state = ScrollbarState::new(total.saturating_sub(view) + 1)
        .position(top)
        .viewport_content_length(view);
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"));
    f.render_stateful_widget(
        bar,
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut state,
    );
}

/// Centered dialog with a title and some centered, wrapped lines of text.
fn confirm_dialog(f: &mut Frame, title: &str, text: Vec<Line>) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .padding(Padding::horizontal(1));
    let para = Paragraph::new(text).block(block).centered().wrap(Wrap { trim: false });
    // exact height after wrapping the question and the address, borders included
    let w = area.width.saturating_sub(4).min(70);
    let h = (para.line_count(w) as u16).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, rect);
    f.render_widget(para, rect);
}
