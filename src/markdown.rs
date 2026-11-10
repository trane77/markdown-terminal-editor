//! Markdown → righe d'anteprima: a capo, liste, citazioni, titoli grandi, immagini, link.

use std::collections::HashMap;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{
    i18n::tr,
    images::{ImageSlot, Images},
};

const BULLETS: [&str; 3] = ["• ", "◦ ", "⊙ "];

fn code_style() -> Style {
    Style::new().fg(Color::Green)
}

pub(crate) struct Preview {
    pub(crate) lines: Vec<Line<'static>>,
    /// per ogni riga: riga del sorgente da cui proviene, e se ha contenuto (false = separatore)
    src: Vec<(usize, bool)>,
    pub(crate) images: Vec<ImageSlot>,
    pub(crate) links: Vec<LinkHit>,
}

/// Pezzo di link interno (`[testo](#titolo)`) su una riga d'anteprima; un link che va a capo ne ha più d'uno.
pub(crate) struct LinkHit {
    pub(crate) line: usize,
    pub(crate) x: u16,
    pub(crate) width: u16,
    pub(crate) dest: String,
    /// riga sorgente del titolo di destinazione, None se non esiste
    pub(crate) target: Option<usize>,
}

/// Ancora di un titolo come la genera GitHub: minuscole, spazi → '-', via la punteggiatura.
fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}


impl Preview {
    /// Riga d'anteprima che corrisponde alla riga `row` del sorgente: la prima con contenuto
    /// tra quelle provenienti dalla riga sorgente più vicina che non la supera.
    pub(crate) fn row_for(&self, row: usize) -> usize {
        let Some(best) = self.src.iter().map(|s| s.0).filter(|&s| s <= row).max() else { return 0 };
        let pos = self.src.iter().position(|&s| s == (best, true));
        pos.or_else(|| self.src.iter().position(|s| s.0 == best)).unwrap_or(0)
    }
}

pub(crate) fn render_markdown(src: &str, width: u16, images: Option<&mut Images>) -> Preview {
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
    let links = r
        .links
        .into_iter()
        .map(|(line, x, width, i)| {
            let dest = r.dests[i].clone();
            let target = r.anchors.get(&dest[1..].to_lowercase()).copied();
            LinkHit { line, x, width, dest, target }
        })
        .collect();
    Preview { lines: r.lines, src: r.src, images: r.slots, links }
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
    /// testo in attesa di andare a capo, con la riga sorgente e l'eventuale link interno di ogni pezzo
    spans: Vec<(Span<'static>, usize, Option<usize>)>,
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
    /// destinazioni dei link interni; `link` è quello in corso
    dests: Vec<String>,
    link: Option<usize>,
    /// pezzi di link sulle righe prodotte: (riga, colonna, larghezza, indice in `dests`)
    links: Vec<(usize, u16, u16, usize)>,
    /// ancora del titolo → riga sorgente
    anchors: HashMap<String, usize>,
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
/// Ogni riga prodotta porta la riga sorgente del suo primo carattere e i pezzi di link interni
/// che contiene: (colonna, larghezza, link).
fn wrap(
    spans: Vec<(Span<'static>, usize, Option<usize>)>,
    width: usize,
    first: Vec<Span<'static>>,
    cont: Vec<Span<'static>>,
) -> Vec<(Line<'static>, usize, Vec<(u16, u16, usize)>)> {
    use unicode_width::UnicodeWidthChar;
    let cw = |c: char| c.width().unwrap_or(0);
    type Ch = (char, Style, usize, Option<usize>);
    let chars: Vec<Ch> =
        spans.iter().flat_map(|(s, src, link)| s.content.chars().map(move |c| (c, s.style, *src, *link))).collect();

    let mut lines = Vec::new();
    let mut prefix = first;
    let mut cur: Vec<Ch> = Vec::new();
    let mut cur_w = 0;
    let mut emit = |prefix: &mut Vec<Span<'static>>, cur: &mut Vec<Ch>, cur_w: &mut usize| {
        while cur.last().is_some_and(|c| c.0 == ' ') {
            cur.pop();
        }
        let src = cur.first().map_or(0, |c| c.2);
        let mut line = std::mem::replace(prefix, cont.clone());
        let plen = line.len();
        let mut col = line.iter().map(Span::width).sum::<usize>() as u16;
        let mut hits: Vec<(u16, u16, usize)> = Vec::new();
        for (c, style, _, link) in cur.drain(..) {
            let w = cw(c) as u16;
            if let Some(l) = link {
                match hits.last_mut() {
                    Some(h) if h.2 == l && h.0 + h.1 == col => h.1 += w,
                    _ => hits.push((col, w, l)),
                }
            }
            col += w;
            let n = line.len();
            match line.last_mut() {
                Some(s) if s.style == style && n > plen => s.content.to_mut().push(c),
                _ => line.push(Span::styled(c.to_string(), style)),
            }
        }
        *cur_w = 0;
        lines.push((Line::from(line), src, hits));
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
        for &ch in token {
            let c = ch.0;
            if !cur.is_empty() && cur_w + cw(c) > avail(&prefix) {
                emit(&mut prefix, &mut cur, &mut cur_w);
                wrapped = true;
            }
            cur.push(ch);
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
        self.spans.push((span, self.line, self.link));
    }

    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let pad = Span::raw(" ".repeat(self.indent));
        let first = if std::mem::take(&mut self.at_marker) { vec![self.bar()] } else { vec![self.bar(), pad.clone()] };
        let spans = std::mem::take(&mut self.spans);
        for (line, src, hits) in wrap(spans, self.width, first, vec![self.bar(), pad]) {
            let n = self.lines.len();
            self.links.extend(hits.into_iter().map(|(x, w, l)| (n, x, w, l)));
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
                Tag::Link { dest_url, .. } => {
                    if dest_url.starts_with('#') {
                        self.dests.push(dest_url.to_string());
                        self.link = Some(self.dests.len() - 1);
                    }
                    self.push_style(Style::new().fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    self.styles.pop();
                    if let Some((level, text)) = self.heading.take() {
                        // titoli uguali: "-1", "-2", ... come su GitHub
                        let base = slug(&text);
                        let (mut anchor, mut n) = (base.clone(), 0);
                        while self.anchors.contains_key(&anchor) {
                            n += 1;
                            anchor = format!("{base}-{n}");
                        }
                        self.anchors.insert(anchor, self.line);
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
                            self.push_span(Span::styled(format!("[{}: {alt}]", tr().image), style));
                        }
                    }
                }
                TagEnd::Link => {
                    self.link = None;
                    self.styles.pop();
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    self.styles.pop();
                }
                _ => {}
            },
            Event::Text(t) if self.in_code => {
                // ogni riga di codice va a capo da sé, rientrata, così anteprima e schermo restano 1:1
                let pad = || vec![Span::raw("  ")];
                for (i, l) in t.lines().enumerate() {
                    let src = self.line + i;
                    let rows = wrap(vec![(Span::styled(l.to_string(), code_style()), src, None)], self.width, pad(), pad());
                    if rows.is_empty() {
                        self.push_line(Line::from(pad()), src, true);
                    }
                    for (line, src, _) in rows {
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
                if let Some((s, ..)) = self.spans.last_mut() {
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
    use ratatui_image::picker::Picker;


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
        let i = tr().image;
        assert_eq!(plain_lines(&pv)[0], format!("[{i}: manca] [{i}: web]"));
        // codifica della parte visibile di un'immagine tagliata dallo scorrimento
        let pv = render_markdown("![pano](img/panorama.png)", 40, Some(&mut im));
        assert!(im.protocol(&pv.images[0], 3, 4).is_some());
    }

    fn plain_lines(pv: &Preview) -> Vec<String> {
        pv.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn internal_links_point_to_headings() {
        let pv = render_markdown("# Uno\n\n## Due, tre!\n\nvai a [qui](#due-tre) o [lì](#manca) o [web](http://x.org)\n", 40, None);
        let l: Vec<_> = pv.links.iter().map(|l| (l.x, l.width, l.dest.as_str(), l.target)).collect();
        assert_eq!(l, [(6, 3, "#due-tre", Some(2)), (12, 2, "#manca", None)]);
        // un link che va a capo è cliccabile su entrambe le righe
        let pv = render_markdown("# Titolo\n\n[uno due tre](#titolo)\n", 8, None);
        let l: Vec<_> = pv.links.iter().map(|l| (l.line - pv.links[0].line, l.x, l.width, l.target)).collect();
        assert_eq!(l, [(0, 0, 7, Some(0)), (1, 0, 3, Some(0))]);
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
