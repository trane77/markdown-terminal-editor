mod i18n;
mod images;
mod input;
mod markdown;
mod ui;

use std::{env, fs, io, path::Path};

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
use tui_textarea::TextArea;

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
    /// external link waiting for confirmation before opening it in the browser
    confirm: Option<String>,
    /// mouse selection in progress (left button pressed in the editor)
    dragging: bool,
    images: Images,
    /// line ending of the file: "\r\n" (CRLF, Windows) or "\n" (LF); kept when saving
    eol: &'static str,
    dirty: bool,
    quit_armed: bool,
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
    fn save(&mut self) {
        let mut text = self.editor.lines().join(self.eol);
        text.push_str(self.eol);
        match fs::write(&self.path, text) {
            Ok(()) => {
                self.dirty = false;
                self.status = tr().saved.into();
            }
            Err(e) => self.status = format!("{}: {e}", tr().save_error),
        }
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
    let mut editor = TextArea::from(text.lines());
    editor.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", tr().editor)),
    );
    editor.set_cursor_line_style(Style::default());

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

    let mut app = App {
        path,
        editor,
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
        confirm: None,
        dragging: false,
        images,
        // ponytail: with mixed line endings, CRLF wins if there is at least one
        eol: if text.contains("\r\n") { "\r\n" } else { "\n" },
        dirty: false,
        quit_armed: false,
        status: String::new(),
    };
    let res = run(&mut terminal, &mut app);
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    res
}
