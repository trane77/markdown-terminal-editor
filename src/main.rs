use std::{
    collections::HashMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use image::{imageops::FilterType, DynamicImage};

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers,
            MouseButton, MouseEvent, MouseEventKind,
        },
        execute,
    },
    layout::{Constraint, Direction, Layout, Margin, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    DefaultTerminal, Frame,
};
use ratatui_image::{picker::Picker, protocol::Protocol, Image, Resize};
use tui_textarea::{CursorMove, TextArea};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Editor,
    Preview,
    Split,
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
                self.status = "Salvato".into();
            }
            Err(e) => self.status = format!("Errore nel salvataggio: {e}"),
        }
    }
}

fn main() -> io::Result<()> {
    let Some(path) = env::args().nth(1) else {
        eprintln!("uso: mdedit <file.md>");
        std::process::exit(1);
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut editor = TextArea::from(text.lines());
    editor.set_block(Block::default().borders(Borders::ALL).title(" Editor "));
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
    let base = Path::new(&path).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
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

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
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
                app.status = "Modifiche non salvate! Ctrl+Q di nuovo per uscire senza salvare".into();
            }
            (true, KeyCode::Char('s')) => app.save(),
            (true, KeyCode::Char('e')) => app.view = View::Editor,
            (true, KeyCode::Char('p')) => app.view = View::Preview,
            (true, KeyCode::Char('l')) => {
                // Ctrl+L entra nello split; se già in split, alterna affiancato / sovrapposto
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
            // tui-textarea farebbe scorrere la vista senza muovere il cursore, rendendo `editor_top`
            // impossibile da ricalcolare: spostiamo invece il cursore di una pagina
            (_, KeyCode::PageDown | KeyCode::PageUp | KeyCode::Char('v'))
                if key.code != KeyCode::Char('v') || ctrl || alt =>
            {
                let down = key.code == KeyCode::PageDown || (key.code == KeyCode::Char('v') && ctrl);
                for _ in 0..app.editor_h {
                    app.editor.move_cursor(if down { CursorMove::Down } else { CursorMove::Up });
                }
            }
            _ => {
                if app.editor.input(key) {
                    app.dirty = true;
                }
                // Ctrl+C / Ctrl+X: oltre agli appunti interni dell'editor, anche quelli di sistema
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
    match m.kind {
        // la rotellina sposta il cursore (o l'anteprima da sola): la vista dell'editor segue il cursore
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let down = m.kind == MouseEventKind::ScrollDown;
            if app.view == View::Preview {
                app.preview_scroll = if down { app.preview_scroll + 3 } else { app.preview_scroll.saturating_sub(3) };
            } else if in_panes {
                for _ in 0..3 {
                    app.editor.move_cursor(if down { CursorMove::Down } else { CursorMove::Up });
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
            // semplice clic: nessuna selezione
            if app.editor.selection_range().is_some_and(|(a, b)| a == b) {
                app.editor.cancel_selection();
            }
        }
        _ => {}
    }
}

/// Porta il cursore sotto il mouse. Fuori dall'editor si ferma al bordo: oltre quello in alto o in
/// basso va una riga più in là, così trascinando fuori la vista scorre.
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
    app.editor.move_cursor(CursorMove::Jump(row.min(u16::MAX as usize) as u16, col.min(u16::MAX as usize) as u16));
}

/// Copia negli appunti di sistema con la sequenza OSC 52 (Windows Terminal, kitty, WezTerm, iTerm2, ...).
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
            out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn draw(f: &mut Frame, app: &mut App) {
    let [main, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());
    let (editor_area, preview_area) = match app.view {
        View::Editor => (Some(main), None),
        View::Preview => (None, Some(main)),
        View::Split => {
            let dir = if app.side_by_side { Direction::Horizontal } else { Direction::Vertical };
            let [a, b] = Layout::new(dir, [Constraint::Percentage(50); 2]).areas(main);
            (Some(a), Some(b))
        }
    };

    let cursor_row = app.editor.cursor().0;
    app.preview_inner = None;
    if let Some(area) = editor_area {
        app.editor_inner = area.inner(Margin { vertical: 1, horizontal: 1 });
        app.editor_h = area.height.saturating_sub(2).max(1) as usize;
        app.editor_top = next_scroll_top(app.editor_top, cursor_row, app.editor_h);
        app.editor_left = next_scroll_top(app.editor_left, app.editor.cursor().1, app.editor_inner.width.max(1) as usize);
        f.render_widget(&app.editor, area);
        // sempre sul bordo destro dell'editor: nello split affiancato è il bordo in mezzo ai due riquadri
        scrollbar(f, area, app.editor.lines().len(), app.editor_top, app.editor_h);
    }

    if let Some(area) = preview_area {
        let block = Block::default().borders(Borders::ALL).title(" Anteprima ");
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
            app.preview_scroll = app.preview_scroll.min(pv.lines.len().saturating_sub(inner.height as usize));
        }
        let content = Rect { y: inner.y + pad as u16, height: inner.height - pad as u16, ..inner };
        let total = pv.lines.len();
        let lines = pv.lines.into_iter().skip(app.preview_scroll).take(content.height as usize).collect::<Vec<_>>();
        f.render_widget(Paragraph::new(lines), content);
        // immagini sopra le righe riservate; di quelle tagliate dallo scorrimento si disegna solo la parte visibile
        let (top, h) = (app.preview_scroll, content.height as usize);
        for slot in &pv.images {
            let (first, last) = (slot.line, slot.line + slot.rows as usize);
            let (from, to) = (first.max(top), last.min(top + h));
            if from >= to {
                continue;
            }
            let rect = Rect { x: content.x + slot.x, y: content.y + (from - top) as u16, width: slot.cols, height: (to - from) as u16 };
            if let Some(p) = app.images.protocol(slot, (from - first) as u16, (to - from) as u16) {
                f.render_widget(Image::new(p), rect);
            }
        }
        // nello split affiancato basta la scrollbar dell'editor in mezzo: i due riquadri scorrono insieme
        if !(app.view == View::Split && app.side_by_side) {
            scrollbar(f, area, total, app.preview_scroll, inner.height as usize);
        }
    }

    let view = match app.view {
        View::Editor => "editor",
        View::Preview => "anteprima (↑↓ PgSu PgGiù)",
        View::Split if app.side_by_side => "split affiancato",
        View::Split => "split sovrapposto",
    };
    let text = format!(
        " {}{} | {view} | img: {:?} | ^E editor  ^P anteprima  ^L split/ruota  ^S salva  ^Q esci  {}",
        app.path,
        if app.dirty { " [+]" } else { "" },
        app.images.picker.protocol_type(),
        app.status
    );
    let pos = format!(
        " Ln {}/{} · {} ",
        cursor_row + 1,
        app.editor.lines().len(),
        if app.eol == "\r\n" { "CRLF" } else { "LF" }
    );
    let [left, right] = Layout::horizontal([Constraint::Min(0), Constraint::Length(pos.chars().count() as u16)]).areas(status);
    let bar = Style::new().add_modifier(Modifier::REVERSED);
    f.render_widget(Paragraph::new(text).style(bar), left);
    f.render_widget(Paragraph::new(pos).style(bar), right);
}

/// Scrollbar disegnata sopra il bordo destro di `area` (riquadro con bordi).
fn scrollbar(f: &mut Frame, area: Rect, total: usize, top: usize, view: usize) {
    // ratatui vuole il numero di posizioni di scorrimento: con `top` all'ultima la barra tocca il fondo
    let mut state = ScrollbarState::new(total.saturating_sub(view) + 1).position(top).viewport_content_length(view);
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"));
    f.render_stateful_widget(bar, area.inner(Margin { vertical: 1, horizontal: 0 }), &mut state);
}

/// Simbolo delle liste puntate per livello di annidamento (dal 4° livello si ricomincia).
const BULLETS: [&str; 3] = ["• ", "◦ ", "⊙ "];

fn code_style() -> Style {
    Style::new().fg(Color::Green)
}

struct Preview {
    lines: Vec<Line<'static>>,
    /// per ogni riga: riga del sorgente da cui proviene, e se ha contenuto (false = separatore)
    src: Vec<(usize, bool)>,
    images: Vec<ImageSlot>,
}

/// Spazio riservato a un'immagine nell'anteprima: righe vuote da `line`, a partire dalla colonna `x`.
struct ImageSlot {
    line: usize,
    x: u16,
    cols: u16,
    rows: u16,
    path: PathBuf,
}

/// Immagini dell'anteprima: lettura, misura in celle e codifica per il terminale, con cache.
struct Images {
    picker: Picker,
    /// cartella del file .md: i percorsi relativi delle immagini partono da qui
    base: PathBuf,
    /// altezza massima di un'immagine, in righe
    max_rows: u16,
    // ponytail: le cache non si aggiornano se un'immagine cambia su disco; serve riavviare l'editor
    decoded: HashMap<PathBuf, Option<DynamicImage>>,
    resized: HashMap<(PathBuf, u16, u16), DynamicImage>,
    protocols: HashMap<(PathBuf, u16, u16, u16, u16), Protocol>,
}

impl Images {
    fn new(picker: Picker, base: PathBuf) -> Self {
        Images { picker, base, max_rows: 20, decoded: HashMap::new(), resized: HashMap::new(), protocols: HashMap::new() }
    }

    /// Celle (colonne, righe) occupate dall'immagine `dest` entro `width` colonne, rispettando le
    /// proporzioni. None se non è un file locale leggibile (remota, mancante, formato non supportato).
    fn size(&mut self, dest: &str, width: u16) -> Option<(PathBuf, u16, u16)> {
        if dest.contains("://") || width == 0 {
            return None;
        }
        let path = self.base.join(dest);
        let img = self.decoded.entry(path.clone()).or_insert_with(|| image::open(&path).ok()).as_ref()?;
        let (fw, fh) = self.picker.font_size();
        let (fw, fh, w, h) = (fw as f64, fh as f64, img.width() as f64, img.height() as f64);
        let mut cols = (w / fw).ceil().clamp(1.0, width as f64);
        let mut rows = (cols * fw * h / w / fh).ceil().max(1.0);
        if rows > self.max_rows as f64 {
            rows = self.max_rows as f64;
            cols = (rows * fh * w / h / fw).round().clamp(1.0, width as f64);
        }
        Some((path, cols as u16, rows as u16))
    }

    /// Immagine codificata per il terminale, limitata alle righe `skip..skip + take`: le altre sono
    /// fuori dallo schermo. Ritagliare l'immagine stessa funziona con ogni protocollo, sixel compreso.
    fn protocol(&mut self, slot: &ImageSlot, skip: u16, take: u16) -> Option<&Protocol> {
        let key = (slot.path.clone(), slot.cols, slot.rows, skip, take);
        if !self.protocols.contains_key(&key) {
            // ponytail: svuota tutto oltre una soglia; un LRU se lo scorrimento tra molte immagini rallenta
            if self.protocols.len() > 64 {
                self.protocols.clear();
            }
            if self.resized.len() > 16 {
                self.resized.clear();
            }
            let (fw, fh) = self.picker.font_size();
            let (pw, ph) = (slot.cols as u32 * fw as u32, slot.rows as u32 * fh as u32);
            let img = self.decoded.get(&slot.path)?.as_ref()?;
            let full = self
                .resized
                .entry((slot.path.clone(), slot.cols, slot.rows))
                .or_insert_with(|| img.resize_exact(pw, ph, FilterType::Triangle));
            let part = full.crop_imm(0, skip as u32 * fh as u32, pw, take as u32 * fh as u32);
            let proto = self.picker.new_protocol(part, Rect::new(0, 0, slot.cols, take), Resize::Fit(None)).ok()?;
            self.protocols.insert(key.clone(), proto);
        }
        self.protocols.get(&key)
    }
}

impl Preview {
    /// Riga d'anteprima che corrisponde alla riga `row` del sorgente: la prima con contenuto
    /// tra quelle provenienti dalla riga sorgente più vicina che non la supera.
    fn row_for(&self, row: usize) -> usize {
        let Some(best) = self.src.iter().map(|s| s.0).filter(|&s| s <= row).max() else { return 0 };
        let pos = self.src.iter().position(|&s| s == (best, true));
        pos.or_else(|| self.src.iter().position(|s| s.0 == best)).unwrap_or(0)
    }
}

fn render_markdown(src: &str, width: u16, images: Option<&mut Images>) -> Preview {
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line_of = |byte: usize| starts.partition_point(|&s| s <= byte) - 1;
    let mut r = Renderer { width: width as usize, images, ..Renderer::default() };
    for (ev, range) in Parser::new_ext(src, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS).into_offset_iter() {
        r.line = line_of(range.start);
        // riga dopo l'ultima con contenuto: alcuni blocchi (liste) includono le righe vuote che li seguono
        let end = src[..range.end].trim_end().len();
        r.end_line = if end == 0 { 0 } else { line_of(end - 1) + 1 };
        r.event(ev);
    }
    r.flush();
    Preview { lines: r.lines, src: r.src, images: r.slots }
}

#[derive(Default)]
struct Renderer<'a> {
    lines: Vec<Line<'static>>,
    /// None (nei test): le immagini mostrano solo il testo alternativo
    images: Option<&'a mut Images>,
    slots: Vec<ImageSlot>,
    /// immagine in corso: destinazione e testo alternativo
    image: Option<(String, String)>,
    src: Vec<(usize, bool)>,
    /// righe del sorgente in cui iniziano/finiscono l'evento corrente
    line: usize,
    end_line: usize,
    /// testo in attesa di andare a capo, con la riga sorgente di ogni pezzo
    spans: Vec<(Span<'static>, usize)>,
    styles: Vec<Style>,
    width: usize,
    /// per ogni lista aperta: prossimo numero (None = puntata) e rientro del testo della voce corrente
    lists: Vec<(Option<u64>, usize)>,
    /// rientro delle righe che proseguono la voce di lista corrente
    indent: usize,
    /// la riga in costruzione inizia con il marcatore di lista, che fa già da rientro
    at_marker: bool,
    quote: usize,
    in_code: bool,
    heading: Option<(HeadingLevel, String)>,
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        HeadingLevel::H2 => Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        _ => Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    }
}

/// Titolo in lettere a blocchi: H1 alto 4 righe, largo 8 celle per lettera o, se non ci sta, 4;
/// H2 alto 4 righe e largo 4. Max 2 righe di testo grande, altrimenti None → titolo normale.
fn big_heading(level: HeadingLevel, text: &str, width: usize) -> Option<Vec<String>> {
    // righe del font 8x8 per ogni riga di pixel
    const FULL: &[&[usize]] = &[&[0], &[1], &[2], &[3], &[4], &[5], &[6], &[7]];
    // (pixel per cella in orizzontale, righe del font)
    let scales: &[(usize, &[&[usize]])] = match level {
        HeadingLevel::H1 => &[(1, FULL), (2, FULL)],
        HeadingLevel::H2 => &[(2, FULL)],
        _ => &[],
    };
    scales.iter().find_map(|&(sx, font_rows)| {
        let rows = wrap_words(text, width * sx / 8)?;
        (rows.len() <= 2).then(|| rows.iter().flat_map(|r| big_text(r, sx, font_rows)).collect())
    })
}

fn wrap_words(text: &str, cap: usize) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for w in text.split_whitespace() {
        let n = w.chars().count();
        if n > cap {
            return None;
        }
        match out.last_mut() {
            Some(l) if l.chars().count() + 1 + n <= cap => {
                l.push(' ');
                l.push_str(w);
            }
            _ => out.push(w.to_string()),
        }
    }
    Some(out)
}

/// Font 8x8 → righe di terminale con i caratteri quadrante (2 pixel in verticale per cella,
/// `sx` = 1 o 2 in orizzontale). Usa solo blocchi di base, visibili in ogni terminale.
fn big_text(text: &str, sx: usize, font_rows: &[&[usize]]) -> Vec<String> {
    use font8x8::{UnicodeFonts, BASIC_FONTS, LATIN_FONTS};
    const QUAD: [char; 16] = [' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█'];
    let glyphs: Vec<[u8; 8]> = text
        .chars()
        .map(|c| BASIC_FONTS.get(c).or_else(|| LATIN_FONTS.get(c)).unwrap_or(BASIC_FONTS.get('?').unwrap()))
        .collect();
    let px = |x: usize, y: usize| font_rows[y].iter().fold(0, |acc, &r| acc | (glyphs[x / 8][r] >> (x % 8)) & 1);
    (0..font_rows.len() / 2)
        .map(|row| {
            (0..glyphs.len() * 8 / sx)
                .map(|col| {
                    let mut i = 0;
                    for dy in 0..2 {
                        // con sx=1 le due colonne coincidono: la cella è piena o vuota per tutta la larghezza
                        for (dx, x) in [col * sx, col * sx + sx - 1].into_iter().enumerate() {
                            i |= px(x, row * 2 + dy) << (dy * 2 + dx);
                        }
                    }
                    QUAD[i as usize]
                })
                .collect()
        })
        .collect()
}

/// A capo per parole entro `width`: la prima riga inizia con `first`, le successive con `cont`
/// (barra delle citazioni + rientro delle liste). Le parole più lunghe della riga vengono spezzate.
/// Ogni riga prodotta porta la riga sorgente del suo primo carattere.
fn wrap(
    spans: Vec<(Span<'static>, usize)>,
    width: usize,
    first: Vec<Span<'static>>,
    cont: Vec<Span<'static>>,
) -> Vec<(Line<'static>, usize)> {
    use unicode_width::UnicodeWidthChar;
    let cw = |c: char| c.width().unwrap_or(0);
    let chars: Vec<(char, Style, usize)> =
        spans.iter().flat_map(|(s, src)| s.content.chars().map(move |c| (c, s.style, *src))).collect();

    let mut lines = Vec::new();
    let mut prefix = first;
    let mut cur: Vec<(char, Style, usize)> = Vec::new();
    let mut cur_w = 0;
    let mut emit = |prefix: &mut Vec<Span<'static>>, cur: &mut Vec<(char, Style, usize)>, cur_w: &mut usize| {
        while cur.last().is_some_and(|c| c.0 == ' ') {
            cur.pop();
        }
        let src = cur.first().map_or(0, |c| c.2);
        let mut line = std::mem::replace(prefix, cont.clone());
        let plen = line.len();
        for (c, style, _) in cur.drain(..) {
            let n = line.len();
            match line.last_mut() {
                Some(s) if s.style == style && n > plen => s.content.to_mut().push(c),
                _ => line.push(Span::styled(c.to_string(), style)),
            }
        }
        *cur_w = 0;
        lines.push((Line::from(line), src));
    };

    // gli spazi a inizio riga si tengono solo sulla prima (es. rientro del marcatore di lista annidata)
    let mut wrapped = false;
    let mut i = 0;
    while i < chars.len() {
        let space = chars[i].0 == ' ';
        let j = (i..chars.len()).find(|&k| (chars[k].0 == ' ') != space).unwrap_or(chars.len());
        let token = &chars[i..j];
        i = j;
        if space {
            if !cur.is_empty() || !wrapped {
                cur.extend_from_slice(token);
                cur_w += token.len();
            }
            continue;
        }
        let avail = |p: &Vec<Span>| width.saturating_sub(p.iter().map(Span::width).sum()).max(1);
        if !cur.is_empty() && cur_w + token.iter().map(|c| cw(c.0)).sum::<usize>() > avail(&prefix) {
            emit(&mut prefix, &mut cur, &mut cur_w);
            wrapped = true;
        }
        for &(c, style, src) in token {
            if !cur.is_empty() && cur_w + cw(c) > avail(&prefix) {
                emit(&mut prefix, &mut cur, &mut cur_w);
                wrapped = true;
            }
            cur.push((c, style, src));
            cur_w += cw(c);
        }
    }
    if !cur.is_empty() {
        emit(&mut prefix, &mut cur, &mut cur_w);
    }
    lines
}

impl Renderer<'_> {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, s: Style) {
        self.styles.push(self.style().patch(s));
    }

    fn bar(&self) -> Span<'static> {
        Span::styled("│ ".repeat(self.quote), Style::new().fg(Color::DarkGray))
    }

    fn push_line(&mut self, line: Line<'static>, src: usize, content: bool) {
        self.lines.push(line);
        self.src.push((src, content));
    }

    fn push_span(&mut self, span: Span<'static>) {
        self.spans.push((span, self.line));
    }

    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let pad = Span::raw(" ".repeat(self.indent));
        let first = if std::mem::take(&mut self.at_marker) { vec![self.bar()] } else { vec![self.bar(), pad.clone()] };
        let spans = std::mem::take(&mut self.spans);
        for (line, src) in wrap(spans, self.width, first, vec![self.bar(), pad]) {
            self.push_line(line, src, true);
        }
    }

    /// Riga vuota di separazione; dentro una citazione mantiene la barra, così la linea resta continua.
    fn blank(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            let sep = if self.quote > 0 { Line::from(self.bar()) } else { Line::default() };
            self.push_line(sep, self.end_line, false);
        }
    }

    fn event(&mut self, ev: Event) {
        let width = self.width;
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    self.flush();
                    self.push_style(heading_style(level));
                    self.heading = Some((level, String::new()));
                }
                Tag::BlockQuote(_) => {
                    self.flush();
                    self.quote += 1;
                    self.push_style(Style::new().add_modifier(Modifier::ITALIC));
                }
                Tag::CodeBlock(_) => {
                    self.flush();
                    self.in_code = true;
                }
                Tag::List(start) => {
                    self.flush();
                    self.lists.push((start, 0));
                }
                Tag::Item => {
                    self.flush();
                    // le sottovoci si allineano al testo della voce madre
                    let depth = self.lists.len().saturating_sub(1);
                    let indent = " ".repeat(self.lists.iter().rev().nth(1).map_or(0, |l| l.1));
                    let marker = match self.lists.last_mut() {
                        Some((Some(n), _)) => {
                            *n += 1;
                            format!("{}. ", *n - 1)
                        }
                        _ => BULLETS[depth % BULLETS.len()].into(),
                    };
                    let marker = Span::styled(format!("{indent}{marker}"), Style::new().fg(Color::Yellow));
                    self.indent = marker.width();
                    if let Some(l) = self.lists.last_mut() {
                        l.1 = self.indent;
                    }
                    self.at_marker = true;
                    self.push_span(marker);
                }
                Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
                Tag::Emphasis => self.push_style(Style::new().add_modifier(Modifier::ITALIC)),
                Tag::Strong => self.push_style(Style::new().add_modifier(Modifier::BOLD)),
                Tag::Strikethrough => self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT)),
                Tag::Link { .. } => self.push_style(Style::new().fg(Color::Blue).add_modifier(Modifier::UNDERLINED)),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    self.styles.pop();
                    if let Some((level, text)) = self.heading.take() {
                        if let Some(rows) = big_heading(level, &text, width) {
                            self.spans.clear();
                            let style = Style { fg: heading_style(level).fg, ..Style::default() };
                            for r in rows {
                                self.push_line(Line::styled(r, style), self.line, true);
                            }
                        }
                    }
                    self.blank();
                }
                TagEnd::Paragraph => self.blank(),
                TagEnd::BlockQuote(_) => {
                    self.styles.pop();
                    self.flush();
                    // toglie la riga di sola barra lasciata dall'ultimo paragrafo, poi separa a livello esterno
                    if self.lines.last().is_some_and(|l| l.spans.len() == 1 && l.spans[0].content == self.bar().content) {
                        self.lines.pop();
                        self.src.pop();
                    }
                    self.quote -= 1;
                    self.blank();
                }
                TagEnd::CodeBlock => {
                    self.in_code = false;
                    self.blank();
                }
                TagEnd::List(_) => {
                    self.flush();
                    self.lists.pop();
                    self.indent = self.lists.last().map_or(0, |l| l.1);
                    if self.lists.is_empty() {
                        self.blank();
                    }
                }
                TagEnd::Item => self.flush(),
                TagEnd::Image => {
                    let Some((dest, alt)) = self.image.take() else { return };
                    // dopo la barra delle citazioni e il rientro della voce di lista
                    let x = (2 * self.quote + self.indent) as u16;
                    let avail = (self.width as u16).saturating_sub(x);
                    match self.images.as_deref_mut().and_then(|im| im.size(&dest, avail)) {
                        Some((path, cols, rows)) => {
                            self.flush();
                            let line = self.lines.len();
                            for _ in 0..rows {
                                self.push_line(Line::from(self.bar()), self.line, true);
                            }
                            self.slots.push(ImageSlot { line, x, cols, rows, path });
                        }
                        None => {
                            let style = Style::new().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
                            self.push_span(Span::styled(format!("[immagine: {alt}]"), style));
                        }
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    self.styles.pop();
                }
                _ => {}
            },
            Event::Text(t) if self.in_code => {
                // ogni riga di codice va a capo da sé, rientrata, così anteprima e schermo restano 1:1
                let pad = || vec![Span::raw("  ")];
                for (i, l) in t.lines().enumerate() {
                    let src = self.line + i;
                    let rows = wrap(vec![(Span::styled(l.to_string(), code_style()), src)], self.width, pad(), pad());
                    if rows.is_empty() {
                        self.push_line(Line::from(pad()), src, true);
                    }
                    for (line, src) in rows {
                        self.push_line(line, src, true);
                    }
                }
            }
            Event::Text(t) | Event::Code(t) if self.image.is_some() => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&t);
                }
            }
            Event::Text(t) | Event::Code(t) if self.heading.is_some() => {
                if let Some((_, h)) = &mut self.heading {
                    h.push_str(&t);
                }
                self.push_span(Span::styled(t.into_string(), self.style()));
            }
            Event::Text(t) => self.push_span(Span::styled(t.into_string(), self.style())),
            Event::Code(t) => self.push_span(Span::styled(t.into_string(), code_style())),
            Event::Html(t) | Event::InlineHtml(t) => self.push_span(Span::raw(t.trim_end().to_string())),
            Event::SoftBreak => self.push_span(Span::raw(" ")),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.push_line(Line::styled("─".repeat(width), Style::new().fg(Color::DarkGray)), self.line, true);
                self.blank();
            }
            Event::TaskListMarker(done) => {
                if let Some((s, _)) = self.spans.last_mut() {
                    let bullet = BULLETS.iter().find(|b| s.content.ends_with(*b));
                    if let Some(b) = bullet {
                        let indent = &s.content[..s.content.len() - b.len()];
                        s.content = format!("{indent}{}", if done { "☑ " } else { "☐ " }).into();
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(src: &str, width: u16) -> Vec<String> {
        render_markdown(src, width, None)
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn renders_basic_elements() {
        let src = "### Titolo\n\n- a\n  - b\n- [x] fatto\n- [ ] da fare\n\n1. uno\n2. due\n\n> cit\n\n---\n\n```\nx = 1\n\ny\n```\n";
        assert_eq!(
            plain(src, 40),
            [
                "Titolo", "", "• a", "  ◦ b", "☑ fatto", "☐ da fare", "", "1. uno", "2. due", "", "│ cit", "",
                "─".repeat(40).as_str(), "", "  x = 1", "  ", "  y", "",
            ]
        );
    }

    #[test]
    fn wraps_with_prefix() {
        // citazione: ogni riga a capo ripete la barra, i paragrafi restano uniti dalla barra
        assert_eq!(plain("> uno due tre quattro\n", 12), ["│ uno due", "│ tre", "│ quattro", ""]);
        assert_eq!(plain("> a\n>\n> b\n\nfuori\n", 40), ["│ a", "│ ", "│ b", "", "fuori", ""]);
        assert_eq!(plain("> a\n>> b\n", 40), ["│ a", "│ ", "│ │ b", "", ""].map(String::from)[..4].to_vec());
        // lista: la continuazione rientra sotto il testo, non sotto il punto
        assert_eq!(plain("- uno due tre\n- x\n", 9), ["• uno due", "  tre", "• x", ""]);
        // simbolo diverso per livello, sottovoci allineate al testo della voce madre
        assert_eq!(
            plain("* a\n   * b\n      * c\n         * d\n", 40),
            ["• a", "  ◦ b", "    ⊙ c", "      • d", ""]
        );
        // numerazione da 1 a ogni livello, sottovoci sotto il testo di "2. "
        assert_eq!(
            plain("1. a\n2. b\n   1. c\n   2. d\n      1. e\n", 40),
            ["1. a", "2. b", "   1. c", "   2. d", "      1. e", ""]
        );
        assert_eq!(plain("- a\n  - [x] b\n", 40), ["• a", "  ☑ b", ""]);
        // parola più lunga della riga: spezzata
        assert_eq!(plain("abcdef\n", 4), ["abcd", "ef", ""]);
    }

    #[test]
    fn source_rows_map_to_preview_rows() {
        let src = "# Ab\ntesto\n\n- a\n- b\n\n```\nx\n```\n\n> q1\n> q2\nfine";
        let pv = render_markdown(src, 40, None);
        let text = |i: usize| pv.lines[i].spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        // titolo grande: la sua riga sorgente punta alla prima delle 4 righe a blocchi
        assert_eq!(pv.row_for(0), 0);
        assert_eq!(text(pv.row_for(1)), "testo");
        assert_eq!(text(pv.row_for(2)), "");
        assert_eq!(text(pv.row_for(3)), "• a");
        assert_eq!(text(pv.row_for(4)), "• b");
        assert_eq!(text(pv.row_for(5)), "");
        assert_eq!(text(pv.row_for(7)), "  x");
        // righe unite nello stesso paragrafo puntano alla riga che le contiene
        assert_eq!(text(pv.row_for(11)), "│ q1 q2 fine");
        assert_eq!(pv.row_for(12), pv.row_for(10));
    }

    #[test]
    fn images_reserve_rows_or_show_alt_text() {
        let mut im = Images::new(Picker::halfblocks(), "test-docs".into());
        im.max_rows = 10;
        // halfblocks: celle da 10x20 px. panorama 640x320 in 40 colonne → 400x200 px → 10 righe
        let src = "prima\n\n![pano](img/panorama.png)\n\ndopo";
        let pv = render_markdown(src, 40, Some(&mut im));
        assert_eq!(pv.images.len(), 1);
        let s = &pv.images[0];
        assert_eq!((s.line, s.x, s.cols, s.rows), (2, 0, 40, 10));
        assert_eq!(pv.row_for(2), 2);
        assert_eq!(pv.lines[s.line + s.rows as usize + 1].spans.iter().map(|x| x.content.as_ref()).collect::<String>(), "dopo");
        // immagine alta: limitata a max_rows, larghezza ridotta in proporzione (240x480 → 10 righe, 10 colonne)
        let pv = render_markdown("![s](img/strisce.jpg)", 40, Some(&mut im));
        assert_eq!((pv.images[0].cols, pv.images[0].rows), (10, 10));
        // in una citazione parte dopo la barra
        let pv = render_markdown("> ![s](img/strisce.jpg)", 40, Some(&mut im));
        assert_eq!(pv.images[0].x, 2);
        // mancante o remota: solo testo alternativo
        let pv = render_markdown("![manca](img/no.png) ![web](https://x.org/a.png)", 80, Some(&mut im));
        assert!(pv.images.is_empty());
        assert_eq!(plain_lines(&pv)[0], "[immagine: manca] [immagine: web]");
        // codifica della parte visibile di un'immagine tagliata dallo scorrimento
        let pv = render_markdown("![pano](img/panorama.png)", 40, Some(&mut im));
        assert!(im.protocol(&pv.images[0], 3, 4).is_some());
    }

    fn plain_lines(pv: &Preview) -> Vec<String> {
        pv.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy"), ("è", "w6g=")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn big_headings() {
        // H1 largo: 8 celle per lettera, 4 righe
        let out = plain("# Ab\n", 80);
        assert_eq!(out.len(), 5);
        assert!(out[..4].iter().all(|r| r.chars().count() == 16));
        assert!(out[0].contains('█') || out[0].contains('▀') || out[0].contains('▄'));
        // H1 in spazio stretto → versione compatta da 4 celle
        let out = render_markdown("# Abcdef\n", 30, None).lines;
        assert_eq!(out[0].spans[0].content.chars().count(), 24);
        // H2 compatto, H3 testo normale
        let out = plain("## Ab\n", 80);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].chars().count(), 8);
        assert_eq!(plain("### Ab\n", 80), ["Ab", ""]);
        // titolo che non entra nemmeno compatto → testo normale
        assert_eq!(plain("# Titolo\n", 7)[0], "Titolo");
    }
}
