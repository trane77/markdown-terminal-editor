use std::{env, fs, io};

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    crossterm::event::{self, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use tui_textarea::TextArea;

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
    preview_scroll: u16,
    dirty: bool,
    quit_armed: bool,
    status: String,
}

impl App<'_> {
    fn save(&mut self) {
        let mut text = self.editor.lines().join("\n");
        text.push('\n');
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

    let mut app = App {
        path,
        editor,
        view: View::Split,
        side_by_side: true,
        preview_scroll: 0,
        dirty: false,
        quit_armed: false,
        status: String::new(),
    };
    let mut terminal = ratatui::init();
    let res = run(&mut terminal, &mut app);
    ratatui::restore();
    res
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;
        let TermEvent::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
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
            _ => {
                if app.editor.input(key) {
                    app.dirty = true;
                }
            }
        }
    }
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

    if let Some(area) = editor_area {
        f.render_widget(&app.editor, area);
    }

    if let Some(area) = preview_area {
        let inner_w = area.width.saturating_sub(2);
        let inner_h = area.height.saturating_sub(2) as usize;
        // ponytail: ri-parsa tutto il documento a ogni frame; cache sul testo se diventa lento con file enormi
        let para = Paragraph::new(render_markdown(&app.editor.lines().join("\n"), inner_w)).wrap(Wrap { trim: false });
        let total = para.line_count(inner_w);
        if app.view == View::Split {
            // ponytail: sync proporzionale cursore→anteprima, approssimato; mappa sorgente→riga se serve precisione
            let row = app.editor.cursor().0;
            let n = app.editor.lines().len().max(1);
            app.preview_scroll = (row * total / n).saturating_sub(inner_h / 2) as u16;
        }
        app.preview_scroll = app.preview_scroll.min(total.saturating_sub(inner_h) as u16);
        let block = Block::default().borders(Borders::ALL).title(" Anteprima ");
        f.render_widget(para.block(block).scroll((app.preview_scroll, 0)), area);
    }

    let view = match app.view {
        View::Editor => "editor",
        View::Preview => "anteprima (↑↓ PgSu PgGiù)",
        View::Split if app.side_by_side => "split affiancato",
        View::Split => "split sovrapposto",
    };
    let text = format!(
        " {}{} | {view} | ^E editor  ^P anteprima  ^L split/ruota  ^S salva  ^Q esci  {}",
        app.path,
        if app.dirty { " [+]" } else { "" },
        app.status
    );
    f.render_widget(Paragraph::new(text).style(Style::new().add_modifier(Modifier::REVERSED)), status);
}

/// Simbolo delle liste puntate per livello di annidamento (dal 4° livello si ricomincia).
const BULLETS: [&str; 3] = ["• ", "◦ ", "⊙ "];

fn code_style() -> Style {
    Style::new().fg(Color::Green)
}

fn render_markdown(src: &str, width: u16) -> Vec<Line<'static>> {
    let mut r = Renderer { width: width as usize, ..Renderer::default() };
    for ev in Parser::new_ext(src, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS) {
        r.event(ev);
    }
    r.flush();
    r.lines
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
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
fn wrap(spans: Vec<Span<'static>>, width: usize, first: Vec<Span<'static>>, cont: Vec<Span<'static>>) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthChar;
    let cw = |c: char| c.width().unwrap_or(0);
    let chars: Vec<(char, Style)> = spans.iter().flat_map(|s| s.content.chars().map(move |c| (c, s.style))).collect();

    let mut lines = Vec::new();
    let mut prefix = first;
    let mut cur: Vec<(char, Style)> = Vec::new();
    let mut cur_w = 0;
    let mut emit = |prefix: &mut Vec<Span<'static>>, cur: &mut Vec<(char, Style)>, cur_w: &mut usize| {
        while cur.last().is_some_and(|c| c.0 == ' ') {
            cur.pop();
        }
        let mut line = std::mem::replace(prefix, cont.clone());
        let plen = line.len();
        for (c, style) in cur.drain(..) {
            let n = line.len();
            match line.last_mut() {
                Some(s) if s.style == style && n > plen => s.content.to_mut().push(c),
                _ => line.push(Span::styled(c.to_string(), style)),
            }
        }
        *cur_w = 0;
        lines.push(Line::from(line));
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
        for &(c, style) in token {
            if !cur.is_empty() && cur_w + cw(c) > avail(&prefix) {
                emit(&mut prefix, &mut cur, &mut cur_w);
                wrapped = true;
            }
            cur.push((c, style));
            cur_w += cw(c);
        }
    }
    if !cur.is_empty() {
        emit(&mut prefix, &mut cur, &mut cur_w);
    }
    lines
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, s: Style) {
        self.styles.push(self.style().patch(s));
    }

    fn bar(&self) -> Span<'static> {
        Span::styled("│ ".repeat(self.quote), Style::new().fg(Color::DarkGray))
    }

    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let pad = Span::raw(" ".repeat(self.indent));
        let first = if std::mem::take(&mut self.at_marker) { vec![self.bar()] } else { vec![self.bar(), pad.clone()] };
        let spans = std::mem::take(&mut self.spans);
        self.lines.extend(wrap(spans, self.width, first, vec![self.bar(), pad]));
    }

    /// Riga vuota di separazione; dentro una citazione mantiene la barra, così la linea resta continua.
    fn blank(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            let sep = if self.quote > 0 { Line::from(self.bar()) } else { Line::default() };
            self.lines.push(sep);
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
                    self.spans.push(marker);
                }
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
                            self.lines.extend(rows.into_iter().map(|r| Line::styled(r, style)));
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
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    self.styles.pop();
                }
                _ => {}
            },
            Event::Text(t) if self.in_code => {
                for l in t.lines() {
                    self.lines.push(Line::from(Span::styled(format!("  {l}"), code_style())));
                }
            }
            Event::Text(t) | Event::Code(t) if self.heading.is_some() => {
                if let Some((_, h)) = &mut self.heading {
                    h.push_str(&t);
                }
                self.spans.push(Span::styled(t.into_string(), self.style()));
            }
            Event::Text(t) => self.spans.push(Span::styled(t.into_string(), self.style())),
            Event::Code(t) => self.spans.push(Span::styled(t.into_string(), code_style())),
            Event::Html(t) | Event::InlineHtml(t) => self.spans.push(Span::raw(t.trim_end().to_string())),
            Event::SoftBreak => self.spans.push(Span::raw(" ")),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.lines.push(Line::styled("─".repeat(width as usize), Style::new().fg(Color::DarkGray)));
                self.blank();
            }
            Event::TaskListMarker(done) => {
                if let Some(s) = self.spans.last_mut() {
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
        render_markdown(src, width)
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
    fn big_headings() {
        // H1 largo: 8 celle per lettera, 4 righe
        let out = plain("# Ab\n", 80);
        assert_eq!(out.len(), 5);
        assert!(out[..4].iter().all(|r| r.chars().count() == 16));
        assert!(out[0].contains('█') || out[0].contains('▀') || out[0].contains('▄'));
        // H1 in spazio stretto → versione compatta da 4 celle
        let out = render_markdown("# Abcdef\n", 30);
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
