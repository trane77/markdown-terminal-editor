mod i18n;
mod images;
mod input;
mod markdown;
mod ui;

use std::{env, fs, io, path::Path, time::SystemTime};

use ratatui::{
    crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture},
        execute,
    },
    layout::{Position, Rect},
    style::Style,
    widgets::{Block, Borders},
};
use ratatui_image::picker::Picker;
use tui_textarea::{CursorMove, TextArea};

use i18n::tr;
use images::Images;
use input::run;
use ui::draw;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Editor,
    Preview,
    Split,
}

/// Piece of a clickable link visible in the preview, in screen coordinates.
#[derive(Clone)]
struct ScreenLink {
    rect: Rect,
    /// same id = same link (even when it wraps)
    id: usize,
    dest: String,
    /// internal links: (source row, preview row) of the target heading
    target: Option<(usize, usize)>,
}

/// Modal dialog shown over the screen; while open, it takes all keys.
enum Dialog {
    /// open this external link in the browser?
    OpenLink(String),
    /// the file changed on disk: reload it (new content attached) or keep the editor's version?
    Reload(String),
    /// quitting with unsaved changes: save first, quit anyway or cancel?
    Quit,
}

/// What identifies a file version on disk without reading it: modification time and size.
type DiskStamp = Option<(SystemTime, u64)>;

fn disk_stamp(path: &str) -> DiskStamp {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn new_editor<'a>(text: &str) -> TextArea<'a> {
    let mut editor = TextArea::from(text.lines());
    editor.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", tr().editor)),
    );
    editor.set_cursor_line_style(Style::default());
    editor
}

/// "\r\n" if the text uses CRLF line endings, "\n" otherwise.
fn detect_eol(text: &str) -> &'static str {
    // ponytail: with mixed line endings, CRLF wins if there is at least one
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

struct App<'a> {
    path: String,
    editor: TextArea<'a>,
    view: View,
    side_by_side: bool,
    preview_scroll: usize,
    /// first visible row and height of the editor. tui-textarea does not expose them: we recompute them
    /// with its own rule (scroll as little as needed to keep the cursor visible), see `next_scroll_top`
    editor_top: usize,
    editor_h: usize,
    /// first visible column (horizontal scrolling of long lines), same rule as `editor_top`
    editor_left: usize,
    /// text area of the editor and the preview in the last frame, for the mouse
    editor_inner: Rect,
    preview_inner: Option<Rect>,
    links: Vec<ScreenLink>,
    /// mouse position, to highlight the link under the pointer
    hover: Option<Position>,
    dialog: Option<Dialog>,
    /// the find bar is open and takes the keys; `query` stays for the next Ctrl+F
    searching: bool,
    query: String,
    /// in preview-only view, scroll the preview to the cursor on the next frame (after a search jump)
    reveal: bool,
    /// the file as we last read or wrote it, to notice when another program changes it
    disk: DiskStamp,
    disk_text: String,
    /// mouse selection in progress (left button pressed in the editor)
    dragging: bool,
    images: Images,
    /// line ending of the file: "\r\n" (CRLF, Windows) or "\n" (LF); kept when saving
    eol: &'static str,
    dirty: bool,
    status: String,
}

/// Same rule as tui-textarea: the view moves only when the cursor leaves the screen.
fn next_scroll_top(prev_top: usize, cursor: usize, len: usize) -> usize {
    if cursor < prev_top {
        cursor
    } else if prev_top + len <= cursor {
        cursor + 1 - len
    } else {
        prev_top
    }
}

impl App<'_> {
    fn new(path: String, text: String, images: Images) -> Self {
        App {
            disk: disk_stamp(&path),
            editor: new_editor(&text),
            path,
            view: View::Split,
            side_by_side: true,
            preview_scroll: 0,
            editor_top: 0,
            editor_h: 1,
            editor_left: 0,
            editor_inner: Rect::default(),
            preview_inner: None,
            links: Vec::new(),
            hover: None,
            dialog: None,
            searching: false,
            query: String::new(),
            reveal: false,
            dragging: false,
            images,
            eol: detect_eol(&text),
            disk_text: text,
            dirty: false,
            status: String::new(),
        }
    }

    /// After an edit: unsaved means different from the file, so undoing back to it clears the marker.
    fn edited(&mut self) {
        self.dirty = self.contents() != self.disk_text;
    }

    /// The editor's text as it would be written to disk.
    fn contents(&self) -> String {
        let mut text = self.editor.lines().join(self.eol);
        text.push_str(self.eol);
        text
    }

    fn save(&mut self) {
        // never overwrite changes made by another program without asking
        if self.check_disk() {
            return;
        }
        let text = self.contents();
        match fs::write(&self.path, &text) {
            Ok(()) => {
                self.disk = disk_stamp(&self.path);
                self.disk_text = text;
                self.dirty = false;
                self.status = tr().saved.into();
            }
            Err(e) => self.status = format!("{}: {e}", tr().save_error),
        }
    }

    /// Opens the reload dialog if another program changed the file since we last read or wrote it.
    /// Returns true when it did. A changed timestamp with the same content is ignored, and so is a
    /// file that was deleted or cannot be read (saving recreates it).
    fn check_disk(&mut self) -> bool {
        if self.dialog.is_some() {
            return false;
        }
        let stamp = disk_stamp(&self.path);
        if stamp == self.disk {
            return false;
        }
        self.disk = stamp;
        let Ok(text) = fs::read_to_string(&self.path) else {
            return false;
        };
        if text == self.disk_text {
            return false;
        }
        if text == self.contents() {
            // the other program wrote exactly what the editor holds
            self.disk_text = text;
            self.dirty = false;
            return false;
        }
        self.dialog = Some(Dialog::Reload(text));
        true
    }

    /// Replaces the editor's text with the file's new content, keeping the cursor where it was.
    fn reload(&mut self, text: String) {
        let (row, col) = self.editor.cursor();
        self.editor = new_editor(&text);
        // the new textarea starts scrolled to the top: reset our copy of its viewport too
        (self.editor_top, self.editor_left) = (0, 0);
        self.editor.move_cursor(CursorMove::Jump(
            row.min(u16::MAX as usize) as u16,
            col.min(u16::MAX as usize) as u16,
        ));
        self.eol = detect_eol(&text);
        self.disk_text = text;
        self.dirty = false;
        self.status = tr().reloaded.into();
    }

    /// Keeps the editor's version: it now differs from the file, so it counts as unsaved.
    fn keep_mine(&mut self, text: String) {
        self.disk_text = text;
        self.dirty = true;
    }
}

fn main() -> io::Result<()> {
    let Some(path) = env::args().nth(1) else {
        eprintln!("{}: mdedit <file.md>", tr().usage);
        std::process::exit(1);
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut terminal = ratatui::init();
    // we handle the mouse ourselves, so the selection stays inside the editor instead of spanning the screen
    execute!(io::stdout(), EnableMouseCapture)?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        hook(info);
    }));
    // ask the terminal for its graphics protocol (sixel, kitty, iTerm2) and font size;
    // without an answer, fall back to colored half blocks
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let base = Path::new(&path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let images = Images::new(picker, base.to_path_buf());

    let mut app = App::new(path, text, images);
    let res = run(&mut terminal, &mut app);
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_for(path: &Path, text: &str) -> App<'static> {
        fs::write(path, text).unwrap();
        let images = Images::new(Picker::halfblocks(), ".".into());
        App::new(path.to_string_lossy().into_owned(), text.into(), images)
    }

    #[test]
    fn undo_back_to_the_saved_text_clears_the_unsaved_marker() {
        let images = Images::new(Picker::halfblocks(), ".".into());
        let mut app = App::new("/nonexistent/doc.md".into(), "one\n".into(), images);
        app.editor.insert_char('x');
        app.edited();
        assert!(app.dirty);
        assert!(app.editor.undo());
        app.edited();
        assert!(!app.dirty);
        assert!(app.editor.redo());
        app.edited();
        assert!(app.dirty);
        assert_eq!(app.editor.lines(), ["xone"]);
    }

    #[test]
    fn external_changes_ask_before_reloading_or_overwriting() {
        let dir = env::temp_dir().join(format!("mdedit-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");

        // nothing changed: no dialog
        let mut app = app_for(&path, "one\ntwo\n");
        assert!(!app.check_disk());

        // same content with a new timestamp (e.g. `touch`): still no dialog
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!app.check_disk());

        // another program changes the file: reloading takes its content and keeps the cursor row
        app.editor.move_cursor(CursorMove::Jump(1, 0));
        fs::write(&path, "one\nTWO\nthree\n").unwrap();
        assert!(app.check_disk());
        let Some(Dialog::Reload(text)) = app.dialog.take() else {
            panic!("no reload dialog")
        };
        app.reload(text);
        assert_eq!(app.editor.lines(), ["one", "TWO", "three"]);
        assert_eq!(app.editor.cursor().0, 1);
        assert!(!app.dirty);

        // saving after an external change asks first and leaves the file alone
        fs::write(&path, "theirs\n").unwrap();
        app.save();
        assert!(matches!(app.dialog, Some(Dialog::Reload(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");

        // keeping our version marks it unsaved; the next save writes it
        let Some(Dialog::Reload(text)) = app.dialog.take() else {
            panic!("no reload dialog")
        };
        app.keep_mine(text);
        assert!(app.dirty);
        app.save();
        assert!(app.dialog.is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\nTWO\nthree\n");
        assert!(!app.check_disk());

        fs::remove_dir_all(&dir).unwrap();
    }
}
