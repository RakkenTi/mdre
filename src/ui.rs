//! All drawing. Widgets are kept deliberately low-level (styled `Line`s written
//! straight into the buffer) because both the reader and the editor need
//! per-cell control that the stock widgets don't give.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::prelude::Stylize;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::{App, Mode, Overlay, StatusKind, filter_commands};
use crate::search::Hit;
use crate::editor::Pos;
use crate::md::inline::{str_width, truncate};
use crate::md::source;
use crate::theme::Theme;
use crate::workspace::{human_size, human_time};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // 6 is derived from the rows constraints (1+3+1+1)
    // Anything less than 6 will cause an out-of-bounds error
    if area.height < 6 {
        draw_collapsed_warning(f, area);
        return;
    }

    f.buffer_mut().set_style(area, app.theme.base());

    // If changed make sure to update the number x in area.height < x above
    // as well as the comment
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(f, rows[0], app);
    draw_body(f, rows[1], app);
    draw_status(f, rows[2], app);
    draw_hints(f, rows[3], app);

    match &app.overlay {
        Overlay::None => {}
        Overlay::Help { scroll } => draw_help(f, area, app, *scroll),
        Overlay::Palette { input, sel } => draw_palette(f, area, app, &input.clone(), *sel),
        Overlay::Prompt(_) => draw_prompt(f, area, app),
        Overlay::Links { sel, broken } => draw_links(f, area, app, *sel, broken),
        Overlay::Headings { sel } => draw_headings(f, area, app, *sel),
        Overlay::Results { title, hits, sel } => draw_results(f, area, app, title, hits, *sel),
    }
}

fn draw_collapsed_warning(f: &mut Frame, area: Rect) {
    let text = "Increase terminal height! Terminal height is too small. Make the terminal taller to see content.";
    f.render_widget(Paragraph::new(text).red(), area);
}

// ------------------------------------------------------------------ chrome

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    f.buffer_mut()
        .set_style(area, Style::default().bg(t.status_bg));

    let mut spans = vec![
        Span::styled(
            " mdre ",
            Style::default()
                .fg(t.bg)
                .bg(t.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().bg(t.status_bg)),
    ];

    let root = compress_path(&app.ws.root.display().to_string());
    spans.push(Span::styled(
        root,
        Style::default().fg(t.dim).bg(t.status_bg),
    ));
    if app.has_file() {
        spans.push(Span::styled(
            " › ",
            Style::default().fg(t.faint).bg(t.status_bg),
        ));
        spans.push(Span::styled(
            app.title(),
            Style::default()
                .fg(t.fg)
                .bg(t.status_bg)
                .add_modifier(Modifier::BOLD),
        ));
        if app.editor.dirty {
            spans.push(Span::styled(
                " ●",
                Style::default().fg(t.warn).bg(t.status_bg),
            ));
        }
    }

    // Right-aligned toggle indicators.
    let mut right = vec![];
    if app.split {
        right.push("split");
    }
    if app.outline {
        right.push("outline");
    }
    if app.ws.recursive {
        right.push("recursive");
    }
    let right_text = if right.is_empty() {
        String::new()
    } else {
        format!("{}  ", right.join(" · "))
    };

    let used = spans.iter().map(|s| str_width(&s.content)).sum::<usize>();
    let pad = (area.width as usize)
        .saturating_sub(used + str_width(&right_text));
    spans.push(Span::styled(
        " ".repeat(pad),
        Style::default().bg(t.status_bg),
    ));
    spans.push(Span::styled(
        right_text,
        Style::default().fg(t.faint).bg(t.status_bg),
    ));

    f.buffer_mut()
        .set_line(area.x, area.y, &Line::from(spans), area.width);
}

fn draw_status(f: &mut Frame, area: Rect, app: &mut App) {
    let t = app.theme;
    f.buffer_mut()
        .set_style(area, Style::default().bg(t.status_bg));

    let mode_bg = match app.mode {
        Mode::Browser => t.mode_files_bg,
        Mode::Read => t.mode_read_bg,
        Mode::Edit => t.mode_edit_bg,
    };
    let mut spans = vec![Span::styled(
        format!(" {} ", app.mode.label()),
        Style::default()
            .fg(t.bg)
            .bg(mode_bg)
            .add_modifier(Modifier::BOLD),
    )];

    let detail = match app.mode {
        Mode::Browser => {
            let n = app.ws.visible().len();
            let sort = app.ws.sort.label();
            format!(" {n} items · sort: {sort}")
        }
        Mode::Edit => {
            let (lines, words, chars) = app.editor.stats();
            let Pos { line, col } = app.editor.cursor;
            let sel = app
                .editor
                .selected_text()
                .map(|s| format!(" · sel {}", s.chars().count()))
                .unwrap_or_default();
            format!(
                " {}:{} · {lines} lines · {words} words · {chars} chars{sel}",
                line + 1,
                col + 1
            )
        }
        Mode::Read => {
            let total = app.doc.as_ref().map(|d| d.lines.len()).unwrap_or(0);
            let words = app.doc.as_ref().map(|d| d.words).unwrap_or(0);
            let pct = if total == 0 {
                100
            } else {
                ((app.reader_scroll + app.reader_area.height as usize).min(total) * 100 / total)
                    .min(100)
            };
            let mins = (words as f64 / 220.0).ceil().max(1.0) as usize;
            format!(" {pct}% · {words} words · ~{mins} min read")
        }
    };
    spans.push(Span::styled(
        detail,
        Style::default().fg(t.status_fg).bg(t.status_bg),
    ));

    if !app.search.needle.is_empty() && !app.search.matches.is_empty() {
        spans.push(Span::styled(
            format!(
                " · /{} [{}/{}]",
                app.search.needle,
                app.search.current + 1,
                app.search.matches.len()
            ),
            Style::default().fg(t.accent).bg(t.status_bg),
        ));
    }

    let message = app.status_visible().map(|s| (s.text.clone(), s.kind));
    if let Some((text, kind)) = message {
        let color = match kind {
            StatusKind::Info => t.info,
            StatusKind::Ok => t.ok,
            StatusKind::Warn => t.warn,
            StatusKind::Err => t.err,
        };
        let used: usize = spans.iter().map(|s| str_width(&s.content)).sum();
        let room = (area.width as usize).saturating_sub(used + 2);
        let text = format!("  {text}");
        let clipped = truncate(
            &[Span::raw(text)],
            room,
            Style::default().fg(color).bg(t.status_bg),
        );
        for mut s in clipped {
            s.style = Style::default().fg(color).bg(t.status_bg);
            spans.push(s);
        }
    }

    f.buffer_mut()
        .set_line(area.x, area.y, &Line::from(spans), area.width);
}

fn draw_hints(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let hints: &[(&str, &str)] = match app.mode {
        Mode::Browser if app.filtering => &[
            ("type", "narrow the list"),
            ("↑↓", "move"),
            ("↵", "open"),
            ("Esc", "clear filter"),
            ("Tab", "keep filter, back to keys"),
        ],
        Mode::Browser => &[
            ("↑↓", "move"),
            ("↵", "open"),
            ("e", "edit"),
            ("n", "new"),
            ("r", "rename"),
            ("f", "find in files"),
            ("d", "delete"),
            ("/", "filter"),
            ("*", "recurse"),
            ("F1", "help"),
        ],
        Mode::Read => &[
            ("↑↓/space", "scroll"),
            ("{}", "headings"),
            ("e", "edit"),
            ("o", "outline"),
            ("/", "find"),
            ("f", "find in files"),
            ("b", "backlinks"),
            ("L", "follow link"),
            ("⌫", "back"),
            ("Tab", "files"),
            ("F1", "help"),
        ],
        Mode::Edit => &[
            ("Ctrl+S", "save"),
            ("Ctrl+B", "bold"),
            ("Ctrl+L", "link"),
            ("Alt+1-6", "heading"),
            ("Alt+L/T", "list/task"),
            ("Alt+A", "align table"),
            ("Ctrl+W", "preview"),
            ("Ctrl+P", "commands"),
            ("Esc", "read"),
        ],
    };
    let mut spans = vec![Span::styled(" ", Style::default().bg(t.bg))];
    for (k, v) in hints {
        spans.push(Span::styled(
            *k,
            Style::default()
                .fg(t.accent)
                .bg(t.bg)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {v}  "),
            Style::default().fg(t.faint).bg(t.bg),
        ));
    }
    let line = Line::from(spans).style(Style::default().bg(t.bg));
    f.buffer_mut().set_line(area.x, area.y, &line, area.width);
}

// -------------------------------------------------------------------- body

fn draw_body(f: &mut Frame, area: Rect, app: &mut App) {
    let sidebar_w = if app.sidebar && area.width > 70 { 32 } else { 0 };
    let outline_w = if app.outline && area.width > 100 { 30 } else { 0 };

    let cols = Layout::horizontal([
        Constraint::Length(sidebar_w),
        Constraint::Min(20),
        Constraint::Length(outline_w),
    ])
    .split(area);

    if sidebar_w > 0 {
        draw_sidebar(f, cols[0], app);
    }
    let main = cols[1];

    if app.split && app.mode != Mode::Browser {
        let halves =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(main);
        draw_editor(f, halves[0], app);
        draw_reader(f, halves[1], app, false);
    } else {
        match app.mode {
            Mode::Edit => draw_editor(f, main, app),
            _ => draw_reader(f, main, app, true),
        }
    }

    if outline_w > 0 {
        draw_outline(f, cols[2], app);
    }
}

fn panel<'a>(app: &App, title: &'a str, focused: bool) -> Block<'a> {
    let t = app.theme;
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { t.border_focus } else { t.border }))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(if focused { t.accent } else { t.dim })
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.bg))
}

// ----------------------------------------------------------------- sidebar

fn draw_sidebar(f: &mut Frame, area: Rect, app: &mut App) {
    let t = app.theme;
    let focused = app.mode == Mode::Browser;
    let title = if app.filtering {
        format!("filter: {}▏", app.ws.filter)
    } else if app.ws.filter.is_empty() {
        "files".to_string()
    } else {
        format!("filter: {}", app.ws.filter)
    };
    let block = panel(app, &title, focused);
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.list_area = inner;

    if let Some(err) = app.ws.error.clone() {
        let line = Line::from(Span::styled(err, Style::default().fg(t.err).bg(t.bg)));
        f.buffer_mut()
            .set_line(inner.x, inner.y, &line, inner.width);
        return;
    }

    let visible = app.ws.visible();
    if visible.is_empty() {
        let line = Line::from(Span::styled(
            "  (no matching files)",
            Style::default().fg(t.faint).bg(t.bg),
        ));
        f.buffer_mut()
            .set_line(inner.x, inner.y, &line, inner.width);
        return;
    }

    // Each entry is two rows: name, then metadata.
    let rows_per = if inner.width >= 24 { 2 } else { 1 };
    let capacity = (inner.height as usize / rows_per).max(1);
    let cursor = visible
        .iter()
        .position(|i| *i == app.ws.selected)
        .unwrap_or(0);
    let start = cursor.saturating_sub(capacity / 2).min(
        visible.len().saturating_sub(capacity),
    );

    for (row, &idx) in visible.iter().skip(start).take(capacity).enumerate() {
        let entry = &app.ws.entries[idx];
        let y = inner.y + (row * rows_per) as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let selected = idx == app.ws.selected;
        let is_open = app.editor.path.as_deref() == Some(entry.path.as_path());

        let bg = if selected && focused {
            t.sel_bg
        } else if selected {
            t.cursor_line_bg
        } else {
            t.bg
        };
        f.buffer_mut().set_style(
            Rect::new(inner.x, y, inner.width, rows_per as u16),
            Style::default().bg(bg),
        );

        let icon = if entry.is_parent {
            "⤴ "
        } else if entry.is_dir {
            "▸ "
        } else if is_open {
            "◆ "
        } else {
            "· "
        };
        let name_style = Style::default()
            .fg(if entry.is_dir {
                t.accent_alt
            } else if is_open {
                t.accent
            } else {
                t.fg
            })
            .bg(bg)
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });

        let spans = vec![
            Span::styled(icon, Style::default().fg(t.dim).bg(bg)),
            Span::styled(entry.name.clone(), name_style),
        ];
        let clipped = truncate(&spans, inner.width as usize, Style::default().fg(t.dim).bg(bg));
        f.buffer_mut()
            .set_line(inner.x, y, &Line::from(clipped), inner.width);

        if rows_per == 2 && y + 1 < inner.y + inner.height {
            let meta = if entry.is_parent {
                "parent directory".to_string()
            } else if entry.is_dir {
                "directory".to_string()
            } else {
                let mut parts = vec![human_size(entry.size)];
                if let Some(m) = entry.modified {
                    parts.push(human_time(m));
                }
                parts.join(" · ")
            };
            let title = entry.title.clone().unwrap_or_default();
            let mut spans = vec![Span::styled(
                format!("  {meta}"),
                Style::default().fg(t.faint).bg(bg),
            )];
            if !title.is_empty() {
                spans.push(Span::styled(
                    format!("  {title}"),
                    Style::default()
                        .fg(t.dim)
                        .bg(bg)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            let clipped = truncate(&spans, inner.width as usize, Style::default().fg(t.faint).bg(bg));
            f.buffer_mut()
                .set_line(inner.x, y + 1, &Line::from(clipped), inner.width);
        }
    }
}

// ------------------------------------------------------------------ reader

fn draw_reader(f: &mut Frame, area: Rect, app: &mut App, focused: bool) {
    let t = app.theme;
    let title = if app.has_file() {
        app.title()
    } else {
        "reader".into()
    };
    let block = panel(app, &title, focused && app.mode == Mode::Read).padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.reader_area = inner;

    if !app.has_file() && app.editor.lines.iter().all(|l| l.is_empty()) {
        draw_welcome(f, inner, app);
        return;
    }

    let width = inner.width;
    let doc_width = width.min(app.opts.max_width);
    let needle = app.search.needle.to_lowercase();
    let matches: Vec<usize> = app.search.matches.clone();
    let height = inner.height as usize;
    // Render first and drop the borrow: following a link to `file.md#section`
    // can only pick a scroll position once the headings exist, so the scroll we
    // want is the one left behind by this call, not the one before it. The
    // second call is a cache hit.
    app.document(doc_width);
    let prev_scroll = app.reader_scroll;
    let doc = app.document(doc_width);
    let indent = ((width.saturating_sub(doc.width)) / 2) as u16;
    let total = doc.lines.len();
    let scroll = prev_scroll.min(total.saturating_sub(1));

    for (row, line) in doc.lines.iter().skip(scroll).take(height).enumerate() {
        let y = inner.y + row as u16;
        let mut line = line.clone();
        // Tint lines holding a search hit.
        if !needle.is_empty() && matches.contains(&(scroll + row)) {
            for span in line.spans.iter_mut() {
                if span.content.to_lowercase().contains(&needle) {
                    span.style = span.style.bg(t.match_bg);
                }
            }
        }
        f.buffer_mut()
            .set_line(inner.x + indent, y, &line, doc_width);
    }

    if total > height {
        let mut state = ScrollbarState::new(total.saturating_sub(height))
            .position(scroll)
            .viewport_content_length(height);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(t.border_focus))
                .track_style(Style::default().fg(t.border))
                .begin_symbol(None)
                .end_symbol(None),
            area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut state,
        );
    }
    app.reader_scroll = scroll;
}

fn draw_welcome(f: &mut Frame, area: Rect, app: &App) {
    let t = app.theme;
    let lines: Vec<(&str, Style)> = vec![
        ("", Style::default()),
        ("  mdre", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        ("  a terminal markdown manager", Style::default().fg(t.dim)),
        ("", Style::default()),
        ("  Tab      browse files", Style::default().fg(t.fg)),
        ("  ↵        open the highlighted file", Style::default().fg(t.fg)),
        ("  e        edit  ·  Ctrl+E toggles read/edit", Style::default().fg(t.fg)),
        ("  Ctrl+W   side-by-side editor and preview", Style::default().fg(t.fg)),
        ("  Ctrl+P   command palette", Style::default().fg(t.fg)),
        ("  F1       full key reference", Style::default().fg(t.fg)),
        ("", Style::default()),
        ("  Reader renders GFM: tables, task lists, alerts,", Style::default().fg(t.faint)),
        ("  footnotes, strikethrough and highlighted code.", Style::default().fg(t.faint)),
    ];
    for (i, (text, style)) in lines.iter().enumerate() {
        if i as u16 >= area.height {
            break;
        }
        let line = Line::from(Span::styled(text.to_string(), style.bg(t.bg)));
        f.buffer_mut()
            .set_line(area.x, area.y + i as u16, &line, area.width);
    }
}

// ------------------------------------------------------------------ outline

fn draw_outline(f: &mut Frame, area: Rect, app: &mut App) {
    let t = app.theme;
    let block = panel(app, "outline", false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let entries: Vec<(u8, String, usize, usize)> = if app.mode == Mode::Edit {
        app.editor
            .outline()
            .into_iter()
            .map(|(lvl, title, src)| (lvl, title, src, src))
            .collect()
    } else {
        let width = app.reader_area.width.max(20);
        app.document(width)
            .toc
            .iter()
            .map(|e| (e.level, e.title.clone(), e.line, e.src_line))
            .collect()
    };

    if entries.is_empty() {
        let line = Line::from(Span::styled(
            " no headings",
            Style::default().fg(t.faint).bg(t.bg),
        ));
        f.buffer_mut()
            .set_line(inner.x, inner.y, &line, inner.width);
        return;
    }

    // Track the heading containing the current position.
    let here = if app.mode == Mode::Edit {
        entries
            .iter()
            .rposition(|(_, _, _, src)| *src <= app.editor.cursor.line)
    } else {
        entries
            .iter()
            .rposition(|(_, _, line, _)| *line <= app.reader_scroll)
    }
    .unwrap_or(0);
    app.outline_sel = app.outline_sel.min(entries.len() - 1);

    let capacity = inner.height as usize;
    let start = here.saturating_sub(capacity / 2).min(entries.len().saturating_sub(capacity));

    for (row, (level, title, _, _)) in entries.iter().skip(start).take(capacity).enumerate() {
        let y = inner.y + row as u16;
        let idx = start + row;
        let current = idx == here;
        let bg = if current { t.cursor_line_bg } else { t.bg };
        let indent = "  ".repeat((*level as usize).saturating_sub(1));
        let color = t.heading[(*level as usize).clamp(1, 6) - 1];
        let spans = vec![
            Span::styled(
                format!("{indent}"),
                Style::default().bg(bg),
            ),
            Span::styled(
                if current { "▸ " } else { "  " },
                Style::default().fg(t.accent).bg(bg),
            ),
            Span::styled(
                title.clone(),
                Style::default().fg(color).bg(bg).add_modifier(
                    if *level <= 2 { Modifier::BOLD } else { Modifier::empty() },
                ),
            ),
        ];
        let clipped = truncate(&spans, inner.width as usize, Style::default().fg(t.dim).bg(bg));
        f.buffer_mut().set_style(
            Rect::new(inner.x, y, inner.width, 1),
            Style::default().bg(bg),
        );
        f.buffer_mut()
            .set_line(inner.x, y, &Line::from(clipped), inner.width);
    }
}

// ------------------------------------------------------------------- editor

fn draw_editor(f: &mut Frame, area: Rect, app: &mut App) {
    let t = app.theme;
    let title = format!(
        "{}{}",
        app.title(),
        if app.editor.dirty { " ●" } else { "" }
    );
    let block = panel(app, &title, app.mode == Mode::Edit);
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.editor_area = inner;

    let gutter = if app.line_numbers {
        (app.editor.lines.len().to_string().len() + 2) as u16
    } else {
        1
    };
    let text_w = inner.width.saturating_sub(gutter).max(4) as usize;
    let height = inner.height as usize;
    let wrap = app.wrap;

    let cur = app.editor.cursor;
    let rows_of = |line: &str| -> usize {
        if wrap {
            segments(&line.chars().collect::<Vec<_>>(), text_w).len()
        } else {
            1
        }
    };

    // Scroll so the cursor's logical line fits, counting wrapped rows.
    if cur.line < app.editor.scroll_y {
        app.editor.scroll_y = cur.line;
    } else {
        let mut used = 0usize;
        let mut first = cur.line;
        for li in (0..=cur.line).rev() {
            let need = rows_of(app.editor.line(li));
            if used + need > height && li != cur.line {
                break;
            }
            used += need;
            first = li;
        }
        if app.editor.scroll_y < first {
            app.editor.scroll_y = first;
        }
    }

    let cursor_disp = display_col(app.editor.line(cur.line), cur.col);
    if wrap {
        app.editor.scroll_x = 0;
    } else if cursor_disp < app.editor.scroll_x {
        app.editor.scroll_x = cursor_disp;
    } else if cursor_disp >= app.editor.scroll_x + text_w {
        app.editor.scroll_x = cursor_disp + 1 - text_w;
    }
    let scroll_y = app.editor.scroll_y;
    let scroll_x = app.editor.scroll_x;

    let selection = app.editor.selection();
    let needle = app.search.needle.clone();
    let case = app.search.case_sensitive;
    let kinds: Vec<source::BlockKind> = app.editor.block_kinds().to_vec();

    let mut row = 0usize;
    let mut li = scroll_y;
    let mut cursor_xy: Option<(u16, u16)> = None;

    while row < height && li < app.editor.lines.len() {
        let raw = app.editor.line(li).to_string();
        let chars: Vec<char> = raw.chars().collect();
        let is_cursor_line = li == cur.line;

        let kind = kinds.get(li).cloned().unwrap_or(source::BlockKind::Markdown);
        let mut spans = source::highlight_line(&raw, &kind, t);

        if let Some((a, b)) = selection {
            if li >= a.line && li <= b.line {
                let from = if li == a.line { a.col } else { 0 };
                let to = if li == b.line { b.col } else { chars.len() + 1 };
                spans = restyle_range(spans, from, to, |s| s.bg(t.sel_bg).fg(t.sel_fg));
            }
        }
        if !needle.is_empty() {
            let hay = if case { raw.clone() } else { raw.to_lowercase() };
            let pat = if case { needle.clone() } else { needle.to_lowercase() };
            let mut from = 0usize;
            while let Some(idx) = hay[from..].find(&pat) {
                let byte = from + idx;
                let col = hay[..byte].chars().count();
                spans = restyle_range(spans, col, col + pat.chars().count(), |s| s.bg(t.match_bg));
                from = byte + pat.len().max(1);
                if from >= hay.len() {
                    break;
                }
            }
        }

        let segs = if wrap {
            segments(&chars, text_w)
        } else {
            vec![(0usize, chars.len())]
        };

        for (seg_idx, (from, to)) in segs.iter().copied().enumerate() {
            if row >= height {
                break;
            }
            let y = inner.y + row as u16;
            let bg = if is_cursor_line && app.mode == Mode::Edit {
                t.cursor_line_bg
            } else {
                t.bg
            };
            f.buffer_mut().set_style(
                Rect::new(inner.x, y, inner.width, 1),
                Style::default().bg(bg),
            );

            if app.line_numbers {
                let label = if seg_idx == 0 {
                    format!("{:>w$} ", li + 1, w = gutter as usize - 2)
                } else {
                    format!("{:>w$} ", "↳", w = gutter as usize - 2)
                };
                let style = Style::default()
                    .fg(if is_cursor_line {
                        t.src_gutter_cur
                    } else {
                        t.src_gutter
                    })
                    .bg(bg);
                f.buffer_mut()
                    .set_line(inner.x, y, &Line::from(Span::styled(label, style)), gutter);
            }

            let mut piece = slice_spans(&spans, from, to);
            for span in piece.iter_mut() {
                if span.style.bg.is_none() {
                    span.style = span.style.bg(bg);
                }
            }
            let piece = if wrap {
                piece
            } else {
                clip_columns(piece, scroll_x, text_w)
            };
            f.buffer_mut().set_line(
                inner.x + gutter,
                y,
                &Line::from(piece),
                inner.width.saturating_sub(gutter),
            );

            // Cursor lands on the segment that contains its column.
            if is_cursor_line
                && ((cur.col >= from && cur.col < to)
                    || (cur.col >= to && seg_idx + 1 == segs.len()))
            {
                let dx: usize = chars[from..cur.col.min(chars.len())]
                    .iter()
                    .map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0))
                    .sum();
                let x = inner.x + gutter + dx.saturating_sub(if wrap { 0 } else { scroll_x }) as u16;
                cursor_xy = Some((x.min(inner.x + inner.width.saturating_sub(1)), y));
            }
            row += 1;
        }
        li += 1;
    }

    if app.mode == Mode::Edit {
        if let Some(pos) = cursor_xy {
            f.set_cursor_position(pos);
        }
    }

    if app.editor.lines.len() > height {
        let mut state = ScrollbarState::new(app.editor.lines.len().saturating_sub(height))
            .position(scroll_y)
            .viewport_content_length(height);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(t.border_focus))
                .track_style(Style::default().fg(t.border))
                .begin_symbol(None)
                .end_symbol(None),
            area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut state,
        );
    }
}

/// Split a logical line into `[start, end)` character ranges that each fit in
/// `width` columns, preferring to break after a space.
fn segments(chars: &[char], width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut segs = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut last_space: Option<usize> = None;
    let mut w = 0usize;
    while i < chars.len() {
        let cw = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(0);
        if w + cw > width {
            let brk = match last_space {
                Some(b) if b > start => b,
                _ => i.max(start + 1),
            };
            segs.push((start, brk));
            start = brk;
            i = brk;
            last_space = None;
            w = 0;
            continue;
        }
        w += cw;
        if chars[i] == ' ' {
            last_space = Some(i + 1);
        }
        i += 1;
    }
    segs.push((start, chars.len()));
    segs
}

/// Extract the `[from, to)` character range out of styled spans.
fn slice_spans(spans: &[Span<'static>], from: usize, to: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut col = 0usize;
    for span in spans {
        let len = span.content.chars().count();
        let start = col;
        let end = col + len;
        col = end;
        if end <= from || start >= to {
            continue;
        }
        let lo = from.saturating_sub(start);
        let hi = (to - start).min(len);
        let text: String = span.content.chars().skip(lo).take(hi - lo).collect();
        if !text.is_empty() {
            out.push(Span::styled(text, span.style));
        }
    }
    out
}

/// Display column (terminal cells) of a character index.
fn display_col(line: &str, col: usize) -> usize {
    line.chars()
        .take(col)
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

/// Re-style the `[from, to)` character range, splitting spans as needed.
fn restyle_range(
    spans: Vec<Span<'static>>,
    from: usize,
    to: usize,
    patch: impl Fn(Style) -> Style,
) -> Vec<Span<'static>> {
    let mut out = Vec::with_capacity(spans.len());
    let mut col = 0usize;
    for span in spans {
        let len = span.content.chars().count();
        let start = col;
        let end = col + len;
        col = end;
        if end <= from || start >= to {
            out.push(span);
            continue;
        }
        let chars: Vec<char> = span.content.chars().collect();
        let lo = from.saturating_sub(start).min(len);
        let hi = (to.saturating_sub(start)).min(len);
        if lo > 0 {
            out.push(Span::styled(
                chars[..lo].iter().collect::<String>(),
                span.style,
            ));
        }
        out.push(Span::styled(
            chars[lo..hi].iter().collect::<String>(),
            patch(span.style),
        ));
        if hi < len {
            out.push(Span::styled(
                chars[hi..].iter().collect::<String>(),
                span.style,
            ));
        }
    }
    out
}

/// Horizontal scroll: drop `skip` display columns and keep `width` of them.
fn clip_columns(spans: Vec<Span<'static>>, skip: usize, width: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut col = 0usize;
    let mut taken = 0usize;
    for span in spans {
        let mut buf = String::new();
        for ch in span.content.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if col + w <= skip {
                col += w;
                continue;
            }
            if taken + w > width {
                break;
            }
            buf.push(ch);
            taken += w;
            col += w;
        }
        if !buf.is_empty() {
            out.push(Span::styled(buf, span.style));
        }
        if taken >= width {
            break;
        }
    }
    out
}

// ----------------------------------------------------------------- overlays

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    )
}

fn overlay_block<'a>(t: &Theme, title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border_focus))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(t.panel_bg))
        .padding(Padding::new(1, 1, 0, 0))
}

fn draw_prompt(f: &mut Frame, area: Rect, app: &App) {
    let Overlay::Prompt(prompt) = &app.overlay else {
        return;
    };
    let t = app.theme;
    let rect = centered(area, 74, 5);
    f.render_widget(Clear, rect);
    let block = overlay_block(t, &prompt.title);
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let confirm = prompt.title.contains("(y/n)");
    let line = if confirm {
        Line::from(vec![
            Span::styled(
                "y",
                Style::default()
                    .fg(t.ok)
                    .bg(t.panel_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("es / ", Style::default().fg(t.dim).bg(t.panel_bg)),
            Span::styled(
                "n",
                Style::default()
                    .fg(t.err)
                    .bg(t.panel_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("o (any other key cancels)", Style::default().fg(t.dim).bg(t.panel_bg)),
        ])
    } else {
        Line::from(vec![
            Span::styled("› ", Style::default().fg(t.accent).bg(t.panel_bg)),
            Span::styled(
                prompt.input.clone(),
                Style::default().fg(t.fg).bg(t.panel_bg),
            ),
        ])
    };
    f.buffer_mut()
        .set_line(inner.x, inner.y + 1, &line, inner.width);
    if !confirm {
        let x = inner.x + 2 + display_col(&prompt.input, prompt.cursor) as u16;
        let right = inner.x + inner.width.saturating_sub(1);
        f.set_cursor_position((x.min(right), inner.y + 1));
    }
}

fn draw_palette(f: &mut Frame, area: Rect, app: &App, input: &str, sel: usize) {
    let t = app.theme;
    let rect = centered(area, 78, 22);
    f.render_widget(Clear, rect);
    let block = overlay_block(t, "commands");
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let query = Line::from(vec![
        Span::styled("› ", Style::default().fg(t.accent).bg(t.panel_bg)),
        Span::styled(input.to_string(), Style::default().fg(t.fg).bg(t.panel_bg)),
        Span::styled("▏", Style::default().fg(t.accent).bg(t.panel_bg)),
    ]);
    f.buffer_mut()
        .set_line(inner.x, inner.y, &query, inner.width);

    let items = filter_commands(input);
    let capacity = inner.height.saturating_sub(2) as usize;
    let start = sel.saturating_sub(capacity.saturating_sub(1));

    for (row, cmd) in items.iter().skip(start).take(capacity).enumerate() {
        let y = inner.y + 2 + row as u16;
        let selected = start + row == sel;
        let bg = if selected { t.sel_bg } else { t.panel_bg };
        f.buffer_mut()
            .set_style(Rect::new(inner.x, y, inner.width, 1), Style::default().bg(bg));
        let spans = vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(t.accent).bg(bg),
            ),
            Span::styled(
                format!("{:<10}", cmd.group),
                Style::default().fg(t.faint).bg(bg),
            ),
            Span::styled(
                format!("{:<34}", cmd.name),
                Style::default().fg(t.fg).bg(bg).add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(cmd.keys.to_string(), Style::default().fg(t.accent_alt).bg(bg)),
        ];
        let clipped = truncate(&spans, inner.width as usize, Style::default().fg(t.dim).bg(bg));
        f.buffer_mut()
            .set_line(inner.x, y, &Line::from(clipped), inner.width);
    }

    if items.is_empty() {
        let line = Line::from(Span::styled(
            "  no matching command",
            Style::default().fg(t.faint).bg(t.panel_bg),
        ));
        f.buffer_mut()
            .set_line(inner.x, inner.y + 2, &line, inner.width);
    }
}

fn draw_links(f: &mut Frame, area: Rect, app: &App, sel: usize, broken: &[bool]) {
    let t = app.theme;
    let links = app.doc.as_ref().map(|d| d.links.clone()).unwrap_or_default();
    let rect = centered(area, 86, (links.len() as u16 + 4).clamp(6, 24));
    f.render_widget(Clear, rect);
    let block = overlay_block(t, "links  ·  ⏎ follow  ·  g go to mention");
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    if links.is_empty() {
        let line = Line::from(Span::styled(
            "no links in this document",
            Style::default().fg(t.faint).bg(t.panel_bg),
        ));
        f.buffer_mut()
            .set_line(inner.x, inner.y + 1, &line, inner.width);
        return;
    }

    let capacity = inner.height as usize;
    let start = sel.saturating_sub(capacity.saturating_sub(1));
    for (row, link) in links.iter().skip(start).take(capacity).enumerate() {
        let y = inner.y + row as u16;
        let selected = start + row == sel;
        let bg = if selected { t.sel_bg } else { t.panel_bg };
        f.buffer_mut()
            .set_style(Rect::new(inner.x, y, inner.width, 1), Style::default().bg(bg));
        let dead = broken.get(start + row).copied().unwrap_or(false);
        let spans = vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(t.accent).bg(bg),
            ),
            Span::styled(
                if dead { "✗ " } else { "  " },
                Style::default().fg(t.err).bg(bg),
            ),
            Span::styled(
                format!("{:<28}", link.label),
                Style::default()
                    .fg(if dead { t.err } else { t.link })
                    .bg(bg),
            ),
            Span::styled(
                link.url.clone(),
                Style::default()
                    .fg(if dead { t.err } else { t.link_url })
                    .bg(bg),
            ),
        ];
        let clipped = truncate(&spans, inner.width as usize, Style::default().fg(t.dim).bg(bg));
        f.buffer_mut()
            .set_line(inner.x, y, &Line::from(clipped), inner.width);
    }
}

/// Cross-file search and backlink results: where the match is, then the line
/// it is on with the matched text picked out.
fn draw_results(f: &mut Frame, area: Rect, app: &App, title: &str, hits: &[Hit], sel: usize) {
    let t = app.theme;
    let rect = centered(area, 96, (hits.len() as u16 + 4).clamp(8, 26));
    f.render_widget(Clear, rect);
    let heading = format!("{title}  ·  ⏎ open");
    let block = overlay_block(t, &heading);
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let capacity = inner.height as usize;
    let start = sel.saturating_sub(capacity.saturating_sub(1));
    for (row, hit) in hits.iter().skip(start).take(capacity).enumerate() {
        let y = inner.y + row as u16;
        let selected = start + row == sel;
        let bg = if selected { t.sel_bg } else { t.panel_bg };
        f.buffer_mut()
            .set_style(Rect::new(inner.x, y, inner.width, 1), Style::default().bg(bg));

        let where_ = hit
            .path
            .strip_prefix(&app.ws.root)
            .unwrap_or(&hit.path)
            .display()
            .to_string();
        let mut spans = vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(t.accent).bg(bg),
            ),
            Span::styled(
                format!("{where_}:{}", hit.line + 1),
                Style::default().fg(t.accent).bg(bg),
            ),
            Span::styled("  ", Style::default().bg(bg)),
        ];

        // Split the line so the matched run can carry the search colour. The
        // hit records a character offset, so slice by characters.
        let text: Vec<char> = hit.text.chars().collect();
        let lead = hit.col.min(text.len());
        let plain = Style::default().fg(t.dim).bg(bg);
        spans.push(Span::styled(
            text[..lead].iter().collect::<String>().trim_start().to_string(),
            plain,
        ));
        if lead < text.len() {
            spans.push(Span::styled(
                text[lead..].iter().collect::<String>(),
                Style::default().fg(t.fg).bg(bg),
            ));
        }

        let clipped = truncate(&spans, inner.width as usize, plain);
        f.buffer_mut()
            .set_line(inner.x, y, &Line::from(clipped), inner.width);
    }
}

fn draw_headings(f: &mut Frame, area: Rect, app: &App, sel: usize) {
    let t = app.theme;
    let toc = app.doc.as_ref().map(|d| d.toc.clone()).unwrap_or_default();
    let rect = centered(area, 70, (toc.len() as u16 + 3).clamp(6, 26));
    f.render_widget(Clear, rect);
    let block = overlay_block(t, "go to heading");
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    if toc.is_empty() {
        let line = Line::from(Span::styled(
            "this document has no headings",
            Style::default().fg(t.faint).bg(t.panel_bg),
        ));
        f.buffer_mut()
            .set_line(inner.x, inner.y + 1, &line, inner.width);
        return;
    }

    let capacity = inner.height as usize;
    let start = sel.saturating_sub(capacity.saturating_sub(1));
    for (row, entry) in toc.iter().skip(start).take(capacity).enumerate() {
        let y = inner.y + row as u16;
        let selected = start + row == sel;
        let bg = if selected { t.sel_bg } else { t.panel_bg };
        f.buffer_mut()
            .set_style(Rect::new(inner.x, y, inner.width, 1), Style::default().bg(bg));
        let level = (entry.level as usize).clamp(1, 6);
        let spans = vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(t.accent).bg(bg),
            ),
            Span::styled("  ".repeat(level - 1), Style::default().bg(bg)),
            Span::styled(
                entry.title.clone(),
                Style::default().fg(t.heading[level - 1]).bg(bg).add_modifier(
                    if level <= 2 { Modifier::BOLD } else { Modifier::empty() },
                ),
            ),
            Span::styled(
                format!("  line {}", entry.src_line + 1),
                Style::default().fg(t.faint).bg(bg),
            ),
        ];
        let clipped = truncate(&spans, inner.width as usize, Style::default().fg(t.dim).bg(bg));
        f.buffer_mut()
            .set_line(inner.x, y, &Line::from(clipped), inner.width);
    }
}

fn draw_help(f: &mut Frame, area: Rect, app: &App, scroll: usize) {
    let t = app.theme;
    let rect = centered(area, 88, area.height.saturating_sub(4));
    f.render_widget(Clear, rect);
    let block = overlay_block(t, "key reference — any key closes, ↑↓ scrolls");
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let head = |s: &str| {
        Line::from(Span::styled(
            s.to_string(),
            Style::default()
                .fg(t.accent)
                .bg(t.panel_bg)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(
                format!("  {k:<16}"),
                Style::default()
                    .fg(t.accent_alt)
                    .bg(t.panel_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(d.to_string(), Style::default().fg(t.fg).bg(t.panel_bg)),
        ])
    };
    let blank = || Line::from(Span::styled("", Style::default().bg(t.panel_bg)));

    lines.push(head("GLOBAL"));
    for (k, d) in [
        ("F1 / ?", "this help"),
        ("Ctrl+P", "command palette (every tool, searchable)"),
        ("Ctrl+E", "switch between reader and editor"),
        ("Ctrl+W", "split view: editor beside live preview"),
        ("Tab", "jump to the file browser and back"),
        ("Alt+F", "file browser from anywhere (even mid-edit)"),
        ("F2", "show/hide the file sidebar"),
        ("F4 / o", "show/hide the outline panel"),
        ("F5", "reload the file from disk"),
        ("F6", "toggle editor soft wrap"),
        ("F9", "switch dark/light theme"),
        ("Ctrl+O", "open a path"),
        ("Ctrl+N", "new file"),
        ("Ctrl+Q / q", "quit"),
    ] {
        lines.push(row(k, d));
    }
    lines.push(blank());

    lines.push(head("FILES"));
    for (k, d) in [
        ("↑ ↓ / j k", "move"),
        ("Enter / l", "open file or enter directory"),
        ("Backspace / h", "parent directory"),
        ("e", "open directly in the editor"),
        ("n / N", "new file / new directory"),
        ("r", "rename"),
        ("d", "delete (asks first)"),
        ("/ then type", "fuzzy filter on names; Esc clears"),
        ("f", "search the contents of every file in the tree"),
        ("s / S", "cycle sort field / reverse"),
        (".", "toggle hidden files"),
        ("a", "toggle markdown-only"),
        ("*", "recursive listing of the whole tree"),
        ("R", "refresh"),
    ] {
        lines.push(row(k, d));
    }
    lines.push(blank());

    lines.push(head("READER"));
    for (k, d) in [
        ("j k / ↑ ↓", "scroll a line"),
        ("Space / PgDn", "scroll a page"),
        ("d / u", "half page down / up"),
        ("g / G", "top / bottom"),
        ("{ }", "previous / next heading"),
        ("/ n N", "search, next, previous"),
        ("f", "search every file in the tree"),
        ("b", "list the documents that link here"),
        ("o", "outline panel (tracks your position)"),
        ("O", "jump to a heading from a list"),
        ("L", "list every link; Enter follows it, g goes to the mention"),
        ("Backspace", "back to the document you came from"),
        ("y", "copy the whole document to the clipboard"),
        ("U", "show or hide URLs after link text"),
        ("H", "show or hide the # markers before headings"),
        ("#", "line numbers inside code blocks"),
        ("+ -", "text column width"),
        ("e / i", "edit this document"),
    ] {
        lines.push(row(k, d));
    }
    lines.push(blank());

    lines.push(head("EDITOR — MOVEMENT & EDITING"));
    for (k, d) in [
        ("Ctrl+S", "save"),
        ("Esc", "back to reader (keeps the buffer)"),
        ("Shift+arrows", "extend selection"),
        ("Ctrl+←/→", "move by word"),
        ("Ctrl+A", "select all"),
        ("Ctrl+C/X/V", "copy / cut / paste (system clipboard via OSC 52)"),
        ("Ctrl+Z / Ctrl+Y", "undo / redo"),
        ("Ctrl+C", "quit from reader or file list"),
        ("Ctrl+D", "duplicate line"),
        ("Ctrl+K", "delete line"),
        ("Alt+↑ / Alt+↓", "move the line up / down"),
        ("Ctrl+U", "delete the word before the cursor"),
        ("Alt+K", "delete to end of line"),
        ("Tab / Shift+Tab", "indent / outdent"),
        ("Ctrl+F / Ctrl+R", "find / replace"),
        ("Ctrl+G", "go to line"),
        ("Enter", "smart: continues lists and quotes"),
    ] {
        lines.push(row(k, d));
    }
    lines.push(blank());

    lines.push(head("EDITOR — MARKDOWN TOOLS"));
    for (k, d) in [
        ("Ctrl+B / Alt+I", "bold / italic (selection or word)"),
        ("Alt+S", "strikethrough"),
        ("Alt+E", "inline code"),
        ("Alt+C", "wrap selection in a code fence"),
        ("Ctrl+L", "insert link around selection"),
        ("Alt+1 … Alt+6", "set heading level"),
        ("Alt+0", "remove heading"),
        ("Alt+H", "cycle heading level"),
        ("Alt+L / Alt+O", "bullet / numbered list"),
        ("Alt+T / Alt+X", "task list / toggle done"),
        ("Alt+Q", "block quote"),
        ("Alt+B", "insert a GFM table"),
        ("Alt+A", "re-align the pipes of the table under the cursor"),
        ("Alt+-", "horizontal rule"),
    ] {
        lines.push(row(k, d));
    }

    let height = inner.height as usize;
    let max = lines.len().saturating_sub(height);
    let scroll = scroll.min(max);
    for (row, line) in lines.iter().skip(scroll).take(height).enumerate() {
        f.buffer_mut()
            .set_line(inner.x, inner.y + row as u16, line, inner.width);
    }
}

fn compress_path(path: &str) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy().to_string();
        if let Some(rest) = path.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}
