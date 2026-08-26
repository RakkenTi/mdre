//! GitHub-Flavored Markdown → styled terminal lines.
//!
//! The renderer walks `pulldown-cmark` events while maintaining a stack of
//! *block prefixes* (quote bars, list markers, footnote labels). Every emitted
//! line re-applies the prefix, which is what makes arbitrarily nested
//! quotes-inside-lists-inside-quotes lay out correctly.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, MetadataBlockKind, Options,
    Parser, Tag, TagEnd,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::md::inline::{self, spans_width, str_width};
use crate::theme::Theme;
use tohki;
use tohki::{Tok};

#[derive(Clone, Copy, Debug)]
pub struct RenderOpts {
    /// Append `(https://…)` after link text.
    pub show_urls: bool,
    /// Number the lines inside fenced code blocks.
    pub code_numbers: bool,
    /// Show the literal `##` markers before headings, dimmed, as an explicit
    /// level cue alongside colour and weight.
    pub heading_markers: bool,
    /// Maximum text column width; content is centered in wider viewports.
    pub max_width: u16,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            show_urls: true,
            code_numbers: false,
            heading_markers: true,
            max_width: 100,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TocEntry {
    pub level: u8,
    pub title: String,
    pub line: usize,
    pub src_line: usize,
}

#[derive(Clone, Debug)]
pub struct LinkEntry {
    pub label: String,
    pub url: String,
    pub line: usize,
}

#[derive(Default)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    pub toc: Vec<TocEntry>,
    pub links: Vec<LinkEntry>,
    /// Source line index for each rendered line (for preview↔editor sync).
    pub src_line: Vec<usize>,
    pub words: usize,
    pub width: u16,
}

impl Rendered {
    /// First rendered line that covers a given source line — the place to
    /// scroll to when syncing the preview with the editor cursor.
    pub fn line_for_src(&self, src: usize) -> usize {
        self.src_line
            .iter()
            .position(|s| *s >= src)
            .unwrap_or_else(|| self.lines.len().saturating_sub(1))
    }
}

struct Ctx {
    first: Vec<Span<'static>>,
    cont: Vec<Span<'static>>,
    used: bool,
    width: usize,
}

struct ListCtx {
    ordered: Option<u64>,
    index: u64,
}

struct TableCtx {
    aligns: Vec<Alignment>,
    head: Vec<Vec<Span<'static>>>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    row: Vec<Vec<Span<'static>>>,
    in_head: bool,
}

struct CodeCtx {
    info: String,
    text: String,
}

pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_MATH
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
}

pub fn render(src: &str, width: u16, theme: &Theme, opts: RenderOpts) -> Rendered {
    let width = width.max(8).min(opts.max_width.max(20)) as usize;
    let mut r = Renderer {
        theme,
        opts,
        width,
        out: Rendered {
            width: width as u16,
            ..Default::default()
        },
        ctx: Vec::new(),
        inline: Vec::new(),
        styles: vec![Style::default().fg(theme.fg).bg(theme.bg)],
        lists: Vec::new(),
        table: None,
        code: None,
        html: None,
        meta: None,
        heading: None,
        heading_text: String::new(),
        link: None,
        footnotes: HashMap::new(),
        footnote_next: 1,
        pending_blank: false,
        cur_src: 0,
        line_starts: line_starts(src),
    };

    let parser = Parser::new_ext(src, options());
    for (event, range) in parser.into_offset_iter() {
        r.cur_src = r.src_line_of(range.start);
        r.event(event, range);
    }
    r.flush_paragraph();
    while r.out.lines.last().is_some_and(|l| is_blank(l)) {
        r.out.lines.pop();
        r.out.src_line.pop();
    }
    r.out.words = src.split_whitespace().filter(|w| !w.is_empty()).count();
    r.out
}

fn is_blank(line: &Line<'_>) -> bool {
    line.spans
        .iter()
        .all(|s| s.content.chars().all(char::is_whitespace))
}

fn line_starts(src: &str) -> Vec<usize> {
    let mut v = vec![0usize];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

struct Renderer<'a> {
    theme: &'a Theme,
    opts: RenderOpts,
    width: usize,
    out: Rendered,
    ctx: Vec<Ctx>,
    inline: Vec<Span<'static>>,
    styles: Vec<Style>,
    lists: Vec<ListCtx>,
    table: Option<TableCtx>,
    code: Option<CodeCtx>,
    html: Option<String>,
    meta: Option<String>,
    heading: Option<u8>,
    heading_text: String,
    link: Option<(String, usize)>,
    footnotes: HashMap<String, usize>,
    footnote_next: usize,
    pending_blank: bool,
    cur_src: usize,
    line_starts: Vec<usize>,
}

impl<'a> Renderer<'a> {
    fn src_line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    fn style(&self) -> Style {
        *self.styles.last().unwrap()
    }

    fn push_style(&mut self, f: impl FnOnce(Style) -> Style) {
        let s = f(self.style());
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn prefix_width(&self) -> usize {
        self.ctx.iter().map(|c| c.width).sum()
    }

    fn avail(&self) -> usize {
        self.width.saturating_sub(self.prefix_width()).max(4)
    }

    fn take_prefix(&mut self) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        for c in self.ctx.iter_mut() {
            if c.used {
                out.extend(c.cont.iter().cloned());
            } else {
                out.extend(c.first.iter().cloned());
                c.used = true;
            }
        }
        out
    }

    fn push_line(&mut self, spans: Vec<Span<'static>>) {
        let line = Line::from(spans).style(Style::default().fg(self.theme.fg).bg(self.theme.bg));
        self.out.lines.push(line);
        self.out.src_line.push(self.cur_src);
    }

    /// Emit one already-laid-out line, prefixed by the active block context.
    fn emit(&mut self, spans: Vec<Span<'static>>) {
        let mut p = self.take_prefix();
        p.extend(spans);
        self.push_line(p);
    }

    fn emit_wrapped(&mut self, spans: &[Span<'static>]) {
        let avail = self.avail();
        for line in inline::wrap(spans, avail) {
            self.emit(line);
        }
    }

    fn blank(&mut self) {
        // A blank line still shows quote bars / list indentation, and belongs
        // to the block it follows so preview↔editor sync lands on real content.
        let p: Vec<Span<'static>> = self.ctx.iter().flat_map(|c| c.cont.clone()).collect();
        let src = self.out.src_line.last().copied().unwrap_or(self.cur_src);
        let line = Line::from(p).style(Style::default().fg(self.theme.fg).bg(self.theme.bg));
        self.out.lines.push(line);
        self.out.src_line.push(src);
    }

    fn want_blank(&mut self) {
        self.pending_blank = true;
    }

    fn resolve_blank(&mut self) {
        if self.pending_blank {
            self.pending_blank = false;
            if !self.out.lines.is_empty() {
                self.blank();
            }
        }
    }

    fn push_ctx(&mut self, first: Vec<Span<'static>>, cont: Vec<Span<'static>>) {
        let mut first = first;
        let mut cont = cont;
        let fw = spans_width(&first);
        let cw = spans_width(&cont);
        let width = fw.max(cw);
        // Keep both variants the same width so wrapping math stays honest.
        inline::pad_to(&mut first, width, Style::default().bg(self.theme.bg));
        inline::pad_to(&mut cont, width, Style::default().bg(self.theme.bg));
        self.ctx.push(Ctx {
            first,
            cont,
            used: false,
            width,
        });
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => {
                let st = Style::default()
                    .fg(self.theme.inline_code_fg)
                    .bg(self.theme.inline_code_bg);
                self.inline.push(Span::styled(t.to_string(), st));
                if self.heading.is_some() {
                    self.heading_text.push_str(&t);
                }
            }
            Event::InlineMath(t) => {
                let st = Style::default()
                    .fg(self.theme.math)
                    .bg(self.theme.bg)
                    .add_modifier(Modifier::ITALIC);
                self.inline.push(Span::styled(format!("{t}"), st));
            }
            Event::DisplayMath(t) => {
                self.flush_paragraph();
                self.want_blank();
                self.resolve_blank();
                let st = Style::default()
                    .fg(self.theme.math)
                    .bg(self.theme.bg)
                    .add_modifier(Modifier::ITALIC);
                for l in t.lines() {
                    let spans = vec![
                        Span::styled("  ", Style::default().bg(self.theme.bg)),
                        Span::styled(l.to_string(), st),
                    ];
                    self.emit(spans);
                }
                self.want_blank();
            }
            Event::Html(h) => {
                if let Some(buf) = self.html.as_mut() {
                    buf.push_str(&h);
                } else {
                    self.html = Some(h.to_string());
                }
            }
            Event::InlineHtml(h) => {
                let st = Style::default().fg(self.theme.html).bg(self.theme.bg);
                self.inline.push(Span::styled(h.to_string(), st));
            }
            Event::FootnoteReference(label) => {
                let n = self.footnote_number(&label);
                let st = Style::default()
                    .fg(self.theme.footnote)
                    .bg(self.theme.bg)
                    .add_modifier(Modifier::BOLD);
                self.inline.push(Span::styled(format!("[{n}]"), st));
            }
            Event::SoftBreak => {
                self.inline
                    .push(Span::styled(" ", Style::default().bg(self.theme.bg)));
                if self.heading.is_some() {
                    self.heading_text.push(' ');
                }
            }
            Event::HardBreak => {
                let spans = std::mem::take(&mut self.inline);
                self.emit_wrapped(&spans);
            }
            Event::Rule => {
                self.flush_paragraph();
                self.want_blank();
                self.resolve_blank();
                let w = self.avail();
                let st = Style::default().fg(self.theme.rule).bg(self.theme.bg);
                self.emit(vec![Span::styled("─".repeat(w), st)]);
                self.want_blank();
            }
            Event::TaskListMarker(done) => {
                let (glyph, color) = if done {
                    ("☑ ", self.theme.task_done)
                } else {
                    ("☐ ", self.theme.task_todo)
                };
                let st = Style::default().fg(color).bg(self.theme.bg);
                if let Some(ctx) = self.ctx.last_mut() {
                    if !ctx.used {
                        ctx.first.push(Span::styled(glyph, st));
                        ctx.cont
                            .push(Span::styled("  ", Style::default().bg(self.theme.bg)));
                        ctx.width += 2;
                    }
                }
            }
        }
    }

    fn footnote_number(&mut self, label: &str) -> usize {
        if let Some(n) = self.footnotes.get(label) {
            return *n;
        }
        let n = self.footnote_next;
        self.footnote_next += 1;
        self.footnotes.insert(label.to_string(), n);
        n
    }

    fn text(&mut self, t: &str) {
        if let Some(code) = self.code.as_mut() {
            code.text.push_str(t);
            return;
        }
        if let Some(meta) = self.meta.as_mut() {
            meta.push_str(t);
            return;
        }
        if self.heading.is_some() {
            self.heading_text.push_str(t);
        }
        let st = self.style();
        self.inline.push(Span::styled(t.to_string(), st));
    }

    fn flush_paragraph(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.inline);
        self.emit_wrapped(&spans);
    }

    fn start(&mut self, tag: Tag<'_>, _range: Range<usize>) {
        match tag {
            Tag::Paragraph => {
                self.flush_paragraph();
                self.resolve_blank();
            }
            Tag::Heading { level, .. } => {
                self.flush_paragraph();
                self.resolve_blank();
                self.heading = Some(level as u8);
                self.heading_text.clear();
                let st = self.theme.heading_style(level as usize);
                self.styles.push(st);
            }
            Tag::BlockQuote(kind) => {
                self.flush_paragraph();
                self.resolve_blank();
                let (bar_color, title) = match kind {
                    Some(BlockQuoteKind::Note) => (self.theme.info, Some(("ℹ NOTE", self.theme.info))),
                    Some(BlockQuoteKind::Tip) => (self.theme.ok, Some(("✦ TIP", self.theme.ok))),
                    Some(BlockQuoteKind::Important) => {
                        (self.theme.accent_alt, Some(("❗IMPORTANT", self.theme.accent_alt)))
                    }
                    Some(BlockQuoteKind::Warning) => {
                        (self.theme.warn, Some(("⚠ WARNING", self.theme.warn)))
                    }
                    Some(BlockQuoteKind::Caution) => {
                        (self.theme.err, Some(("⛔ CAUTION", self.theme.err)))
                    }
                    None => (self.theme.quote_bar, None),
                };
                let bar = Style::default().fg(bar_color).bg(self.theme.bg);
                self.push_ctx(
                    vec![Span::styled("▌ ", bar)],
                    vec![Span::styled("▌ ", bar)],
                );
                if let Some((label, color)) = title {
                    // Alerts get a bold, iconified caption line before their body.
                    let st = Style::default()
                        .fg(color)
                        .bg(self.theme.bg)
                        .add_modifier(Modifier::BOLD);
                    let label = label.to_string();
                    self.emit(vec![Span::styled(label, st)]);
                }
                self.styles.push(
                    Style::default()
                        .fg(self.theme.quote_fg)
                        .bg(self.theme.bg)
                        .add_modifier(Modifier::ITALIC),
                );
            }
            Tag::CodeBlock(kind) => {
                self.flush_paragraph();
                self.resolve_blank();
                let info = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some(CodeCtx {
                    info,
                    text: String::new(),
                });
            }
            Tag::HtmlBlock => {
                self.flush_paragraph();
                self.resolve_blank();
                self.html = Some(String::new());
            }
            Tag::List(start) => {
                // Tight lists emit text without a Paragraph tag, so a nested
                // list can start while the parent item's text is still pending.
                self.flush_paragraph();
                if self.lists.is_empty() {
                    self.resolve_blank();
                }
                self.pending_blank = false;
                self.lists.push(ListCtx {
                    ordered: start,
                    index: start.unwrap_or(0),
                });
            }
            Tag::Item => {
                self.flush_paragraph();
                self.pending_blank = false;
                let depth = self.lists.len().saturating_sub(1);
                let (marker, style) = match self.lists.last_mut() {
                    Some(l) if l.ordered.is_some() => {
                        let n = l.index;
                        l.index += 1;
                        (
                            format!("{n}. "),
                            Style::default().fg(self.theme.number).bg(self.theme.bg),
                        )
                    }
                    _ => {
                        let glyph = match depth % 3 {
                            0 => "• ",
                            1 => "◦ ",
                            _ => "▪ ",
                        };
                        (
                            glyph.to_string(),
                            Style::default().fg(self.theme.bullet).bg(self.theme.bg),
                        )
                    }
                };
                let pad = " ".repeat(str_width(&marker));
                self.push_ctx(
                    vec![Span::styled(marker, style)],
                    vec![Span::styled(pad, Style::default().bg(self.theme.bg))],
                );
            }
            Tag::FootnoteDefinition(label) => {
                self.flush_paragraph();
                self.want_blank();
                self.resolve_blank();
                let n = self.footnote_number(&label);
                let st = Style::default()
                    .fg(self.theme.footnote)
                    .bg(self.theme.bg)
                    .add_modifier(Modifier::BOLD);
                let marker = format!("[{n}] ");
                let pad = " ".repeat(str_width(&marker));
                self.push_ctx(
                    vec![Span::styled(marker, st)],
                    vec![Span::styled(pad, Style::default().bg(self.theme.bg))],
                );
            }
            Tag::DefinitionList => {
                self.flush_paragraph();
                self.resolve_blank();
            }
            Tag::DefinitionListTitle => {
                self.styles.push(
                    Style::default()
                        .fg(self.theme.fg)
                        .bg(self.theme.bg)
                        .add_modifier(Modifier::BOLD),
                );
            }
            Tag::DefinitionListDefinition => {
                let st = Style::default().fg(self.theme.dim).bg(self.theme.bg);
                self.push_ctx(
                    vec![Span::styled("  : ", st)],
                    vec![Span::styled("    ", Style::default().bg(self.theme.bg))],
                );
            }
            Tag::Table(aligns) => {
                self.flush_paragraph();
                self.want_blank();
                self.resolve_blank();
                self.table = Some(TableCtx {
                    aligns,
                    head: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                }
                self.styles.push(
                    Style::default()
                        .fg(self.theme.table_head)
                        .bg(self.theme.bg)
                        .add_modifier(Modifier::BOLD),
                );
            }
            Tag::TableRow | Tag::TableCell => {}
            Tag::Emphasis => self.push_style(|s| s.add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(|s| s.add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
            Tag::Superscript | Tag::Subscript => {
                self.push_style(|s| s.add_modifier(Modifier::DIM))
            }
            Tag::Link { dest_url, .. } => {
                self.link = Some((dest_url.to_string(), self.inline.len()));
                let c = self.theme.link;
                self.push_style(move |s| s.fg(c).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { dest_url, .. } => {
                self.link = Some((dest_url.to_string(), self.inline.len()));
                let c = self.theme.image;
                self.inline.push(Span::styled(
                    "🖼 ",
                    Style::default().fg(c).bg(self.theme.bg),
                ));
                self.push_style(move |s| s.fg(c).add_modifier(Modifier::ITALIC));
            }
            Tag::MetadataBlock(_) => {
                self.flush_paragraph();
                self.meta = Some(String::new());
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_paragraph();
                self.want_blank();
            }
            TagEnd::Heading(level) => {
                let level = level as usize;
                let text = self.heading_text.trim().to_string();
                let mut spans = Vec::new();
                if self.opts.heading_markers {
                    let marker = Style::default()
                        .fg(self.theme.heading[level.clamp(1, 6) - 1])
                        .bg(self.theme.bg)
                        .add_modifier(Modifier::DIM);
                    spans.push(Span::styled("#".repeat(level) + " ", marker));
                }
                spans.append(&mut self.inline);
                let line_idx = self.out.lines.len();
                self.emit_wrapped(&spans);
                self.pop_style();
                self.out.toc.push(TocEntry {
                    level: level as u8,
                    title: text.clone(),
                    line: line_idx,
                    src_line: self.cur_src,
                });
                if level <= 2 {
                    let w = self.avail();
                    let glyph = if level == 1 { "━" } else { "─" };
                    let st = Style::default()
                        .fg(self.theme.heading[level - 1])
                        .bg(self.theme.bg)
                        .add_modifier(Modifier::DIM);
                    self.emit(vec![Span::styled(glyph.repeat(w), st)]);
                }
                self.heading = None;
                self.want_blank();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_paragraph();
                self.pop_style();
                self.ctx.pop();
                self.pending_blank = false;
                self.want_blank();
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.render_code(&code);
                }
                self.want_blank();
            }
            TagEnd::HtmlBlock => {
                if let Some(html) = self.html.take() {
                    self.render_html(&html);
                }
                self.want_blank();
            }
            TagEnd::List(_) => {
                self.flush_paragraph();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.want_blank();
                }
            }
            TagEnd::Item => {
                self.flush_paragraph();
                // An item that produced nothing still occupies a line.
                if let Some(ctx) = self.ctx.last() {
                    if !ctx.used {
                        self.emit(Vec::new());
                    }
                }
                self.ctx.pop();
                self.pending_blank = false;
            }
            TagEnd::FootnoteDefinition => {
                self.flush_paragraph();
                self.ctx.pop();
                self.want_blank();
            }
            TagEnd::DefinitionList => self.want_blank(),
            TagEnd::DefinitionListTitle => {
                self.flush_paragraph();
                self.pop_style();
            }
            TagEnd::DefinitionListDefinition => {
                self.flush_paragraph();
                self.ctx.pop();
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                }
                self.want_blank();
            }
            TagEnd::TableHead => {
                self.pop_style();
                if let Some(t) = self.table.as_mut() {
                    t.in_head = false;
                    t.head = std::mem::take(&mut t.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.row);
                    if !row.is_empty() {
                        t.rows.push(row);
                    }
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(t) = self.table.as_mut() {
                    t.row.push(cell);
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                if let Some((url, start)) = self.link.take() {
                    let label: String = self
                        .inline
                        .iter()
                        .skip(start)
                        .map(|s| s.content.as_ref())
                        .collect::<String>();
                    self.out.links.push(LinkEntry {
                        label: label.trim().to_string(),
                        url: url.clone(),
                        line: self.out.lines.len(),
                    });
                    let shown = label.trim() == url.trim()
                        || url.starts_with('#')
                        || url.is_empty();
                    if self.opts.show_urls && !shown {
                        let st = Style::default().fg(self.theme.link_url).bg(self.theme.bg);
                        self.inline.push(Span::styled(format!(" ({url})"), st));
                    }
                }
            }
            TagEnd::Image => {
                self.pop_style();
                if let Some((url, _)) = self.link.take() {
                    if self.opts.show_urls {
                        let st = Style::default().fg(self.theme.link_url).bg(self.theme.bg);
                        self.inline.push(Span::styled(format!(" ({url})"), st));
                    }
                }
            }
            TagEnd::MetadataBlock(kind) => {
                if let Some(meta) = self.meta.take() {
                    self.render_metadata(&meta, kind);
                }
                self.want_blank();
            }
        }
    }

    fn render_code(&mut self, code: &CodeCtx) {
        let theme = self.theme;
        let lang = tohki::lang_for(&code.info);
        let label = tohki::display_name(&code.info);
        let total = self.avail();
        let border = Style::default().fg(theme.code_gutter).bg(theme.code_bg);
        let label_st = Style::default().fg(theme.code_label).bg(theme.code_bg);
        let body_bg = Style::default().fg(theme.code_fg).bg(theme.code_bg);

        let lines: Vec<&str> = code.text.trim_end_matches('\n').split('\n').collect();
        let num_w = if self.opts.code_numbers {
            lines.len().to_string().len() + 1
        } else {
            0
        };
        // │ + space + [gutter] + text + space + │
        let inner = total.saturating_sub(4 + num_w).max(8);

        // Top chrome: ╭─ lang ─────╮
        let head_label = format!(" {label} ");
        let dashes = total
            .saturating_sub(2 + str_width(&head_label))
            .max(0);
        self.emit(vec![
            Span::styled("╭─", border),
            Span::styled(head_label, label_st),
            Span::styled("─".repeat(dashes.saturating_sub(1)), border),
            Span::styled("╮", border),
        ]);

        let all_toks = tohki::tokenize(lang, &code.text);
        for (n, row_tok) in all_toks.iter().enumerate() {
            let spans: Vec<Span<'static>> = row_tok.into_iter().map(|tok| {
                Span::styled(tok.text.to_string(), self.tok_style(tok.kind))
            }).collect();

            let wrapped = hard_wrap(&spans, inner);
            for (wi, mut piece) in wrapped.into_iter().enumerate() {
                let mut row = vec![Span::styled("│", border)];
                if num_w > 0 {
                    let label = if wi == 0 {
                        format!("{:>w$} ", n + 1, w = num_w - 1)
                    } else {
                        " ".repeat(num_w)
                    };
                    row.push(Span::styled(
                        label,
                        Style::default().fg(theme.code_gutter).bg(theme.code_bg),
                    ));
                }
                row.append(&mut piece);
                            inline::pad_to(&mut row, total.saturating_sub(2), body_bg);
                            row.push(Span::styled(" │", border));
                            self.emit(row);
            }
        }

        self.emit(vec![
            Span::styled("╰", border),
            Span::styled("─".repeat(total.saturating_sub(2)), border),
            Span::styled("╯", border),
        ]);
    }

    fn tok_style(&self, kind: Tok) -> Style {
        let t = self.theme;
        let base = Style::default().bg(t.code_bg);
        match kind {
            Tok::Text => base.fg(t.code_fg),
            Tok::Keyword => base.fg(t.syn_keyword).add_modifier(Modifier::BOLD),
            Tok::Type => base.fg(t.syn_type),
            Tok::Const => base.fg(t.syn_const),
            Tok::Str => base.fg(t.syn_string),
            Tok::Number => base.fg(t.syn_number),
            Tok::Comment => base.fg(t.syn_comment).add_modifier(Modifier::ITALIC),
            Tok::Func => base.fg(t.syn_func),
            Tok::Punct => base.fg(t.syn_punct),
            Tok::Attr => base.fg(t.syn_attr),
        }
    }

    fn render_html(&mut self, html: &str) {
        let st = Style::default()
            .fg(self.theme.html)
            .bg(self.theme.bg)
            .add_modifier(Modifier::ITALIC);
        let avail = self.avail();
        for raw in html.trim_end().lines() {
            let spans = vec![Span::styled(raw.to_string(), st)];
            let clipped = inline::truncate(&spans, avail, st);
            self.emit(clipped);
        }
    }

    fn render_metadata(&mut self, meta: &str, kind: MetadataBlockKind) {
        let theme = self.theme;
        let border = Style::default().fg(theme.rule).bg(theme.bg);
        let key = Style::default().fg(theme.accent).bg(theme.bg);
        let val = Style::default().fg(theme.meta_fg).bg(theme.bg);
        let avail = self.avail();
        let label = match kind {
            MetadataBlockKind::YamlStyle => " front matter ",
            MetadataBlockKind::PlusesStyle => " metadata ",
        };
        self.emit(vec![
            Span::styled("┄".repeat(2), border),
            Span::styled(label, Style::default().fg(theme.dim).bg(theme.bg)),
            Span::styled("┄".repeat(avail.saturating_sub(2 + str_width(label))), border),
        ]);
        for raw in meta.trim_end().lines() {
            let spans = match raw.split_once(':') {
                Some((k, v)) if !k.trim().is_empty() && !k.starts_with(' ') => vec![
                    Span::styled(format!("{k}:"), key),
                    Span::styled(v.to_string(), val),
                ],
                _ => vec![Span::styled(raw.to_string(), val)],
            };
            let clipped = inline::truncate(&spans, avail, val);
            self.emit(clipped);
        }
        self.emit(vec![Span::styled("┄".repeat(avail), border)]);
    }

    fn render_table(&mut self, t: TableCtx) {
        let theme = self.theme;
        let border = Style::default().fg(theme.table_border).bg(theme.bg);
        let ncols = t
            .head
            .len()
            .max(t.rows.iter().map(|r| r.len()).max().unwrap_or(0))
            .max(1);
        let avail = self.avail();

        // Natural width per column, then shrink proportionally if too wide.
        let mut widths = vec![0usize; ncols];
        let consider = |row: &Vec<Vec<Span<'static>>>, widths: &mut Vec<usize>| {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(spans_width(cell));
                }
            }
        };
        consider(&t.head, &mut widths);
        for row in &t.rows {
            consider(row, &mut widths);
        }
        for w in widths.iter_mut() {
            *w = (*w).max(1);
        }

        let chrome = ncols * 3 + 1;
        let mut budget = avail.saturating_sub(chrome).max(ncols);
        let natural: usize = widths.iter().sum();
        if natural > budget {
            // Shrink the widest columns first so narrow ones stay readable.
            let mut shrunk = widths.clone();
            while shrunk.iter().sum::<usize>() > budget {
                let (idx, _) = shrunk
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, w)| **w)
                    .map(|(i, w)| (i, *w))
                    .unwrap();
                if shrunk[idx] <= 3 {
                    break;
                }
                shrunk[idx] -= 1;
            }
            widths = shrunk;
        } else {
            budget = natural;
        }
        let _ = budget;

        let rule = |left: &str, mid: &str, right: &str, widths: &[usize]| -> Vec<Span<'static>> {
            let mut s = String::from(left);
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                s.push_str(if i + 1 == widths.len() { right } else { mid });
            }
            vec![Span::styled(s, border)]
        };

        let top = rule("┌", "┬", "┐", &widths);
        self.emit(top);
        if !t.head.is_empty() {
            self.emit_row(&t.head, &widths, &t.aligns, border);
            let sep = rule("├", "┼", "┤", &widths);
            self.emit(sep);
        }
        for row in &t.rows {
            self.emit_row(row, &widths, &t.aligns, border);
        }
        let bottom = rule("└", "┴", "┘", &widths);
        self.emit(bottom);
    }

    fn emit_row(
        &mut self,
        row: &[Vec<Span<'static>>],
        widths: &[usize],
        aligns: &[Alignment],
        border: Style,
    ) {
        let bg = Style::default().bg(self.theme.bg);
        // Wrap each cell, then emit as many physical lines as the tallest cell.
        let wrapped: Vec<Vec<Vec<Span<'static>>>> = widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let empty: Vec<Span<'static>> = Vec::new();
                let cell = row.get(i).unwrap_or(&empty);
                inline::wrap(cell, *w)
            })
            .collect();
        let height = wrapped.iter().map(|c| c.len()).max().unwrap_or(1).max(1);

        for r in 0..height {
            let mut line: Vec<Span<'static>> = vec![Span::styled("│ ", border)];
            for (i, w) in widths.iter().enumerate() {
                let empty: Vec<Span<'static>> = Vec::new();
                let piece = wrapped[i].get(r).unwrap_or(&empty).clone();
                let used = spans_width(&piece);
                let pad = w.saturating_sub(used);
                let align = aligns.get(i).copied().unwrap_or(Alignment::None);
                let (lead, trail) = match align {
                    Alignment::Right => (pad, 0),
                    Alignment::Center => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                if lead > 0 {
                    line.push(Span::styled(" ".repeat(lead), bg));
                }
                line.extend(piece);
                if trail > 0 {
                    line.push(Span::styled(" ".repeat(trail), bg));
                }
                line.push(Span::styled(
                    if i + 1 == widths.len() { " │" } else { " │ " },
                    border,
                ));
            }
            self.emit(line);
        }
    }
}

/// Break styled spans at exact column boundaries (no word awareness) — used for
/// code, where breaking on spaces would misrepresent the source.
fn hard_wrap(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut out: Vec<Vec<Span<'static>>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut cur_w = 0usize;
    for span in spans {
        let mut buf = String::new();
        for ch in span.content.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if cur_w + cw > width {
                if !buf.is_empty() {
                    cur.push(Span::styled(std::mem::take(&mut buf), span.style));
                }
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            buf.push(ch);
            cur_w += cw;
        }
        if !buf.is_empty() {
            cur.push(Span::styled(buf, span.style));
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DARK;

    fn lines(src: &str, width: u16) -> Vec<String> {
        render(src, width, &DARK, RenderOpts::default())
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn body(src: &str) -> String {
        lines(src, 60).join("\n")
    }

    #[test]
    fn headings_get_markers_and_a_rule() {
        let out = lines("# Title\n\n### Small\n", 40);
        assert_eq!(out[0], "# Title");
        assert!(out[1].starts_with('━'), "h1 gets an underline rule");
        assert!(out.iter().any(|l| l == "### Small"));
    }

    #[test]
    fn heading_markers_can_be_switched_off() {
        let opts = RenderOpts {
            heading_markers: false,
            ..Default::default()
        };
        let doc = render("### Small\n", 40, &DARK, opts);
        let text: String = doc.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text.trim_end(), "Small");
    }

    #[test]
    fn headings_populate_the_table_of_contents() {
        let doc = render("# A\n\n## B\n\ntext\n", 40, &DARK, RenderOpts::default());
        let titles: Vec<&str> = doc.toc.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["A", "B"]);
        assert_eq!(doc.toc[0].level, 1);
        assert_eq!(doc.toc[1].level, 2);
        assert_eq!(doc.lines[doc.toc[1].line], doc.lines[doc.toc[1].line]);
    }

    #[test]
    fn nested_lists_indent_by_level() {
        let out = body("- one\n  - two\n    - three\n");
        assert!(out.contains("• one"));
        assert!(out.contains("  ◦ two"));
        assert!(out.contains("    ▪ three"));
    }

    #[test]
    fn ordered_lists_keep_their_numbering() {
        let out = body("3. three\n4. four\n");
        assert!(out.contains("3. three"), "{out}");
        assert!(out.contains("4. four"), "{out}");
    }

    #[test]
    fn task_list_markers_become_checkboxes() {
        let out = body("- [x] done\n- [ ] todo\n");
        assert!(out.contains("☑ done"), "{out}");
        assert!(out.contains("☐ todo"), "{out}");
    }

    #[test]
    fn block_quotes_keep_a_bar_on_every_line() {
        let out = lines("> one two three four five six seven eight nine ten\n", 24);
        let quoted: Vec<&String> = out.iter().filter(|l| l.starts_with('▌')).collect();
        assert!(quoted.len() >= 2, "wrapped quote keeps its bar: {out:?}");
    }

    #[test]
    fn gfm_alerts_get_a_caption() {
        let out = body("> [!WARNING]\n> be careful\n");
        assert!(out.contains("⚠ WARNING"), "{out}");
        assert!(out.contains("be careful"));
    }

    #[test]
    fn tables_are_drawn_with_box_characters() {
        let out = body("| a | b |\n| - | - |\n| 1 | 2 |\n");
        assert!(out.contains('┌') && out.contains('┼') && out.contains('┘'), "{out}");
        assert!(out.contains("│ a"), "{out}");
    }

    #[test]
    fn table_alignment_is_respected() {
        let out = lines("| left | right |\n| :--- | ----: |\n| a | b |\n", 60);
        let row = out.iter().find(|l| l.contains(" a ")).unwrap();
        // Right-aligned cell hugs its trailing border.
        assert!(row.contains("b │"), "{row}");
    }

    #[test]
    fn tables_shrink_to_the_available_width() {
        let src = "| aaaaaaaaaaaaaaa | bbbbbbbbbbbbbbb |\n| - | - |\n| 1 | 2 |\n";
        for line in lines(src, 30) {
            assert!(crate::md::inline::str_width(&line) <= 30, "{line:?}");
        }
    }

    #[test]
    fn code_blocks_are_boxed_and_labelled() {
        let out = body("```rust\nfn main() {}\n```\n");
        assert!(out.contains("╭─ rust"), "{out}");
        assert!(out.contains("fn main() {}"));
        assert!(out.contains('╯'));
    }

    #[test]
    fn long_code_lines_wrap_inside_the_box() {
        let src = format!("```\n{}\n```\n", "x".repeat(200));
        for line in lines(&src, 40) {
            assert!(crate::md::inline::str_width(&line) <= 40, "{line:?}");
        }
    }

    #[test]
    fn footnotes_are_numbered_consistently() {
        let out = body("text[^a] and[^b]\n\n[^a]: first\n[^b]: second\n");
        assert!(out.contains("text[1] and[2]"), "{out}");
        assert!(out.contains("[1] first"), "{out}");
        assert!(out.contains("[2] second"), "{out}");
    }

    #[test]
    fn links_are_collected_with_their_anchor_text() {
        let doc = render(
            "see [the docs](https://example.com) now\n",
            60,
            &DARK,
            RenderOpts::default(),
        );
        assert_eq!(doc.links.len(), 1);
        assert_eq!(doc.links[0].label, "the docs");
        assert_eq!(doc.links[0].url, "https://example.com");
    }

    #[test]
    fn urls_can_be_hidden() {
        let opts = RenderOpts {
            show_urls: false,
            ..Default::default()
        };
        let doc = render("[x](https://example.com)\n", 60, &DARK, opts);
        let text: String = doc.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.contains("example.com"), "{text}");
    }

    #[test]
    fn front_matter_is_rendered_as_metadata() {
        let out = body("---\ntitle: Hello\n---\n\n# Doc\n");
        assert!(out.contains("front matter"), "{out}");
        assert!(out.contains("title: Hello"), "{out}");
    }

    #[test]
    fn every_rendered_line_fits_the_width() {
        let src = include_str!("../../demo/showcase.md");
        for width in [30u16, 55, 80] {
            for line in lines(src, width) {
                assert!(
                    crate::md::inline::str_width(&line) <= width as usize,
                    "width {width} overflowed: {line:?}"
                );
            }
        }
    }

    #[test]
    fn source_lines_map_back_to_the_document() {
        let doc = render("# One\n\npara\n\n# Two\n", 40, &DARK, RenderOpts::default());
        let second = doc.toc.iter().find(|t| t.title == "Two").unwrap();
        assert_eq!(second.src_line, 4);
        assert_eq!(doc.line_for_src(4), second.line);
    }

    #[test]
    fn trailing_blank_lines_are_trimmed() {
        let out = lines("text\n\n\n\n", 40);
        assert_eq!(out.last().map(String::as_str), Some("text"));
    }

    #[test]
    fn strikethrough_and_inline_code_survive() {
        let out = body("~~gone~~ and `code`\n");
        assert!(out.contains("gone"));
        assert!(out.contains("code"));
    }
}
