//! Syntax colouring for *markdown source* — what editor mode displays.
//!
//! Unlike the reader, this preserves every character of the source (markers
//! included) and merely tints it, so the text you edit is exactly the text on
//! disk. Fenced code blocks are handed to the real code highlighter.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::syntax::{self, HlState, Lang, Tok};
use crate::theme::Theme;

/// Per-line block classification, computed in one pass over the document.
#[derive(Clone)]
pub enum BlockKind {
    Markdown,
    FenceMarker,
    Code {
        lang: Option<&'static Lang>,
        state: HlState,
    },
    FrontMatter,
    FrontMatterMarker,
}

/// Classify every line. Fence state and front matter need whole-document
/// context, so the editor caches this and invalidates it on edit.
pub fn scan(lines: &[String]) -> Vec<BlockKind> {
    let mut out = Vec::with_capacity(lines.len());
    let mut fence: Option<(char, usize, Option<&'static Lang>, HlState)> = None;
    let mut front_matter = !lines.is_empty() && lines[0].trim_end() == "---";

    for (i, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        let indent = raw.len() - trimmed.len();

        if front_matter {
            let marker = trimmed == "---" || trimmed == "...";
            out.push(if i == 0 || marker {
                BlockKind::FrontMatterMarker
            } else {
                BlockKind::FrontMatter
            });
            if i > 0 && marker {
                front_matter = false;
            }
            continue;
        }

        if let Some((ch, len, lang, state)) = fence.take() {
            let closes = indent <= 3
                && trimmed.starts_with(ch)
                && trimmed.chars().take_while(|c| *c == ch).count() >= len
                && trimmed.trim_end().chars().all(|c| c == ch);
            if closes {
                out.push(BlockKind::FenceMarker);
                continue;
            }
            let mut next = state;
            out.push(BlockKind::Code { lang, state: next });
            let _ = syntax::highlight(lang, raw, &mut next);
            fence = Some((ch, len, lang, next));
            continue;
        }

        let opener = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if opener && indent <= 3 {
            let ch = trimmed.chars().next().unwrap();
            let len = trimmed.chars().take_while(|c| *c == ch).count();
            let info = trimmed[len..].trim();
            fence = Some((ch, len, syntax::lang_for(info), HlState::default()));
            out.push(BlockKind::FenceMarker);
            continue;
        }

        out.push(BlockKind::Markdown);
    }
    out
}

/// Headings discovered by a source scan — powers the outline panel in editor mode.
pub fn outline(lines: &[String], kinds: &[BlockKind]) -> Vec<(u8, String, usize)> {
    let mut out = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        if !matches!(kinds.get(i), Some(BlockKind::Markdown)) {
            continue;
        }
        let t = raw.trim_start();
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && t.chars().nth(hashes).is_none_or(|c| c == ' ') {
            out.push((
                hashes as u8,
                t[hashes..].trim().trim_end_matches('#').trim().to_string(),
                i,
            ));
        }
    }
    out
}

pub fn highlight_line(line: &str, kind: &BlockKind, theme: &Theme) -> Vec<Span<'static>> {
    let base = Style::default().fg(theme.fg);
    match kind {
        BlockKind::FrontMatterMarker => vec![Span::styled(
            line.to_string(),
            Style::default().fg(theme.rule),
        )],
        BlockKind::FrontMatter => front_matter_line(line, theme),
        BlockKind::FenceMarker => {
            let t = line.trim_start();
            let indent = line.len() - t.len();
            let ch = t.chars().next().unwrap_or('`');
            let n = t.chars().take_while(|c| *c == ch).count();
            let mut spans = vec![];
            if indent > 0 {
                spans.push(Span::styled(line[..indent].to_string(), base));
            }
            spans.push(Span::styled(
                t[..n.min(t.len())].to_string(),
                Style::default().fg(theme.code_gutter),
            ));
            if t.len() > n {
                spans.push(Span::styled(
                    t[n..].to_string(),
                    Style::default()
                        .fg(theme.code_label)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            spans
        }
        BlockKind::Code { lang, state } => {
            let mut st = *state;
            syntax::highlight(*lang, line, &mut st)
                .into_iter()
                .map(|(text, tok)| Span::styled(text, tok_style(tok, theme)))
                .collect()
        }
        BlockKind::Markdown => markdown_line(line, theme),
    }
}

fn tok_style(kind: Tok, t: &Theme) -> Style {
    match kind {
        Tok::Text => Style::default().fg(t.code_fg),
        Tok::Keyword => Style::default().fg(t.syn_keyword).add_modifier(Modifier::BOLD),
        Tok::Type => Style::default().fg(t.syn_type),
        Tok::Const => Style::default().fg(t.syn_const),
        Tok::Str => Style::default().fg(t.syn_string),
        Tok::Number => Style::default().fg(t.syn_number),
        Tok::Comment => Style::default().fg(t.syn_comment).add_modifier(Modifier::ITALIC),
        Tok::Func => Style::default().fg(t.syn_func),
        Tok::Punct => Style::default().fg(t.syn_punct),
        Tok::Attr => Style::default().fg(t.syn_attr),
    }
}

fn front_matter_line(line: &str, theme: &Theme) -> Vec<Span<'static>> {
    match line.split_once(':') {
        Some((k, v)) if !k.trim().is_empty() && !k.starts_with(' ') => vec![
            Span::styled(format!("{k}:"), Style::default().fg(theme.accent)),
            Span::styled(v.to_string(), Style::default().fg(theme.meta_fg)),
        ],
        _ => vec![Span::styled(
            line.to_string(),
            Style::default().fg(theme.meta_fg),
        )],
    }
}

fn markdown_line(line: &str, theme: &Theme) -> Vec<Span<'static>> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut i = 0usize;

    // Leading indentation.
    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    if i > 0 {
        spans.push(Span::styled(
            chars[..i].iter().collect::<String>(),
            Style::default().fg(theme.src_whitespace),
        ));
    }
    let rest: String = chars[i..].iter().collect();

    // Thematic break
    let compact: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() >= 3
        && (compact.chars().all(|c| c == '-')
            || compact.chars().all(|c| c == '*')
            || compact.chars().all(|c| c == '_'))
    {
        spans.push(Span::styled(rest, Style::default().fg(theme.rule)));
        return spans;
    }

    // ATX heading
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && rest.chars().nth(hashes).is_none_or(|c| c == ' ') {
        let color = theme.heading[hashes - 1];
        spans.push(Span::styled(
            rest[..hashes].to_string(),
            Style::default().fg(color).add_modifier(Modifier::DIM),
        ));
        let body = &rest[hashes..];
        let mut inner = inline_spans(
            body,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
            theme,
        );
        spans.append(&mut inner);
        return spans;
    }

    // Blockquote markers (possibly nested)
    let mut rest = rest.as_str();
    let mut consumed = 0usize;
    while rest.starts_with('>') {
        let take = if rest[1..].starts_with(' ') { 2 } else { 1 };
        spans.push(Span::styled(
            rest[..take].to_string(),
            Style::default().fg(theme.quote_bar),
        ));
        rest = &rest[take..];
        consumed += take;
    }
    if consumed > 0 {
        let mut inner = markdown_line(rest, theme);
        spans.append(&mut inner);
        return spans;
    }

    // Reference definition:  [label]: https://…
    if rest.starts_with('[') {
        if let Some(close) = rest.find("]:") {
            spans.push(Span::styled(
                rest[..close + 2].to_string(),
                Style::default().fg(theme.link),
            ));
            spans.push(Span::styled(
                rest[close + 2..].to_string(),
                Style::default().fg(theme.link_url),
            ));
            return spans;
        }
    }

    // List markers
    if let Some((marker_len, ordered)) = list_marker(rest) {
        let style = if ordered {
            Style::default().fg(theme.number)
        } else {
            Style::default().fg(theme.bullet)
        };
        spans.push(Span::styled(rest[..marker_len].to_string(), style));
        let after = &rest[marker_len..];
        // Task list checkbox
        let lower = after.to_ascii_lowercase();
        if lower.starts_with("[ ] ") || lower.starts_with("[x] ") {
            let done = lower.starts_with("[x] ");
            spans.push(Span::styled(
                after[..3].to_string(),
                Style::default()
                    .fg(if done { theme.task_done } else { theme.task_todo })
                    .add_modifier(Modifier::BOLD),
            ));
            let mut style = Style::default().fg(theme.fg);
            if done {
                style = style
                    .add_modifier(Modifier::CROSSED_OUT)
                    .fg(theme.dim);
            }
            let mut inner = inline_spans(&after[3..], style, theme);
            spans.append(&mut inner);
            return spans;
        }
        let mut inner = inline_spans(after, Style::default().fg(theme.fg), theme);
        spans.append(&mut inner);
        return spans;
    }

    // Table rows
    let trimmed = rest.trim_end();
    if trimmed.starts_with('|') && trimmed.matches('|').count() >= 2 {
        let is_delim = trimmed
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' '));
        for part in split_keep(rest, '|') {
            let style = if part == "|" {
                Style::default().fg(theme.table_border)
            } else if is_delim {
                Style::default().fg(theme.table_border)
            } else {
                Style::default().fg(theme.fg)
            };
            if part == "|" || is_delim {
                spans.push(Span::styled(part.to_string(), style));
            } else {
                spans.append(&mut inline_spans(&part, style, theme));
            }
        }
        return spans;
    }

    spans.append(&mut inline_spans(rest, Style::default().fg(theme.fg), theme));
    spans
}

fn split_keep(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c == sep {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(sep.to_string());
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `("- ", false)` / `("12. ", true)` when the line opens a list item.
fn list_marker(s: &str) -> Option<(usize, bool)> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    if matches!(b[0], b'-' | b'*' | b'+') && b.get(1) == Some(&b' ') {
        return Some((2, false));
    }
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 {
        let next = b.get(digits).copied();
        if matches!(next, Some(b'.') | Some(b')')) && b.get(digits + 1) == Some(&b' ') {
            return Some((digits + 2, true));
        }
    }
    None
}

/// Inline markup: code spans, emphasis, links, images, autolinks, footnotes.
fn inline_spans(s: &str, base: Style, theme: &Theme) -> Vec<Span<'static>> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut plain = String::new();
    let marker = Style::default().fg(theme.src_marker);
    let mut i = 0usize;

    macro_rules! flush {
        () => {
            if !plain.is_empty() {
                out.push(Span::styled(std::mem::take(&mut plain), base));
            }
        };
    }

    while i < chars.len() {
        let c = chars[i];

        // Backslash escape
        if c == '\\' && i + 1 < chars.len() {
            flush!();
            out.push(Span::styled(
                chars[i..i + 2].iter().collect::<String>(),
                marker,
            ));
            i += 2;
            continue;
        }

        // Code span
        if c == '`' {
            let ticks = chars[i..].iter().take_while(|c| **c == '`').count();
            if let Some(close) = find_run(&chars, i + ticks, '`', ticks) {
                flush!();
                let code = Style::default()
                    .fg(theme.inline_code_fg)
                    .bg(theme.inline_code_bg);
                out.push(Span::styled(
                    chars[i..close + ticks].iter().collect::<String>(),
                    code,
                ));
                i = close + ticks;
                continue;
            }
        }

        // Strikethrough
        if c == '~' && chars.get(i + 1) == Some(&'~') {
            if let Some(close) = find_run(&chars, i + 2, '~', 2) {
                flush!();
                out.push(Span::styled("~~".to_string(), marker));
                out.append(&mut inline_spans(
                    &chars[i + 2..close].iter().collect::<String>(),
                    base.add_modifier(Modifier::CROSSED_OUT).fg(theme.dim),
                    theme,
                ));
                out.push(Span::styled("~~".to_string(), marker));
                i = close + 2;
                continue;
            }
        }

        // Emphasis / strong
        if c == '*' || c == '_' {
            let run = chars[i..].iter().take_while(|x| **x == c).count().min(3);
            if let Some(close) = find_run(&chars, i + run, c, run) {
                if close > i + run {
                    flush!();
                    let inner_style = match run {
                        1 => base.add_modifier(Modifier::ITALIC),
                        2 => base.add_modifier(Modifier::BOLD),
                        _ => base
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::ITALIC),
                    };
                    let mstyle = marker.add_modifier(Modifier::DIM);
                    out.push(Span::styled(
                        chars[i..i + run].iter().collect::<String>(),
                        mstyle,
                    ));
                    out.append(&mut inline_spans(
                        &chars[i + run..close].iter().collect::<String>(),
                        inner_style,
                        theme,
                    ));
                    out.push(Span::styled(
                        chars[close..close + run].iter().collect::<String>(),
                        mstyle,
                    ));
                    i = close + run;
                    continue;
                }
            }
        }

        // Image / link / footnote reference
        if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
            let img = c == '!';
            let open = if img { i + 1 } else { i };
            if let Some(close) = matching(&chars, open, '[', ']') {
                let is_link = chars.get(close + 1) == Some(&'(');
                let is_ref = chars.get(close + 1) == Some(&'[');
                let label: String = chars[open + 1..close].iter().collect();
                let footnote = label.starts_with('^');
                flush!();
                let accent = if img { theme.image } else { theme.link };
                out.push(Span::styled(
                    chars[i..open + 1].iter().collect::<String>(),
                    marker,
                ));
                if footnote {
                    out.push(Span::styled(
                        label,
                        Style::default()
                            .fg(theme.footnote)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    out.append(&mut inline_spans(
                        &label,
                        base.fg(accent).add_modifier(Modifier::UNDERLINED),
                        theme,
                    ));
                }
                out.push(Span::styled("]".to_string(), marker));
                i = close + 1;
                if is_link || is_ref {
                    let (o, cl) = if is_link { ('(', ')') } else { ('[', ']') };
                    if let Some(end) = matching(&chars, i, o, cl) {
                        out.push(Span::styled(
                            chars[i..=end].iter().collect::<String>(),
                            Style::default().fg(theme.link_url),
                        ));
                        i = end + 1;
                    }
                }
                continue;
            }
        }

        // Autolink / inline HTML
        if c == '<' {
            if let Some(close) = chars[i..].iter().position(|x| *x == '>') {
                let body: String = chars[i + 1..i + close].iter().collect();
                flush!();
                let style = if body.contains("://") || body.contains('@') {
                    Style::default()
                        .fg(theme.link)
                        .add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::default().fg(theme.html)
                };
                out.push(Span::styled(
                    chars[i..=i + close].iter().collect::<String>(),
                    style,
                ));
                i += close + 1;
                continue;
            }
        }

        plain.push(c);
        i += 1;
    }
    flush!();

    // A trailing double-space is a hard line break. Mark it by *styling* the
    // spaces rather than substituting glyphs — the editor must never show
    // characters the buffer doesn't contain.
    if s.ends_with("  ") && !s.trim().is_empty() {
        if let Some(last) = out.last_mut() {
            let content = last.content.to_string();
            if let Some(stripped) = content.strip_suffix("  ") {
                let style = last.style;
                *last = Span::styled(stripped.to_string(), style);
                out.push(Span::styled(
                    "  ".to_string(),
                    style
                        .bg(theme.src_whitespace)
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
        }
    }
    out
}

/// Position of the next run of exactly `n` `c` characters at or after `from`.
fn find_run(chars: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == c {
            let run = chars[i..].iter().take_while(|x| **x == c).count();
            if run >= n {
                return Some(i);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

/// Index of the bracket closing the one at `from`, honoring nesting.
fn matching(chars: &[char], from: usize, open: char, close: char) -> Option<usize> {
    if chars.get(from) != Some(&open) {
        return None;
    }
    let mut depth = 0usize;
    let mut i = from;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DARK;

    fn split(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    fn text_of(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn render(line: &str) -> Vec<Span<'static>> {
        highlight_line(line, &BlockKind::Markdown, &DARK)
    }

    #[test]
    fn highlighting_never_changes_the_text() {
        let cases = [
            "# Heading",
            "  - [ ] task **bold** `code`",
            "> quote with [link](http://x) and ~~strike~~",
            "| a | b |",
            "plain text with * unmatched and _ stray",
            "![img](a.png) trailing  ",
        ];
        for case in cases {
            assert_eq!(text_of(&render(case)), case, "round trip failed for {case:?}");
        }
    }

    #[test]
    fn fences_are_detected_and_closed() {
        let kinds = scan(&split("text\n```rust\nfn f() {}\n```\nafter"));
        assert!(matches!(kinds[0], BlockKind::Markdown));
        assert!(matches!(kinds[1], BlockKind::FenceMarker));
        assert!(matches!(kinds[2], BlockKind::Code { .. }));
        assert!(matches!(kinds[3], BlockKind::FenceMarker));
        assert!(matches!(kinds[4], BlockKind::Markdown));
    }

    #[test]
    fn front_matter_is_its_own_block() {
        let kinds = scan(&split("---\ntitle: x\n---\n# Doc"));
        assert!(matches!(kinds[0], BlockKind::FrontMatterMarker));
        assert!(matches!(kinds[1], BlockKind::FrontMatter));
        assert!(matches!(kinds[2], BlockKind::FrontMatterMarker));
        assert!(matches!(kinds[3], BlockKind::Markdown));
    }

    #[test]
    fn outline_skips_headings_inside_code() {
        let lines = split("# Real\n\n```sh\n# not a heading\n```\n\n## Also real");
        let kinds = scan(&lines);
        let found = outline(&lines, &kinds);
        let titles: Vec<&str> = found.iter().map(|(_, t, _)| t.as_str()).collect();
        assert_eq!(titles, vec!["Real", "Also real"]);
        assert_eq!(found[1].0, 2);
        assert_eq!(found[1].2, 6);
    }

    #[test]
    fn code_inside_a_fence_uses_the_fence_language() {
        let lines = split("```rust\nlet x = 1;\n```");
        let kinds = scan(&lines);
        let spans = highlight_line(&lines[1], &kinds[1], &DARK);
        assert_eq!(text_of(&spans), "let x = 1;");
        assert_eq!(spans[0].content.as_ref(), "let");
        assert_eq!(spans[0].style.fg, Some(DARK.syn_keyword));
    }

    #[test]
    fn heading_markers_are_dimmed_and_text_is_bold() {
        let spans = render("## Title");
        assert_eq!(spans[0].content.as_ref(), "##");
        assert!(spans.iter().any(|s| s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn task_checkboxes_are_coloured_by_state() {
        let done = render("- [x] finished");
        assert!(done.iter().any(|s| s.style.fg == Some(DARK.task_done)));
        let todo = render("- [ ] pending");
        assert!(todo.iter().any(|s| s.style.fg == Some(DARK.task_todo)));
    }

    #[test]
    fn thematic_breaks_and_tables_are_recognised() {
        assert_eq!(render("---")[0].style.fg, Some(DARK.rule));
        let row = render("| a | b |");
        assert!(row.iter().any(|s| s.style.fg == Some(DARK.table_border)));
    }
}
