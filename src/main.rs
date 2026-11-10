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

/// Pezzo di link cliccabile visibile nell'anteprima, in coordinate dello schermo.
#[derive(Clone)]
struct ScreenLink {
    rect: Rect,
    /// stesso id = stesso link (anche se va a capo)
    id: usize,
    dest: String,
    /// link interni: (riga sorgente, riga d'anteprima) del titolo di destinazione
    target: Option<(usize, usize)>,
}

struct App<'a> {
    path: String,
    editor: TextArea<'a>,
    view: View,
    side_by_side: bool,
    preview_scroll: usize,
    /// prima riga visibile e altezza dell'editor. tui-textarea non le espone: le ricalcoliamo con la
    /// sua stessa regola (scorre il minimo per tenere visibile il cursore), vedi `next_scroll_top`
    editor_top: usize,
    editor_h: usize,
    /// prima colonna visibile (scorrimento orizzontale delle righe lunghe), stessa regola di `editor_top`
    editor_left: usize,
    /// area del testo nell'editor e nell'anteprima nell'ultimo disegno, per il mouse
    editor_inner: Rect,
    preview_inner: Option<Rect>,
    links: Vec<ScreenLink>,
    /// posizione del mouse, per evidenziare il link sotto il puntatore
    hover: Option<Position>,
    /// link esterno in attesa di conferma prima di aprirlo nel browser
    confirm: Option<String>,
    /// selezione col mouse in corso (tasto sinistro premuto nell'editor)
    dragging: bool,
    images: Images,
    /// a capo del file: "\r\n" (CRLF, Windows) o "\n" (LF); mantenuto al salvataggio
    eol: &'static str,
    dirty: bool,
    quit_armed: bool,
    status: String,
}

/// Stessa regola di tui-textarea: la vista si sposta solo quando il cursore esce dallo schermo.
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
    // il mouse lo gestiamo noi: così la selezione resta dentro l'editor invece di attraversare lo schermo
    execute!(io::stdout(), EnableMouseCapture)?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        hook(info);
    }));
    // chiede al terminale protocollo grafico (sixel, kitty, iTerm2) e dimensione dei caratteri;
    // senza risposta si ripiega sui mezzi blocchi colorati
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
        // ponytail: con a capo misti vince CRLF se ce n'è almeno uno
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
