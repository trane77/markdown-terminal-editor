//! Screen drawing: editor, synced preview, scrollbars, status bar.

use std::path::Path;

use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};
use ratatui_image::Image;

use crate::{i18n::tr, input::match_count, markdown::render_markdown, next_scroll_top, App, Dialog, ScreenLink, View};

pub(crate) fn draw(f: &mut Frame, app: &mut App) {
    let [main, hints, status] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(f.area());
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
    // a search jump to reveal in preview-only view; in the other views the editor already follows the cursor
    let reveal = std::mem::take(&mut app.reveal);
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
            // after a search jump, bring the cursor's line into view, a third from the top
            if reveal {
                app.preview_scroll = pv.row_for(cursor_row).saturating_sub(inner.height as usize / 3);
            }
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
    let key = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    // first row: the find bar while searching, otherwise the shortcut keys
    let row: Vec<Span> = if app.searching {
        let (total, current) = match_count(app);
        let count = match (app.query.is_empty(), total) {
            (true, _) => String::new(),
            (false, 0) => format!("  {}", t.no_matches),
            (false, n) => format!("  {current}/{n}"),
        };
        vec![
            Span::styled(format!(" {}: ", t.find), key),
            Span::raw(app.query.as_str()),
            Span::styled("▏", key),
            Span::styled(count, Style::new().fg(Color::DarkGray)),
            Span::styled("   Enter/↓ ", key),
            Span::raw(t.next),
            Span::styled("  ↑ ", key),
            Span::raw(t.previous),
            Span::styled("  Esc ", key),
            Span::raw(t.close),
        ]
    } else {
        // groups: views and find │ changes to the file │ quit; `t.hints` follows the order of KEYS
        const KEYS: [&str; 8] = ["^E", "^P", "^L", "^F", "^S", "^Z", "^Y", "^Q"];
        const GROUPS: [&[usize]; 3] = [&[0, 1, 2, 3], &[5, 6, 4], &[7]];
        let sep = Span::styled("│", Style::new().fg(Color::DarkGray));
        let mut row: Vec<Span> = Vec::new();
        for (g, group) in GROUPS.iter().enumerate() {
            if g > 0 {
                row.push(sep.clone());
            }
            for &i in *group {
                let (k, label) = (KEYS[i], t.hints[i]);
                // nothing to save: the save shortcut is shown dimmed, as disabled
                if k == "^S" && !app.dirty {
                    let off = Style::new().fg(Color::DarkGray);
                    row.push(Span::styled(format!(" {k} "), off));
                    row.push(Span::styled(format!("{label} "), off));
                } else {
                    row.push(Span::styled(format!(" {k} "), key));
                    row.push(Span::raw(format!("{label} ")));
                }
            }
        }
        if app.view == View::Preview {
            row.push(Span::styled(format!(" {}", t.preview_keys), key));
        }
        row
    };
    f.render_widget(Paragraph::new(Line::from(row)), hints);

    // second row: view and file on the left, position and file format on the right
    let view = match app.view {
        View::Editor => t.editor,
        View::Preview => t.preview,
        View::Split if app.side_by_side => t.split_side,
        View::Split => t.split_stacked,
    };
    let lines = app.editor.lines().len();
    let col = app.editor.cursor().1;
    let badge = format!(" {} ", view.to_uppercase());
    let status_msg = if app.status.is_empty() {
        String::new()
    } else {
        format!("  {}", app.status)
    };
    let marker = usize::from(app.dirty);

    // right side: position and line ending always, plus optional details that are dropped one by one,
    // least important first, while the left side lacks room for the view, file name and status
    let position = format!(" {} {}:{}", t.line, cursor_row + 1, col + 1);
    let eol = format!(" · {} ", if app.eol == "\r\n" { "CRLF" } else { "LF" });
    // 0% on the first line, 100% on the last
    let percent = format!(" · {}%", cursor_row * 100 / lines.saturating_sub(1).max(1));
    let total = format!(" · {lines} {}", t.lines);
    let img = format!("img: {:?} ", app.images.picker.protocol_type());
    let mut optional = vec![total, percent];
    let mut show_img = true;
    let file_name = Path::new(&app.path).file_name().map_or(app.path.len(), |n| n.len());
    let left_min = badge.chars().count() + 1 + file_name + marker + status_msg.chars().count();
    let right = loop {
        let right = format!(
            "{position}{}{eol}{}",
            optional.concat(),
            if show_img { format!("· {img}") } else { String::new() }
        );
        if right.chars().count() + left_min <= status.width as usize {
            break right;
        }
        if show_img {
            show_img = false;
        } else if optional.is_empty() {
            break right;
        } else {
            optional.remove(0);
        }
    };
    let [l, r] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right.chars().count() as u16)]).areas(status);
    // the path gives way first, from the left, so the unsaved marker and the status message stay visible
    let room = (l.width as usize).saturating_sub(badge.chars().count() + 1 + marker + status_msg.chars().count());
    let left = Line::from(vec![
        Span::styled(
            badge,
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        // unsaved changes: "!" right after the file name, in the same style
        Span::styled(
            format!(" {}{}", shorten_left(&app.path, room), if app.dirty { "!" } else { "" }),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::raw(status_msg),
    ]);
    let bar = Style::new().bg(Color::DarkGray).fg(Color::White);
    f.render_widget(Paragraph::new(left).style(bar), l);
    f.render_widget(Paragraph::new(right).style(bar), r);
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

/// `text` cut from the left to at most `max` characters, with "…" marking the cut.
fn shorten_left(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    std::iter::once('…').chain(text.chars().skip(n - keep)).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Images;
    use ratatui::{backend::TestBackend, Terminal};
    use ratatui_image::picker::Picker;

    #[test]
    fn status_bar_drops_details_when_narrow() {
        let images = Images::new(Picker::halfblocks(), ".".into());
        let mut app = App::new("docs/notes.md".into(), "# Hi\n\ntext\n".into(), images);
        app.dirty = true;
        let mut bar = |width: u16| {
            let mut t = Terminal::new(TestBackend::new(width, 6)).unwrap();
            t.draw(|f| draw(f, &mut app)).unwrap();
            let b = t.backend().buffer();
            (0..width).map(|x| b[(x, 5)].symbol().to_string()).collect::<String>()
        };
        let wide = bar(120);
        assert!(
            wide.contains("notes.md!") && wide.contains("img: ") && wide.contains("3 lines"),
            "{wide}"
        );
        // image protocol goes first, then the total lines, then the percentage
        let mid = bar(70);
        assert!(
            !mid.contains("img: ") && mid.contains("3 lines") && mid.contains("0%"),
            "{mid}"
        );
        let narrow = bar(45);
        assert!(!narrow.contains("lines") && !narrow.contains('%'), "{narrow}");
        // position, line ending and the file name with its marker always stay
        assert!(
            narrow.contains("Ln 1:1") && narrow.contains("LF") && narrow.contains("notes.md!"),
            "{narrow}"
        );
    }

    #[test]
    fn long_paths_are_cut_from_the_left() {
        assert_eq!(shorten_left("docs/a.md", 20), "docs/a.md");
        assert_eq!(shorten_left("/very/long/path/to/notes.md", 12), "…to/notes.md");
        assert_eq!(shorten_left("notes.md", 0), "…");
    }
}
