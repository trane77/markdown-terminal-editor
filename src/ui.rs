//! Disegno dello schermo: editor, anteprima sincronizzata, scrollbar, barra di stato.

use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use ratatui_image::Image;

use crate::{i18n::tr, markdown::render_markdown, next_scroll_top, App, View};

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
        // sempre sul bordo destro dell'editor: nello split affiancato è il bordo in mezzo ai due riquadri
        scrollbar(f, area, app.editor.lines().len(), app.editor_top, app.editor_h);
    }

    if let Some(area) = preview_area {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", tr().preview));
        let inner = block.inner(area);
        app.preview_inner = Some(inner);
        f.render_widget(block, area);
        // ponytail: ri-parsa tutto il documento a ogni frame; cache sul testo se diventa lento con file enormi
        app.images.max_rows = (inner.height / 2).max(3);
        let pv = render_markdown(&app.editor.lines().join("\n"), inner.width, Some(&mut app.images));
        let mut pad = 0;
        if app.view == View::Split {
            // la riga d'anteprima della riga col cursore va alla stessa altezza dello schermo del cursore;
            // se sopra non c'è abbastanza contenuto, si sposta l'anteprima in basso
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
                (rect, l.dest.clone(), l.target.map(|t| (t, pv.row_for(t))))
            })
            .collect();
        let lines = pv
            .lines
            .into_iter()
            .skip(app.preview_scroll)
            .take(content.height as usize)
            .collect::<Vec<_>>();
        f.render_widget(Paragraph::new(lines), content);
        // immagini sopra le righe riservate; di quelle tagliate dallo scorrimento si disegna solo la parte visibile
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
        // nello split affiancato basta la scrollbar dell'editor in mezzo: i due riquadri scorrono insieme
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
}

/// Scrollbar disegnata sopra il bordo destro di `area` (riquadro con bordi).
fn scrollbar(f: &mut Frame, area: Rect, total: usize, top: usize, view: usize) {
    // ratatui vuole il numero di posizioni di scorrimento: con `top` all'ultima la barra tocca il fondo
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
