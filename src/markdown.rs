//! Markdown → preview lines: wrapping, lists, quotes, big headings, images, links, code.

use std::{
    borrow::{Borrow, Cow},
    collections::HashMap,
    env,
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd, TextMergeWithOffset,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use regex::Regex;
use syntect::{
    highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet},
    parsing::{ParseState, ScopeStack, SyntaxSet},
};

use crate::{
    i18n::tr,
    images::{ImageSlot, Images},
};

const BULLETS: [&str; 3] = ["• ", "◦ ", "⊙ "];

/// Number of a numbered list item as shown on its nesting level: 1, 2, 3 on the first level, then
/// a, b, c, then i, ii, iii, and again from numbers. The source always uses digits (CommonMark has
/// no lettered lists); a list starting at 3 starts at c or iii.
fn ordinal(n: u64, level: usize) -> String {
    match level % 3 {
        0 => n.to_string(),
        1 if n > 0 => {
            // bijective base 26: z, aa, ab, …
            let (mut n, mut s) = (n, Vec::new());
            while n > 0 {
                n -= 1;
                s.push(b'a' + (n % 26) as u8);
                n /= 26;
            }
            s.iter().rev().map(|&b| b as char).collect()
        }
        2 if (1..4000).contains(&n) => {
            let mut n = n;
            let mut s = String::new();
            for (value, roman) in [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ] {
                while n >= value {
                    s.push_str(roman);
                    n -= value;
                }
            }
            s
        }
        // no letter or roman numeral for 0 (a list may start at 0) or beyond 3999
        _ => n.to_string(),
    }
}

fn code_style() -> Style {
    Style::new().fg(Color::Green)
}

fn link_style() -> Style {
    Style::new().fg(Color::Blue).add_modifier(Modifier::UNDERLINED)
}

fn border_style() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// Bare URLs that GitHub turns into links (GFM "extended autolinks"): `https://…`, `http://…` and
/// `www.…`, without trailing punctuation or an unbalanced closing parenthesis.
/// ponytail: no email autolinks and no domain validation, add them if they turn out to matter
fn bare_urls(text: &str) -> Vec<std::ops::Range<usize>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b(?:https?://|www\.)[^\s<]+").expect("valid regex"));
    re.find_iter(text)
        .filter_map(|m| {
            let mut url = m.as_str();
            loop {
                let unbalanced = url.ends_with(')') && url.matches(')').count() > url.matches('(').count();
                if url.ends_with(['?', '!', '.', ',', ':', ';', '*', '_', '~', '\'', '"']) || unbalanced {
                    url = &url[..url.len() - 1];
                } else {
                    break;
                }
            }
            let prefix = if url.starts_with("www.") {
                4
            } else {
                url.find("//")? + 2
            };
            (url.len() > prefix).then(|| m.start()..m.start() + url.len())
        })
        .collect()
}

/// Piece of text waiting to be wrapped: span, source row, link index.
type Pending = (Span<'static>, usize, Option<usize>);

/// Table being read: column alignments and, for each row (the header first), its source row and
/// the pieces of each cell. Drawn all at once at the end, when every column width is known.
struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<(usize, Vec<Vec<Pending>>)>,
}

/// Grammars and theme for code blocks, loaded on first use.
fn syntaxes() -> &'static (SyntaxSet, Theme) {
    static S: OnceLock<(SyntaxSet, Theme)> = OnceLock::new();
    S.get_or_init(|| {
        let mut themes = ThemeSet::load_defaults().themes;
        // if the theme gets renamed, use another bundled one: the empty default would give black text on dark gray
        let theme = themes
            .remove("base16-eighties.dark")
            .or_else(|| themes.into_values().next())
            .unwrap_or_default();
        (two_face::syntax::extra_newlines(), theme)
    })
}

/// Theme color for the terminal: 24-bit if it advertises them (COLORTERM, Windows Terminal),
/// otherwise the nearest one in the 256-color palette.
fn term_color(r: u8, g: u8, b: u8) -> Color {
    static TRUECOLOR: OnceLock<bool> = OnceLock::new();
    let truecolor = *TRUECOLOR.get_or_init(|| {
        env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit") || env::var_os("WT_SESSION").is_some()
    });
    if truecolor {
        Color::Rgb(r, g, b)
    } else {
        indexed(r, g, b)
    }
}

/// Nearest of the 6x6x6 cube (16..=231) and the gray ramp (232..=255) of the 256-color palette.
fn indexed(r: u8, g: u8, b: u8) -> Color {
    const STEPS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let near = |v: i32| (0..6).min_by_key(|&i| (STEPS[i] - v).abs()).unwrap_or(0);
    let (ri, gi, bi) = (near(r), near(g), near(b));
    let gray = (((r + g + b) / 3 - 8 + 5) / 10).clamp(0, 23);
    let dist = |cr: i32, cg: i32, cb: i32| (cr - r).pow(2) + (cg - g).pow(2) + (cb - b).pow(2);
    let gv = 8 + 10 * gray;
    if dist(gv, gv, gv) < dist(STEPS[ri], STEPS[gi], STEPS[bi]) {
        Color::Indexed(232 + gray as u8)
    } else {
        Color::Indexed((16 + 36 * ri + 6 * gi + bi) as u8)
    }
}

/// Code block background: the theme's, a dark gray slightly lighter than the terminal.
fn code_bg() -> Color {
    let bg = syntaxes().1.settings.background;
    bg.map_or(term_color(45, 45, 45), |c| term_color(c.r, c.g, c.b))
}

/// Highlighted code block, with the highlighter state after each line: when the block changes,
/// highlighting resumes from the first changed line instead of the start.
struct Highlighted {
    lang: String,
    code: String,
    lines: Vec<Vec<Span<'static>>>,
    states: Vec<(ParseState, HighlightState)>,
}

/// Blocks highlighted in the current and the previous frame: the preview is rebuilt every frame,
/// and whatever is no longer in the document leaves the cache after one round.
#[derive(Default)]
struct HighlightCache {
    cur: HashMap<u64, Arc<Highlighted>>,
    prev: HashMap<u64, Arc<Highlighted>>,
}

fn highlight_cache() -> MutexGuard<'static, HighlightCache> {
    static CACHE: OnceLock<Mutex<HighlightCache>> = OnceLock::new();
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Colored code lines for `lang` (name or extension, e.g. "rust", "py"); plain text if unknown.
fn highlight(lang: &str, code: &str) -> Arc<Highlighted> {
    let mut hasher = DefaultHasher::new();
    (lang, code).hash(&mut hasher);
    let key = hasher.finish();
    let mut cache = highlight_cache();
    let same = |b: &Highlighted| b.lang == lang && b.code == code;
    if let Some(b) = cache.cur.get(&key).filter(|b| same(b)) {
        return b.clone();
    }
    if let Some(b) = cache.prev.remove(&key).filter(|b| same(b)) {
        cache.cur.insert(key, b.clone());
        return b;
    }
    // changed block: keep the leading lines shared with the most similar cached version
    // ponytail: an edit at the top of a block re-highlights all of it; comparing states would avoid that
    let old = cache
        .cur
        .values()
        .chain(cache.prev.values())
        .filter(|b| b.lang == lang)
        .map(|b| (b.code.lines().zip(code.lines()).take_while(|(a, b)| a == b).count(), b))
        .max_by_key(|(n, _)| *n);
    let (mut lines, mut states) = old.map_or_else(Default::default, |(n, b)| {
        (b.lines[..n].to_vec(), b.states[..n].to_vec())
    });

    let (ss, theme) = syntaxes();
    // common ``` names the grammars do not recognize
    let token = match lang.to_lowercase().as_str() {
        "shell" | "console" | "shellsession" | "terminal" => "bash",
        "jsx" => "js",
        "csharp" | "c#" => "cs",
        "golang" => "go",
        _ => lang,
    };
    let syntax = ss
        .find_syntax_by_token(token)
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let highlighter = Highlighter::new(theme);
    let (mut parse, mut hl) = states.last().cloned().unwrap_or_else(|| {
        (
            ParseState::new(syntax),
            HighlightState::new(&highlighter, ScopeStack::new()),
        )
    });
    let bg = Style::new().bg(code_bg());
    for line in code.lines().skip(lines.len()) {
        let text = format!("{line}\n");
        let spans = match parse.parse_line(&text, ss) {
            Ok(ops) => HighlightIterator::new(&mut hl, &ops, &text, &highlighter)
                .map(|(st, text)| {
                    let mut style = bg.fg(term_color(st.foreground.r, st.foreground.g, st.foreground.b));
                    if st.font_style.contains(FontStyle::BOLD) {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if st.font_style.contains(FontStyle::ITALIC) {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    Span::styled(text.trim_end_matches('\n').to_string(), style)
                })
                .collect(),
            // line the highlighter cannot parse: keep it visible, without colors
            Err(_) => vec![Span::styled(line.to_string(), bg)],
        };
        lines.push(spans);
        states.push((parse.clone(), hl.clone()));
    }
    let block = Arc::new(Highlighted {
        lang: lang.to_string(),
        code: code.to_string(),
        lines,
        states,
    });
    cache.cur.insert(key, block.clone());
    block
}

/// Removes from the source what the terminal would interpret: control characters (ESC, BEL, ...)
/// become U+FFFD and tabs become spaces up to the next multiple of 4. Line numbers stay the same.
fn sanitize(src: &str) -> Cow<'_, str> {
    if !src.chars().any(|c| c.is_control() && c != '\n') {
        return Cow::Borrowed(src);
    }
    let mut out = String::with_capacity(src.len());
    let mut col = 0;
    for c in src.chars() {
        match c {
            '\n' => {
                out.push(c);
                col = 0;
            }
            '\t' => {
                let n = 4 - col % 4;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\r' => {}
            c => {
                out.push(if c.is_control() { '\u{FFFD}' } else { c });
                col += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// Code block being read: its text arrives in pieces and is highlighted all at once at the end.
struct CodeBlock {
    /// language given after ``` ("" if missing or for indented blocks)
    lang: String,
    /// fenced with ```: has a header row (with the language) and a closing row
    fenced: bool,
    text: String,
    /// source row of the first code line
    first: Option<usize>,
}

pub(crate) struct Preview {
    pub(crate) lines: Vec<Line<'static>>,
    /// for each line: the source row it comes from, and whether it has content (false = separator)
    src: Vec<(usize, bool)>,
    pub(crate) images: Vec<ImageSlot>,
    pub(crate) links: Vec<LinkHit>,
}

/// Piece of a clickable link on a preview line: internal (`[text](#heading)`) or external (see
/// `is_external`). A link that wraps has several, with the same `id`.
pub(crate) struct LinkHit {
    pub(crate) id: usize,
    pub(crate) line: usize,
    pub(crate) x: u16,
    pub(crate) width: u16,
    pub(crate) dest: String,
    /// internal links: source row of the target heading, None if it does not exist
    pub(crate) target: Option<usize>,
}

/// Links to open in the browser. Other schemes (file://, relative paths) are not clickable.
pub(crate) fn is_external(dest: &str) -> bool {
    ["http://", "https://", "mailto:"].iter().any(|p| dest.starts_with(p))
}

/// Heading anchor as GitHub generates it: lowercase, spaces → '-', punctuation removed.
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
    /// Preview row matching source row `row`: the first one with content among those coming
    /// from the nearest source row not past it.
    pub(crate) fn row_for(&self, row: usize) -> usize {
        let Some(best) = self.src.iter().map(|s| s.0).filter(|&s| s <= row).max() else {
            return 0;
        };
        let pos = self.src.iter().position(|&s| s == (best, true));
        pos.or_else(|| self.src.iter().position(|s| s.0 == best)).unwrap_or(0)
    }
}

/// The GitHub Flavored Markdown extensions, plus the ones github.com renders on top: footnotes and
/// alerts (`ENABLE_GFM`). Math (`ENABLE_MATH`) is left for the version that also draws Mermaid
/// diagrams: until then `$…$` stays plain text.
fn gfm_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
}

pub(crate) fn render_markdown(src: &str, width: u16, images: Option<&mut Images>) -> Preview {
    let src = sanitize(src);
    let src = src.as_ref();
    {
        let mut cache = highlight_cache();
        cache.prev = std::mem::take(&mut cache.cur);
    }
    let starts: Vec<usize> = std::iter::once(0)
        .chain(src.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |byte: usize| starts.partition_point(|&s| s <= byte) - 1;
    let mut r = Renderer {
        width: width as usize,
        images,
        blank_rows: src.lines().map(|l| l.trim().is_empty()).collect(),
        ..Renderer::default()
    };
    // the parser splits text at `_`, `(`, …: merged, a bare URL arrives in one piece
    for (ev, range) in TextMergeWithOffset::new(Parser::new_ext(src, gfm_options()).into_offset_iter()) {
        r.line = line_of(range.start);
        // row after the last one with content: some blocks (lists) include the blank lines after them
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
            let target = dest
                .strip_prefix('#')
                .and_then(|a| r.anchors.get(&a.to_lowercase()).copied());
            LinkHit {
                id: i,
                line,
                x,
                width,
                dest,
                target,
            }
        })
        .collect();
    Preview {
        lines: r.lines,
        src: r.src,
        images: r.slots,
        links,
    }
}

#[derive(Default)]
struct Renderer<'a> {
    lines: Vec<Line<'static>>,
    /// None (in tests): images show only their alt text
    images: Option<&'a mut Images>,
    slots: Vec<ImageSlot>,
    /// image being read: destination and alt text
    image: Option<(String, String)>,
    src: Vec<(usize, bool)>,
    /// source rows where the current event starts/ends
    line: usize,
    end_line: usize,
    /// text waiting to be wrapped, with the source row and the link (if any) of each piece
    spans: Vec<Pending>,
    styles: Vec<Style>,
    width: usize,
    /// for each open list: next number (None = bulleted) and text indent of the current item
    lists: Vec<(Option<u64>, usize)>,
    /// indent of the lines continuing the current list item
    indent: usize,
    /// the line being built starts with the list marker, which already acts as indent
    at_marker: bool,
    quote: usize,
    code: Option<CodeBlock>,
    heading: Option<(HeadingLevel, String)>,
    /// destinations of clickable links; `link` is the current one
    dests: Vec<String>,
    link: Option<usize>,
    /// link pieces on the produced lines: (line, column, width, index in `dests`)
    links: Vec<(usize, u16, u16, usize)>,
    /// heading anchor → source row; footnote definitions are stored as "^label"
    anchors: HashMap<String, usize>,
    /// for each source row: whether it is empty (or only whitespace)
    blank_rows: Vec<bool>,
    table: Option<Table>,
    /// inside `[…](…)`: bare URLs in the link text are not links of their own
    in_link: bool,
    /// inside an HTML comment, hidden as on GitHub
    comment: bool,
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::new()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        HeadingLevel::H2 => Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        _ => Style::new()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    }
}

/// Heading in block letters: H1 4 rows tall and 8 cells per letter, or 4 if it does not fit;
/// H2 4 rows tall and 4 wide. At most 2 rows of big text, otherwise None → normal heading.
fn big_heading(level: HeadingLevel, text: &str, width: usize) -> Option<Vec<String>> {
    // 8x8 font rows for each pixel row
    const FULL: &[&[usize]] = &[&[0], &[1], &[2], &[3], &[4], &[5], &[6], &[7]];
    // (horizontal pixels per cell, font rows)
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

/// 8x8 font → terminal rows of quadrant characters (2 vertical pixels per cell, `sx` = 1 or 2
/// horizontally). Uses only basic block characters, visible in every terminal.
fn big_text(text: &str, sx: usize, font_rows: &[&[usize]]) -> Vec<String> {
    use font8x8::{UnicodeFonts, BASIC_FONTS, LATIN_FONTS};
    const QUAD: [char; 16] = [
        ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
    ];
    let glyphs: Vec<[u8; 8]> = text
        .chars()
        .map(|c| {
            BASIC_FONTS
                .get(c)
                .or_else(|| LATIN_FONTS.get(c))
                .unwrap_or(BASIC_FONTS.get('?').unwrap())
        })
        .collect();
    let px = |x: usize, y: usize| {
        font_rows[y]
            .iter()
            .fold(0, |acc, &r| acc | (glyphs[x / 8][r] >> (x % 8)) & 1)
    };
    (0..font_rows.len() / 2)
        .map(|row| {
            (0..glyphs.len() * 8 / sx)
                .map(|col| {
                    let mut i = 0;
                    for dy in 0..2 {
                        // with sx=1 both columns coincide: the cell is full or empty across its width
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

/// Link piece on a line produced by `wrap`: (column, width, link index).
type LinkSpan = (u16, u16, usize);

/// Word wrap within `width`: the first line starts with `first`, the following ones with `cont`
/// (quote bar + list indent). Words longer than the line are split.
/// Each produced line carries the source row of its first character and the link pieces it
/// contains: (column, width, link).
fn wrap<S: Borrow<Span<'static>>>(
    spans: impl IntoIterator<Item = (S, usize, Option<usize>)>,
    width: usize,
    first: Vec<Span<'static>>,
    cont: Vec<Span<'static>>,
) -> Vec<(Line<'static>, usize, Vec<LinkSpan>)> {
    use unicode_width::UnicodeWidthChar;
    let cw = |c: char| c.width().unwrap_or(0);
    type Ch = (char, Style, usize, Option<usize>);
    let mut chars: Vec<Ch> = Vec::new();
    for (s, src, link) in spans {
        let s = s.borrow();
        chars.extend(s.content.chars().map(|c| (c, s.style, src, link)));
    }

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
        let mut hits: Vec<LinkSpan> = Vec::new();
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

    // leading spaces are kept only on the first line (e.g. the indent of a nested list marker)
    let mut wrapped = false;
    let mut i = 0;
    while i < chars.len() {
        let space = chars[i].0 == ' ';
        let j = (i..chars.len())
            .find(|&k| (chars[k].0 == ' ') != space)
            .unwrap_or(chars.len());
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
        let first = if std::mem::take(&mut self.at_marker) {
            vec![self.bar()]
        } else {
            vec![self.bar(), pad.clone()]
        };
        let spans = std::mem::take(&mut self.spans);
        for (line, src, hits) in wrap(spans, self.width, first, vec![self.bar(), pad]) {
            let n = self.lines.len();
            self.links.extend(hits.into_iter().map(|(x, w, l)| (n, x, w, l)));
            self.push_line(line, src, true);
        }
    }

    /// Blank separator line; inside a quote it keeps the bar, so the line stays continuous.
    fn blank(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            let sep = if self.quote > 0 {
                Line::from(self.bar())
            } else {
                Line::default()
            };
            self.push_line(sep, self.end_line, false);
        }
    }

    /// Code block on a lighter full-width background: header row with the language on the right
    /// (on the opening ```), highlighted code, blank row on the closing ```.
    /// Each code line wraps on its own, so preview and source stay aligned.
    fn code_block(&mut self, code: CodeBlock) {
        let bg = Style::new().bg(code_bg());
        let (bar, indent) = (self.bar(), " ".repeat(self.indent));
        let prefix = || vec![bar.clone(), Span::raw(indent.clone()), Span::styled("  ", bg)];
        let width = self.width;
        // `right` is aligned to the right edge; the background fills the rest of the line
        let fill = |mut spans: Vec<Span<'static>>, right: Option<Span<'static>>| {
            let used: usize = spans.iter().chain(&right).map(Span::width).sum();
            spans.push(Span::styled(" ".repeat(width.saturating_sub(used)), bg));
            spans.extend(right);
            Line::from(spans)
        };
        if code.fenced {
            let label = format!("{} ", code.lang);
            let label = Span::styled(label, bg.fg(Color::Gray).add_modifier(Modifier::ITALIC));
            let fits =
                !code.lang.is_empty() && prefix().iter().map(Span::width).sum::<usize>() + label.width() <= width;
            self.push_line(fill(prefix(), fits.then_some(label)), self.line, true);
        }
        let first = code.first.unwrap_or(self.line);
        let hl = highlight(&code.lang, &code.text);
        for (i, spans) in hl.lines.iter().enumerate() {
            let src = first + i;
            let rows = wrap(spans.iter().map(|s| (s, src, None)), width, prefix(), prefix());
            if rows.is_empty() {
                self.push_line(fill(prefix(), None), src, true);
            }
            for (line, src, _) in rows {
                self.push_line(fill(line.spans, None), src, true);
            }
        }
        if code.fenced {
            self.push_line(fill(prefix(), None), self.end_line.saturating_sub(1), true);
        }
    }

    /// After a paragraph, quote, code block or table: a blank separator row. Inside a list item the
    /// source decides, so preview and source rows stay aligned: a blank row only if the source has
    /// one there (a nested list or the next item can follow right away).
    fn end_block(&mut self) {
        if self.lists.is_empty() || self.quote > 0 || self.blank_after() {
            self.blank();
        } else {
            self.flush();
        }
    }

    /// The source row after the current event is empty.
    fn blank_after(&self) -> bool {
        self.blank_rows.get(self.end_line).copied().unwrap_or(false)
    }

    /// Plain text, with bare URLs turned into links (outside `[…](…)` links).
    fn text(&mut self, t: &str) {
        let style = self.style();
        let push = |r: &mut Self, s: &str, style: Style| {
            if !s.is_empty() {
                r.push_span(Span::styled(s.to_string(), style));
            }
        };
        let mut last = 0;
        for url in if self.in_link { Vec::new() } else { bare_urls(t) } {
            push(self, &t[last..url.start], style);
            let u = &t[url.clone()];
            self.dests.push(if u.starts_with("www.") {
                format!("http://{u}")
            } else {
                u.to_string()
            });
            self.link = Some(self.dests.len() - 1);
            push(self, u, style.patch(link_style()));
            self.link = None;
            last = url.end;
        }
        push(self, &t[last..], style);
    }

    /// Table with box-drawing borders, after the quote bar and the list indent. Columns get their
    /// widest cell, then the widest ones shrink until the table fits; cells wrap inside them.
    /// Rows map to their source rows; the separator under the header to the `|---|` row.
    fn draw_table(&mut self, t: Table) {
        let lead = vec![self.bar(), Span::raw(" ".repeat(self.indent))];
        let lead_w: usize = lead.iter().map(Span::width).sum();
        let ncols = t.rows.iter().map(|r| r.1.len()).max().unwrap_or(0).max(t.aligns.len());
        let (Some(first), Some(last)) = (t.rows.first().map(|r| r.0), t.rows.last().map(|r| r.0)) else {
            return;
        };
        let mut widths = vec![1; ncols];
        for (_, cells) in &t.rows {
            for (w, cell) in widths.iter_mut().zip(cells) {
                *w = (*w).max(cell.iter().map(|s| s.0.width()).sum());
            }
        }
        // "│ " + cells separated by " │ " + " │"
        let avail = self.width.saturating_sub(lead_w + 3 * ncols + 1);
        // ponytail: shrinks one cell at a time, plenty fast for tables that fit a terminal
        while widths.iter().sum::<usize>() > avail {
            match widths.iter_mut().max() {
                Some(w) if *w > 1 => *w -= 1,
                _ => break,
            }
        }
        let border = |l: &str, m: &str, r: &str| {
            let segs: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            let mut spans = lead.clone();
            spans.push(Span::styled(format!("{l}{}{r}", segs.join(m)), border_style()));
            Line::from(spans)
        };
        self.push_line(border("┌", "┬", "┐"), first, false);
        for (ri, (src, cells)) in t.rows.iter().enumerate() {
            let wrapped: Vec<_> = (0..ncols)
                .map(|i| {
                    let cell = cells.get(i).map_or(&[][..], |c| &c[..]);
                    wrap(cell.iter().map(|(s, r, l)| (s, *r, *l)), widths[i], vec![], vec![])
                })
                .collect();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(0).max(1);
            for k in 0..height {
                let n = self.lines.len();
                let mut spans = lead.clone();
                spans.push(Span::styled("│ ", border_style()));
                let mut x = lead_w + 2;
                for (i, rows) in wrapped.iter().enumerate() {
                    let (content, hits) = rows
                        .get(k)
                        .map_or((Vec::new(), Vec::new()), |(l, _, h)| (l.spans.clone(), h.clone()));
                    let w: usize = content.iter().map(Span::width).sum();
                    let pad = widths[i].saturating_sub(w);
                    let (left, right) = match t.aligns.get(i) {
                        Some(Alignment::Right) => (pad, 0),
                        Some(Alignment::Center) => (pad / 2, pad - pad / 2),
                        _ => (0, pad),
                    };
                    // link pieces are relative to the cell: move them to its column
                    let cx = (x + left) as u16;
                    self.links
                        .extend(hits.into_iter().map(|(hx, hw, l)| (n, cx + hx, hw, l)));
                    spans.push(Span::raw(" ".repeat(left)));
                    spans.extend(content);
                    spans.push(Span::raw(" ".repeat(right)));
                    spans.push(Span::styled(if i + 1 < ncols { " │ " } else { " │" }, border_style()));
                    x += left + w + right + 3;
                }
                self.push_line(Line::from(spans), *src, true);
            }
            if ri == 0 {
                self.push_line(border("├", "┼", "┤"), src + 1, true);
            }
        }
        self.push_line(border("└", "┴", "┘"), last, false);
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
                Tag::BlockQuote(kind) => {
                    self.flush();
                    self.quote += 1;
                    self.push_style(Style::new().add_modifier(Modifier::ITALIC));
                    // GitHub alert: its title on the `> [!NOTE]` row
                    if let Some(kind) = kind {
                        let (i, color) = match kind {
                            BlockQuoteKind::Note => (0, Color::Blue),
                            BlockQuoteKind::Tip => (1, Color::Green),
                            BlockQuoteKind::Important => (2, Color::Magenta),
                            BlockQuoteKind::Warning => (3, Color::Yellow),
                            BlockQuoteKind::Caution => (4, Color::Red),
                        };
                        let style = Style::new().fg(color).add_modifier(Modifier::BOLD);
                        self.push_span(Span::styled(tr().alerts[i], style));
                        self.flush();
                    }
                }
                Tag::CodeBlock(kind) => {
                    self.flush();
                    let (lang, fenced) = match kind {
                        // "rust,ignore" (rustdoc) and "{.python .numberLines}" (pandoc): only the first name counts
                        CodeBlockKind::Fenced(info) => {
                            let lang = info
                                .trim_start_matches(['{', '.'])
                                .split(|c: char| c == ',' || c == '}' || c.is_whitespace())
                                .next()
                                .unwrap_or("");
                            (lang.to_string(), true)
                        }
                        CodeBlockKind::Indented => (String::new(), false),
                    };
                    self.code = Some(CodeBlock {
                        lang,
                        fenced,
                        text: String::new(),
                        first: None,
                    });
                }
                Tag::List(start) => {
                    self.flush();
                    self.lists.push((start, 0));
                }
                Tag::Item => {
                    self.flush();
                    // sub-items align with the text of the parent item
                    let depth = self.lists.len().saturating_sub(1);
                    let indent = " ".repeat(self.lists.iter().rev().nth(1).map_or(0, |l| l.1));
                    // nesting level among the numbered lists only, so a bulleted list in between doesn't count
                    let level = self.lists.iter().filter(|l| l.0.is_some()).count().saturating_sub(1);
                    let marker = match self.lists.last_mut() {
                        Some((Some(n), _)) => {
                            *n += 1;
                            format!("{}. ", ordinal(*n - 1, level))
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
                    if dest_url.starts_with('#') || is_external(&dest_url) {
                        self.dests.push(dest_url.to_string());
                        self.link = Some(self.dests.len() - 1);
                    }
                    self.in_link = true;
                    self.push_style(link_style());
                }
                Tag::FootnoteDefinition(label) => {
                    // in place, not moved to the end as on GitHub: preview and source stay aligned
                    self.flush();
                    self.anchors.insert(format!("^{}", label.to_lowercase()), self.line);
                    let marker = Span::styled(format!("[{label}]: "), Style::new().fg(Color::Yellow));
                    self.indent = marker.width();
                    self.at_marker = true;
                    self.push_span(marker);
                }
                Tag::Table(aligns) => {
                    self.flush();
                    self.table = Some(Table {
                        aligns,
                        rows: Vec::new(),
                    });
                }
                Tag::TableHead | Tag::TableRow => {
                    if matches!(tag, Tag::TableHead) {
                        self.push_style(Style::new().add_modifier(Modifier::BOLD));
                    }
                    let line = self.line;
                    if let Some(t) = &mut self.table {
                        t.rows.push((line, Vec::new()));
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    self.styles.pop();
                    if let Some((level, text)) = self.heading.take() {
                        // duplicate headings: "-1", "-2", ... as on GitHub
                        let base = slug(&text);
                        let (mut anchor, mut n) = (base.clone(), 0);
                        while self.anchors.contains_key(&anchor) {
                            n += 1;
                            anchor = format!("{base}-{n}");
                        }
                        self.anchors.insert(anchor, self.line);
                        if let Some(rows) = big_heading(level, &text, width) {
                            self.spans.clear();
                            let style = Style {
                                fg: heading_style(level).fg,
                                ..Style::default()
                            };
                            for r in rows {
                                self.push_line(Line::styled(r, style), self.line, true);
                            }
                        }
                    }
                    self.blank();
                }
                TagEnd::Paragraph => self.end_block(),
                TagEnd::BlockQuote(_) => {
                    self.styles.pop();
                    self.flush();
                    // drop the bar-only line left by the last paragraph, then separate at the outer level
                    if self
                        .lines
                        .last()
                        .is_some_and(|l| l.spans.len() == 1 && l.spans[0].content == self.bar().content)
                    {
                        self.lines.pop();
                        self.src.pop();
                    }
                    self.quote -= 1;
                    self.end_block();
                }
                TagEnd::CodeBlock => {
                    if let Some(code) = self.code.take() {
                        self.code_block(code);
                    }
                    self.end_block();
                }
                TagEnd::List(_) => {
                    self.flush();
                    self.lists.pop();
                    self.indent = self.lists.last().map_or(0, |l| l.1);
                    if self.lists.is_empty() {
                        self.blank();
                    }
                }
                // blank row between the items of a loose list, as in the source
                TagEnd::Item if self.blank_after() => self.blank(),
                TagEnd::Item => self.flush(),
                TagEnd::Image => {
                    let Some((dest, alt)) = self.image.take() else { return };
                    // after the quote bar and the list item indent
                    let x = (2 * self.quote + self.indent) as u16;
                    let avail = (self.width as u16).saturating_sub(x);
                    // in a table cell an image would break the rows: only its alt text
                    let in_table = self.table.is_some();
                    match self
                        .images
                        .as_deref_mut()
                        .filter(|_| !in_table)
                        .and_then(|im| im.size(&dest, avail))
                    {
                        Some((path, cols, rows)) => {
                            self.flush();
                            let line = self.lines.len();
                            for _ in 0..rows {
                                self.push_line(Line::from(self.bar()), self.line, true);
                            }
                            self.slots.push(ImageSlot {
                                line,
                                x,
                                cols,
                                rows,
                                path,
                            });
                        }
                        None => {
                            let style = Style::new().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
                            self.push_span(Span::styled(format!("[{}: {alt}]", tr().image), style));
                        }
                    }
                }
                TagEnd::Link => {
                    self.link = None;
                    self.in_link = false;
                    self.styles.pop();
                }
                TagEnd::FootnoteDefinition => {
                    self.flush();
                    self.indent = self.lists.last().map_or(0, |l| l.1);
                }
                TagEnd::TableHead => {
                    self.styles.pop();
                }
                TagEnd::TableCell => {
                    let cell = std::mem::take(&mut self.spans);
                    if let Some((_, cells)) = self.table.as_mut().and_then(|t| t.rows.last_mut()) {
                        cells.push(cell);
                    }
                }
                TagEnd::Table => {
                    if let Some(t) = self.table.take() {
                        self.draw_table(t);
                    }
                    self.end_block();
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    self.styles.pop();
                }
                _ => {}
            },
            Event::Text(t) if self.code.is_some() => {
                let line = self.line;
                if let Some(code) = &mut self.code {
                    code.first.get_or_insert(line);
                    code.text.push_str(&t);
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
            Event::Text(t) => self.text(&t),
            Event::Code(t) => self.push_span(Span::styled(t.into_string(), code_style())),
            // not emitted: ENABLE_MATH is off (see `gfm_options`)
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
            Event::FootnoteReference(label) => {
                self.dests.push(format!("#^{label}"));
                let outer = self.link.replace(self.dests.len() - 1);
                self.push_span(Span::styled(format!("[{label}]"), link_style()));
                self.link = outer;
            }
            // a comment can arrive in several pieces (one per line in an HTML block)
            Event::Html(t) | Event::InlineHtml(t) if self.comment || t.trim_start().starts_with("<!--") => {
                self.comment = !t.contains("-->");
            }
            Event::Html(t) | Event::InlineHtml(t) => self.push_span(Span::raw(t.trim_end().to_string())),
            Event::SoftBreak => self.push_span(Span::raw(" ")),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                // inside quotes and list items: after the bar and the indent, like the text
                let mut spans = vec![self.bar(), Span::raw(" ".repeat(self.indent))];
                let used: usize = spans.iter().map(Span::width).sum();
                spans.push(Span::styled("─".repeat(width.saturating_sub(used)), border_style()));
                self.push_line(Line::from(spans), self.line, true);
                self.blank();
            }
            Event::TaskListMarker(done) => {
                // two spaces: some terminals draw the checkbox wider than one cell, over a single space
                let check = if done { "☑  " } else { "☐  " };
                if let Some((s, ..)) = self.spans.last_mut() {
                    s.content = match BULLETS.iter().find(|b| s.content.ends_with(*b)) {
                        // bulleted item: the checkbox replaces the bullet
                        Some(b) => format!("{}{check}", &s.content[..s.content.len() - b.len()]),
                        // numbered item: the checkbox goes after the number
                        None => format!("{}{check}", s.content),
                    }
                    .into();
                    // continuation lines and sub-items align with the task's text
                    self.indent = s.width();
                    if let Some(l) = self.lists.last_mut() {
                        l.1 = self.indent;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_image::picker::Picker;

    fn plain(src: &str, width: u16) -> Vec<String> {
        plain_lines(&render_markdown(src, width, None))
    }

    #[test]
    fn renders_basic_elements() {
        let src = "### Title\n\n- a\n  - b\n- [x] done\n- [ ] to do\n\n1. one\n2. two\n\n> quote\n\n---\n\n```\nx = 1\n\ny\n```\n";
        assert_eq!(
            plain(src, 40),
            [
                "Title",
                "",
                "• a",
                "  ◦ b",
                "☑  done",
                "☐  to do",
                "",
                "1. one",
                "2. two",
                "",
                "│ quote",
                "",
                "─".repeat(40).as_str(),
                "",
                // header (no language), code, closing row
                "",
                "  x = 1",
                "",
                "  y",
                "",
                "",
            ]
        );
    }

    #[test]
    fn wraps_with_prefix() {
        // quote: every wrapped line repeats the bar, paragraphs stay joined by the bar
        assert_eq!(
            plain("> one two three seven\n", 12),
            ["│ one two", "│ three", "│ seven", ""]
        );
        assert_eq!(
            plain("> a\n>\n> b\n\noutside\n", 40),
            ["│ a", "│ ", "│ b", "", "outside", ""]
        );
        assert_eq!(
            plain("> a\n>> b\n", 40),
            ["│ a", "│ ", "│ │ b", "", ""].map(String::from)[..4].to_vec()
        );
        // list: continuation lines indent under the text, not under the bullet
        assert_eq!(plain("- one two three\n- x\n", 9), ["• one two", "  three", "• x", ""]);
        // different bullet per level, sub-items aligned with the parent item's text
        assert_eq!(
            plain("* a\n   * b\n      * c\n         * d\n", 40),
            ["• a", "  ◦ b", "    ⊙ c", "      • d", ""]
        );
        // numbers, then letters, then roman numerals by level; sub-items under the text of "2. "
        assert_eq!(
            plain("1. a\n2. b\n   1. c\n   2. d\n      1. e\n      2. f\n", 40),
            ["1. a", "2. b", "   a. c", "   b. d", "      i. e", "      ii. f", ""]
        );
        // a bulleted list in between doesn't count as a numbered level
        assert_eq!(
            plain("1. a\n   - b\n     1. c\n", 40),
            ["1. a", "   ◦ b", "     a. c", ""]
        );
        assert_eq!(plain("- a\n  - [x] b\n", 40), ["• a", "  ☑  b", ""]);
        // bulleted task: wrapped text and sub-items align with the task's text
        assert_eq!(plain("- [ ] aa bb\n  - c\n", 7), ["☐  aa", "   bb", "   ◦ c", ""]);
        // numbered task: checkbox after the number, wrapped text aligned with the task text
        assert_eq!(
            plain("1. [ ] aa bb\n2. [x] c\n", 10),
            ["1. ☐  aa", "      bb", "2. ☑  c", ""]
        );
        // word longer than the line: split
        assert_eq!(plain("abcdef\n", 4), ["abcd", "ef", ""]);
    }

    #[test]
    fn loose_lists_keep_source_rows() {
        // a blank row between items makes the list "loose": item texts become paragraphs, but a
        // nested list still follows its parent right away, and blank rows match the source
        let pv = render_markdown("- one\n    - two\n        - three\n\n- four\n", 40, None);
        assert_eq!(plain_lines(&pv), ["• one", "  ◦ two", "    ⊙ three", "", "• four", ""]);
        assert_eq!(pv.src.iter().map(|s| s.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);
        // the same for quotes and code blocks inside an item
        assert_eq!(
            plain("- a\n  > q\n- b\n\n- c\n", 40),
            ["• a", "│   q", "• b", "", "• c", ""]
        );
        // paragraphs of one item separated by a blank row keep it
        assert_eq!(plain("- a\n\n  b\n- c\n", 40), ["• a", "", "  b", "• c", ""]);
    }

    #[test]
    fn renders_tables() {
        let src = "| Name | Qty | Notes |\n|:--|--:|:-:|\n| apple | 3 | [x](#name) |\n| kiwi fruit | 12 |\n";
        let pv = render_markdown(src, 40, None);
        assert_eq!(
            plain_lines(&pv),
            [
                "┌────────────┬─────┬───────┐",
                "│ Name       │ Qty │ Notes │",
                "├────────────┼─────┼───────┤",
                "│ apple      │   3 │   x   │",
                "│ kiwi fruit │  12 │       │",
                "└────────────┴─────┴───────┘",
                "",
            ]
        );
        // each row on its own source row, the separator on the |---| row
        assert_eq!(
            pv.src,
            [
                (0, false),
                (0, true),
                (1, true),
                (2, true),
                (3, true),
                (3, false),
                (4, false)
            ]
        );
        // the link inside a cell is clickable where it is drawn
        let link = &pv.links[0];
        assert_eq!((link.line, link.x, link.width), (3, 23, 1));
        // too narrow: widest columns shrink and cells wrap, without panicking
        assert_eq!(
            plain("| a | b |\n|-|-|\n| hello | wo |\n", 11),
            [
                "┌────┬────┐",
                "│ a  │ b  │",
                "├────┼────┤",
                "│ he │ wo │",
                "│ ll │    │",
                "│ o  │    │",
                "└────┴────┘",
                "",
            ]
        );
        assert_eq!(
            plain("| a | b | c | d |\n|-|-|-|-|\n| hello | world | foo | bar |\n", 6).len(),
            10
        );
        // inside a quote and a list item: after the bar and the indent
        assert_eq!(
            plain("> | a |\n> |---|\n> | b |\n", 20),
            ["│ ┌───┐", "│ │ a │", "│ ├───┤", "│ │ b │", "│ └───┘", ""]
        );
        assert_eq!(
            plain("- x\n\n  | a |\n  |---|\n", 20)[2..5],
            ["  ┌───┐", "  │ a │", "  ├───┤"]
        );
    }

    #[test]
    fn renders_github_extensions() {
        // footnotes stay where they are written, and the reference jumps to the definition
        let pv = render_markdown("Text[^1].\n\n[^1]: The note, long enough to wrap.\n", 24, None);
        assert_eq!(
            plain_lines(&pv),
            ["Text[1].", "", "[1]: The note, long", "     enough to wrap.", ""]
        );
        assert_eq!((pv.links[0].dest.as_str(), pv.links[0].target), ("#^1", Some(2)));
        // alerts: the title (translated) on the [!NOTE] row
        let title = format!("│ {}", tr().alerts[3]);
        assert_eq!(
            plain("> [!WARNING]\n> Be careful.\n", 30),
            [title.as_str(), "│ Be careful.", ""]
        );
        // HTML comments are hidden, inline and as blocks
        assert_eq!(
            plain("a <!-- x --> b\n\n<!--\nblock\n-->\n\nafter\n", 30),
            ["a  b", "", "after", ""]
        );
        // a rule inside a quote keeps the bar
        assert_eq!(plain("> a\n>\n> ---\n", 8), ["│ a", "│ ", "│ ──────", ""]);
        // bare URLs are links, but not inside the text of a link
        let pv = render_markdown("See www.rust-lang.org, or [https://a.io](https://b.io).\n", 80, None);
        let dests: Vec<_> = pv.links.iter().map(|l| l.dest.as_str()).collect();
        assert_eq!(dests, ["http://www.rust-lang.org", "https://b.io"]);
    }

    #[test]
    fn finds_bare_urls() {
        let urls = |t: &str| bare_urls(t).into_iter().map(|r| t[r].to_string()).collect::<Vec<_>>();
        assert_eq!(urls("go to https://example.com/a_(b))."), ["https://example.com/a_(b)"]);
        assert_eq!(urls("www.x.org, http://y.io?"), ["www.x.org", "http://y.io"]);
        assert_eq!(urls("(see https://z.io/p)"), ["https://z.io/p"]);
        // a bare prefix or one inside a word is not a link
        assert!(urls("https:// and awww.x.org").is_empty());
    }

    #[test]
    fn ordinals_by_level() {
        assert_eq!(ordinal(12, 0), "12");
        assert_eq!(
            [1, 26, 27, 52, 703].map(|n| ordinal(n, 1)),
            ["a", "z", "aa", "az", "aaa"]
        );
        assert_eq!(
            [1, 4, 9, 14, 1994].map(|n| ordinal(n, 2)),
            ["i", "iv", "ix", "xiv", "mcmxciv"]
        );
        assert_eq!(ordinal(3, 3), "3");
        assert_eq!((ordinal(0, 1), ordinal(4000, 2)), ("0".into(), "4000".into()));
    }

    #[test]
    fn source_rows_map_to_preview_rows() {
        let src = "# Ab\ntext\n\n- a\n- b\n\n```\nx\n```\n\n> q1\n> q2\nend";
        let pv = render_markdown(src, 40, None);
        let text = |i: usize| plain_lines(&pv)[i].clone();
        // big heading: its source row points at the first of the 4 block rows
        assert_eq!(pv.row_for(0), 0);
        assert_eq!(text(pv.row_for(1)), "text");
        assert_eq!(text(pv.row_for(2)), "");
        assert_eq!(text(pv.row_for(3)), "• a");
        assert_eq!(text(pv.row_for(4)), "• b");
        assert_eq!(text(pv.row_for(5)), "");
        // the opening and closing ``` rows each have their own preview row
        assert_eq!(pv.row_for(6) + 1, pv.row_for(7));
        assert_eq!(text(pv.row_for(7)), "  x");
        assert_eq!(pv.row_for(8), pv.row_for(7) + 1);
        // lines joined into one paragraph point at the line containing them
        assert_eq!(text(pv.row_for(11)), "│ q1 q2 end");
        assert_eq!(pv.row_for(12), pv.row_for(10));
    }

    #[test]
    fn images_reserve_rows_or_show_alt_text() {
        let mut im = Images::new(Picker::halfblocks(), "test-docs".into());
        im.max_rows = 10;
        // halfblocks: 10x20 px cells. panorama 640x320 in 40 columns → 400x200 px → 10 rows
        let src = "before\n\n![pano](img/panorama.png)\n\nafter";
        let pv = render_markdown(src, 40, Some(&mut im));
        assert_eq!(pv.images.len(), 1);
        let s = &pv.images[0];
        assert_eq!((s.line, s.x, s.cols, s.rows), (2, 0, 40, 10));
        assert_eq!(pv.row_for(2), 2);
        assert_eq!(
            pv.lines[s.line + s.rows as usize + 1]
                .spans
                .iter()
                .map(|x| x.content.as_ref())
                .collect::<String>(),
            "after"
        );
        // tall image: limited to max_rows, width scaled down to match (240x480 → 10 rows, 10 columns)
        let pv = render_markdown("![s](img/stripes.jpg)", 40, Some(&mut im));
        assert_eq!((pv.images[0].cols, pv.images[0].rows), (10, 10));
        // inside a quote it starts after the bar
        let pv = render_markdown("> ![s](img/stripes.jpg)", 40, Some(&mut im));
        assert_eq!(pv.images[0].x, 2);
        // missing or remote: alt text only
        let pv = render_markdown("![missing](img/no.png) ![web](https://x.org/a.png)", 80, Some(&mut im));
        assert!(pv.images.is_empty());
        let i = tr().image;
        assert_eq!(plain_lines(&pv)[0], format!("[{i}: missing] [{i}: web]"));
        // encoding of the visible part of an image cut by scrolling
        let pv = render_markdown("![pano](img/panorama.png)", 40, Some(&mut im));
        assert!(im.protocol(&pv.images[0], 3, 4).is_some());
    }

    fn plain_lines(pv: &Preview) -> Vec<String> {
        // code blocks pad their lines with colored spaces: only visible characters count there
        pv.lines
            .iter()
            .map(|l| {
                let t: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                if l.spans.iter().any(|s| s.style.bg == Some(code_bg())) {
                    t.trim_end().to_string()
                } else {
                    t
                }
            })
            .collect()
    }

    #[test]
    fn code_blocks_are_highlighted() {
        let pv = render_markdown("```rust\nfn main() {}\n```\n\n```unknown\nx\n```\n", 30, None);
        let bg = code_bg();
        // header with the language on the right, everything on the full-width block background
        assert_eq!(plain_lines(&pv)[0], format!("{}rust", " ".repeat(25)));
        for l in &pv.lines[..3] {
            assert_eq!(l.width(), 30);
            assert!(l.spans.iter().filter(|s| s.width() > 0).all(|s| s.style.bg == Some(bg)));
        }
        // "fn" has a different color from "main": highlighted
        let fg = |w: &str| {
            pv.lines[1]
                .spans
                .iter()
                .find(|s| s.content.contains(w))
                .and_then(|s| s.style.fg)
        };
        assert_ne!(fg("fn"), fg("main"));
        // unknown language: plain text, but the label stays
        assert!(plain_lines(&pv)[4].ends_with("unknown"));
    }

    #[test]
    fn code_blocks_keep_quote_bar_and_list_indent() {
        let pv = render_markdown("> ```\n> x\n> ```\n\n- a\n\n  ```\n  y\n  ```\n", 30, None);
        let text = |i: usize| plain_lines(&pv)[i].clone();
        assert_eq!(text(pv.row_for(1)), "│   x");
        assert_eq!(text(pv.row_for(0)), "│");
        assert_eq!(text(pv.row_for(7)), "    y");
        // ``` rows get their own preview row inside quotes and lists too
        for code in [1, 7] {
            assert_eq!(pv.row_for(code - 1) + 1, pv.row_for(code));
            assert_eq!(pv.row_for(code + 1), pv.row_for(code) + 1);
        }
    }

    #[test]
    fn code_info_strings_and_incremental_highlight() {
        let pv = plain(
            "```rust,ignore\nfn a() {}\n```\n\n```{.python .numberLines}\nx = 1\n```\n",
            30,
        );
        assert!(pv[0].ends_with(" rust") && pv[4].ends_with(" python"), "{pv:?}");
        // resuming from already highlighted leading lines gives the same result as starting over
        // ("rs" is a different cache key: nothing is reused)
        highlight("rust", "fn a() {\n    let s = \"x\";\n}\n");
        let code = "fn a() {\n    let s = \"x\";\n    /* c\n    d */ 1\n}\n";
        assert_eq!(highlight("rust", code).lines, highlight("rs", code).lines);
        assert_eq!(indexed(45, 45, 45), Color::Indexed(236));
        assert_eq!(indexed(255, 0, 0), Color::Indexed(196));
    }

    #[test]
    fn control_chars_and_tabs_never_reach_the_terminal() {
        let pv = render_markdown("```\nA\x1b]0;x\x07B\n\tC\n```\n\nX\x1b[2JY\ta\r\n", 30, None);
        assert!(pv
            .lines
            .iter()
            .flat_map(|l| &l.spans)
            .all(|s| !s.content.contains(char::is_control)));
        let t = plain_lines(&pv);
        assert_eq!(t[1..3], ["  A\u{FFFD}]0;x\u{FFFD}B", "      C"]);
        assert_eq!(t[5], "X\u{FFFD}[2JY  a");
    }

    #[test]
    fn links_are_clickable() {
        let pv = render_markdown(
            "# One\n\n## Two, three!\n\ngo to [here](#two-three) or [there](#missing) or [web](http://x.org) or [file](a.md)\n",
            60,
            None,
        );
        let l: Vec<_> = pv
            .links
            .iter()
            .map(|l| (l.x, l.width, l.dest.as_str(), l.target))
            .collect();
        // relative paths are not clickable
        assert_eq!(
            l,
            [
                (6, 4, "#two-three", Some(2)),
                (14, 5, "#missing", None),
                (23, 3, "http://x.org", None)
            ]
        );
        // a link that wraps is clickable on both lines
        let pv = render_markdown("# Title\n\n[one two three](#title)\n", 8, None);
        let l: Vec<_> = pv
            .links
            .iter()
            .map(|l| (l.line - pv.links[0].line, l.x, l.width, l.target, l.id))
            .collect();
        assert_eq!(l, [(0, 0, 7, Some(0), 0), (1, 0, 5, Some(0), 0)]);
    }

    #[test]
    fn big_headings() {
        // wide H1: 8 cells per letter, 4 rows
        let out = plain("# Ab\n", 80);
        assert_eq!(out.len(), 5);
        assert!(out[..4].iter().all(|r| r.chars().count() == 16));
        assert!(out[0].contains('█') || out[0].contains('▀') || out[0].contains('▄'));
        // H1 in a narrow space → compact 4-cell version
        let out = render_markdown("# Abcdef\n", 30, None).lines;
        assert_eq!(out[0].spans[0].content.chars().count(), 24);
        // compact H2, H3 as normal text
        let out = plain("## Ab\n", 80);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].chars().count(), 8);
        assert_eq!(plain("### Ab\n", 80), ["Ab", ""]);
        // heading that does not fit even compact → normal text
        assert_eq!(plain("# Title\n", 7)[0], "Title");
    }
}
