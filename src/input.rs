//! Keyboard and mouse: commands, selection confined to the editor, links, clipboard.

use std::{
    env,
    io::{self, Write},
    process::{Command, Stdio},
};

use ratatui::{
    crossterm::event::{
        self, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    layout::Position,
    DefaultTerminal,
};
use tui_textarea::CursorMove;

use crate::{draw, i18n::tr, App, View};

pub(crate) fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;
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
        // while the confirmation dialog is open, only Enter (open) or Esc (cancel) count
        if let Some(url) = app.confirm.take() {
            match key.code {
                KeyCode::Enter => open_url(app, &url),
                KeyCode::Esc => {}
                _ => app.confirm = Some(url),
            }
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let is_quit = ctrl && key.code == KeyCode::Char('q');
        if !is_quit {
            app.quit_armed = false;
        }
        app.status.clear();

        match (ctrl, key.code) {
            (true, KeyCode::Char('q')) => {
                if !app.dirty || app.quit_armed {
                    return Ok(());
                }
                app.quit_armed = true;
                app.status = tr().unsaved.into();
            }
            (true, KeyCode::Char('s')) => app.save(),
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
            (_, KeyCode::PageDown | KeyCode::PageUp | KeyCode::Char('v'))
                if key.code != KeyCode::Char('v') || ctrl || alt =>
            {
                let down = key.code == KeyCode::PageDown || (key.code == KeyCode::Char('v') && ctrl);
                for _ in 0..app.editor_h {
                    app.editor
                        .move_cursor(if down { CursorMove::Down } else { CursorMove::Up });
                }
            }
            _ => {
                if app.editor.input(key) {
                    app.dirty = true;
                }
                // Ctrl+C / Ctrl+X: besides the editor's internal clipboard, also the system one
                if ctrl && matches!(key.code, KeyCode::Char('c' | 'x')) {
                    copy_to_clipboard(&app.editor.yank_text());
                }
            }
        }
    }
}

fn on_mouse(app: &mut App, m: MouseEvent) {
    let pos = Position::new(m.column, m.row);
    let r = app.editor_inner;
    let in_panes = r.contains(pos) || app.preview_inner.is_some_and(|p| p.contains(pos));
    app.hover = Some(pos);
    if app.confirm.is_some() {
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
                    app.confirm = Some(l.dest);
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
