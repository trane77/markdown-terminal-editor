//! Keyboard and mouse: commands, selection confined to the editor, links, clipboard.

use std::{
    env,
    io::{self, Write},
    process::{Command, Stdio},
    time::Duration,
};

use ratatui::{
    crossterm::event::{
        self, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    layout::Position,
    DefaultTerminal,
};
use tui_textarea::CursorMove;

use crate::{draw, i18n::tr, App, Dialog, View};

pub(crate) fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    let mut redraw = true;
    loop {
        if redraw {
            terminal.draw(|f| draw(f, app))?;
        }
        redraw = true;
        // with no input for a while, check whether another program changed the file
        if !event::poll(Duration::from_millis(500))? {
            redraw = app.check_disk();
            continue;
        }
        let key = match event::read()? {
            TermEvent::Key(key) => key,
            TermEvent::Mouse(m) => {
                on_mouse(app, m);
                continue;
            }
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if let Some(dialog) = app.dialog.take() {
            if dialog_key(app, dialog, key.code, ctrl) {
                return Ok(());
            }
            continue;
        }
        if app.searching {
            search_key(app, key.code, ctrl);
            continue;
        }
        app.status.clear();

        match (ctrl, key.code) {
            (true, KeyCode::Char('q')) => {
                if !app.dirty {
                    return Ok(());
                }
                app.dialog = Some(Dialog::Quit);
            }
            (true, KeyCode::Char('s')) => app.save(),
            (true, KeyCode::Char('f')) => {
                // reopen with the last query, already highlighted
                app.searching = true;
                set_query(app);
            }
            // the usual undo/redo/paste keys; tui-textarea's own Ctrl+U / Ctrl+R still work too
            (true, KeyCode::Char('z')) => {
                if app.editor.undo() {
                    app.edited();
                }
            }
            (true, KeyCode::Char('y')) => {
                if app.editor.redo() {
                    app.edited();
                }
            }
            (true, KeyCode::Char('v')) => {
                if app.editor.paste() {
                    app.edited();
                }
            }
            (true, KeyCode::Char('e')) => app.view = View::Editor,
            (true, KeyCode::Char('p')) => app.view = View::Preview,
            (true, KeyCode::Char('l')) => {
                // Ctrl+L enters split view; if already split, toggles side-by-side / stacked
                if app.view == View::Split {
                    app.side_by_side = !app.side_by_side;
                }
                app.view = View::Split;
            }
            (_, code) if app.view == View::Preview => match code {
                KeyCode::Up => app.preview_scroll = app.preview_scroll.saturating_sub(1),
                KeyCode::Down => app.preview_scroll = app.preview_scroll.saturating_add(1),
                KeyCode::PageUp => app.preview_scroll = app.preview_scroll.saturating_sub(10),
                KeyCode::PageDown => app.preview_scroll = app.preview_scroll.saturating_add(10),
                KeyCode::Home => app.preview_scroll = 0,
                _ => {}
            },
            // tui-textarea would scroll the view without moving the cursor, making `editor_top`
            // impossible to recompute: move the cursor by one page instead
            (_, KeyCode::PageDown | KeyCode::PageUp) | (false, KeyCode::Char('v'))
                if key.code != KeyCode::Char('v') || alt =>
            {
                let down = key.code == KeyCode::PageDown;
                for _ in 0..app.editor_h {
                    app.editor
                        .move_cursor(if down { CursorMove::Down } else { CursorMove::Up });
                }
            }
            _ => {
                if app.editor.input(key) {
                    app.edited();
                }
                // Ctrl+C / Ctrl+X: besides the editor's internal clipboard, also the system one
                if ctrl && matches!(key.code, KeyCode::Char('c' | 'x')) {
                    copy_to_clipboard(&app.editor.yank_text());
                }
            }
        }
    }
}

/// Literal, "smart case" search pattern: case-insensitive unless the query has an uppercase letter.
fn search_pattern(query: &str) -> String {
    let case = if query.chars().any(char::is_uppercase) {
        ""
    } else {
        "(?i)"
    };
    format!("{case}{}", regex::escape(query))
}

/// Highlights the matches of the current query and moves to the first one from the cursor on.
fn set_query(app: &mut App) {
    let pattern = if app.query.is_empty() {
        String::new()
    } else {
        search_pattern(&app.query)
    };
    // an escaped query is always a valid pattern
    let _ = app.editor.set_search_pattern(pattern);
    if !app.query.is_empty() {
        app.editor.search_forward(true);
        app.reveal = true;
    }
}

/// Key pressed while the find bar is open: typing edits the query, Enter/↓/Ctrl+F go to the next
/// match, ↑ to the previous one, Esc closes the bar and clears the highlight.
fn search_key(app: &mut App, code: KeyCode, ctrl: bool) {
    match code {
        KeyCode::Esc => {
            app.searching = false;
            let _ = app.editor.set_search_pattern("");
        }
        KeyCode::Enter | KeyCode::Down => app.reveal |= app.editor.search_forward(false),
        KeyCode::Char('f') if ctrl => app.reveal |= app.editor.search_forward(false),
        KeyCode::Up => app.reveal |= app.editor.search_back(false),
        KeyCode::Backspace => {
            app.query.pop();
            set_query(app);
        }
        KeyCode::Char(c) if !ctrl => {
            app.query.push(c);
            set_query(app);
        }
        _ => {}
    }
}

/// Number of matches of the current search, and the 1-based index of the one at the cursor (0 if
/// the cursor is not on a match).
pub(crate) fn match_count(app: &App) -> (usize, usize) {
    let Some(re) = app.editor.search_pattern() else {
        return (0, 0);
    };
    let (row, col) = app.editor.cursor();
    let (mut total, mut current) = (0, 0);
    for (r, line) in app.editor.lines().iter().enumerate() {
        for m in re.find_iter(line) {
            total += 1;
            if r == row && line[..m.start()].chars().count() == col {
                current = total;
            }
        }
    }
    (total, current)
}

/// Key pressed while a dialog is open: only Enter (confirm) or Esc (cancel) count, plus Ctrl+Q when
/// quitting; any other key leaves the dialog open. Returns true when the app must quit.
fn dialog_key(app: &mut App, dialog: Dialog, code: KeyCode, ctrl: bool) -> bool {
    match (dialog, code) {
        (Dialog::Quit, KeyCode::Enter) => {
            app.save();
            // quit only if the save went through: not on a write error or an external change
            return !app.dirty && app.dialog.is_none();
        }
        (Dialog::Quit, KeyCode::Char('q')) if ctrl => return true,
        (Dialog::Quit, KeyCode::Esc) => {}
        (Dialog::OpenLink(url), KeyCode::Enter) => open_url(app, &url),
        (Dialog::OpenLink(_), KeyCode::Esc) => {}
        (Dialog::Reload(text), KeyCode::Enter) => app.reload(text),
        (Dialog::Reload(text), KeyCode::Esc) => app.keep_mine(text),
        (dialog, _) => app.dialog = Some(dialog),
    }
    false
}

fn on_mouse(app: &mut App, m: MouseEvent) {
    let pos = Position::new(m.column, m.row);
    let r = app.editor_inner;
    let in_panes = r.contains(pos) || app.preview_inner.is_some_and(|p| p.contains(pos));
    app.hover = Some(pos);
    if app.dialog.is_some() {
        return;
    }
    match m.kind {
        // the wheel moves the cursor (or the preview when shown alone): the editor view follows the cursor
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let down = m.kind == MouseEventKind::ScrollDown;
            if app.view == View::Preview {
                app.preview_scroll = if down {
                    app.preview_scroll + 3
                } else {
                    app.preview_scroll.saturating_sub(3)
                };
            } else if in_panes {
                for _ in 0..3 {
                    app.editor
                        .move_cursor(if down { CursorMove::Down } else { CursorMove::Up });
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) if app.preview_inner.is_some_and(|p| p.contains(pos)) => {
            if let Some(l) = app.links.iter().find(|l| l.rect.contains(pos)).cloned() {
                if l.dest.starts_with('#') {
                    follow_link(app, &l.dest, l.target);
                } else {
                    app.dialog = Some(Dialog::OpenLink(l.dest));
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) if app.view != View::Preview && r.contains(pos) => {
            app.editor.cancel_selection();
            jump_to(app, pos);
            app.editor.start_selection();
            app.dragging = true;
        }
        MouseEventKind::Drag(MouseButton::Left) if app.dragging => jump_to(app, pos),
        MouseEventKind::Up(MouseButton::Left) if app.dragging => {
            app.dragging = false;
            // plain click: no selection
            if app.editor.selection_range().is_some_and(|(a, b)| a == b) {
                app.editor.cancel_selection();
            }
        }
        _ => {}
    }
}

/// Jumps to the heading of an internal link: at the top of the editor (and of the preview, if alone).
fn follow_link(app: &mut App, dest: &str, target: Option<(usize, usize)>) {
    let Some((line, row)) = target else {
        app.status = format!("{}: {dest}", tr().no_anchor);
        return;
    };
    app.editor.cancel_selection();
    // scroll tui-textarea's view together with our copy `editor_top`, then put the cursor there
    let delta = line as i64 - app.editor_top as i64;
    app.editor
        .scroll((delta.clamp(i16::MIN as i64, i16::MAX as i64) as i16, 0));
    app.editor_top = line;
    app.editor
        .move_cursor(CursorMove::Jump(line.min(u16::MAX as usize) as u16, 0));
    app.preview_scroll = row;
}

/// Opens an external link in the default browser without blocking the editor.
fn open_url(app: &mut App, url: &str) {
    // on WSL the browser is the Windows one
    let cmds: &[&str] = if env::var_os("WSL_DISTRO_NAME").is_some() {
        &["wslview", "explorer.exe", "xdg-open"]
    } else {
        &["xdg-open"]
    };
    let child = cmds.iter().find_map(|c| {
        let mut cmd = Command::new(c);
        cmd.arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn().ok()
    });
    match child {
        // wait on a separate thread, so the finished process does not linger as a zombie
        Some(mut c) => drop(std::thread::spawn(move || c.wait())),
        None => app.status = format!("{}: {url}", tr().open_error),
    }
}

/// Moves the cursor under the mouse. Outside the editor it stops at the border; past the top or
/// bottom border it goes one row further, so dragging outside scrolls the view.
fn jump_to(app: &mut App, pos: Position) {
    let r = app.editor_inner;
    let row = if pos.y < r.y {
        app.editor_top.saturating_sub(1)
    } else if pos.y >= r.bottom() {
        app.editor_top + r.height as usize
    } else {
        app.editor_top + (pos.y - r.y) as usize
    };
    let col = app.editor_left + pos.x.clamp(r.x, r.right().saturating_sub(1)).saturating_sub(r.x) as usize;
    app.editor.move_cursor(CursorMove::Jump(
        row.min(u16::MAX as usize) as u16,
        col.min(u16::MAX as usize) as u16,
    ));
}

/// Copies to the system clipboard with the OSC 52 sequence (Windows Terminal, kitty, WezTerm, iTerm2, ...).
fn copy_to_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }
    let mut out = io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                ABC[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Images;
    use ratatui_image::picker::Picker;
    use std::fs;

    #[test]
    fn search_is_literal_smart_case_and_cycles() {
        let images = Images::new(Picker::halfblocks(), ".".into());
        let text = "a.b axb\nA.B\nnone\n";
        let mut app = App::new("/nonexistent/doc.md".into(), text.into(), images);
        // "." is literal, and a lowercase query also finds uppercase text
        for c in "a.b".chars() {
            search_key(&mut app, KeyCode::Char(c), false);
        }
        assert_eq!(match_count(&app), (2, 1));
        assert_eq!(app.editor.cursor(), (0, 0));
        search_key(&mut app, KeyCode::Enter, false);
        assert_eq!((app.editor.cursor(), match_count(&app)), ((1, 0), (2, 2)));
        // wraps around, and ↑ goes back
        search_key(&mut app, KeyCode::Down, false);
        assert_eq!(app.editor.cursor(), (0, 0));
        search_key(&mut app, KeyCode::Up, false);
        assert_eq!(app.editor.cursor(), (1, 0));
        // an uppercase letter makes it case-sensitive
        app.query = "A.B".into();
        set_query(&mut app);
        assert_eq!(match_count(&app), (1, 1));
        // Esc closes the bar and clears the highlight but keeps the query for the next Ctrl+F
        app.searching = true;
        search_key(&mut app, KeyCode::Esc, false);
        assert!(!app.searching && app.editor.search_pattern().is_none());
        assert_eq!(app.query, "A.B");
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_first() {
        let dir = env::temp_dir().join(format!("mdedit-quit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");
        fs::write(&path, "old\n").unwrap();
        let images = Images::new(Picker::halfblocks(), ".".into());
        let mut app = App::new(path.to_string_lossy().into_owned(), "old\n".into(), images);
        app.editor.insert_str("new ");
        app.dirty = true;

        // other keys keep the dialog open; Esc cancels without quitting
        assert!(!dialog_key(&mut app, Dialog::Quit, KeyCode::Char('x'), false));
        assert!(matches!(app.dialog.take(), Some(Dialog::Quit)));
        assert!(!dialog_key(&mut app, Dialog::Quit, KeyCode::Esc, false));
        assert!(app.dialog.is_none());

        // Ctrl+Q quits without saving
        assert!(dialog_key(&mut app, Dialog::Quit, KeyCode::Char('q'), true));
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");

        // a file changed by another program: Enter does not quit, it asks about the change first
        fs::write(&path, "theirs, longer\n").unwrap();
        assert!(!dialog_key(&mut app, Dialog::Quit, KeyCode::Enter, false));
        assert!(matches!(app.dialog, Some(Dialog::Reload(_))));
        let Some(Dialog::Reload(text)) = app.dialog.take() else {
            unreachable!()
        };
        app.keep_mine(text);

        // Enter saves and quits
        assert!(dialog_key(&mut app, Dialog::Quit, KeyCode::Enter, false));
        assert_eq!(fs::read_to_string(&path).unwrap(), "new old\n");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
            ("è", "w6g="),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }
}
