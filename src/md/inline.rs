//! Width-aware wrapping of styled inline runs.
//!
//! The renderer builds a paragraph as a flat list of styled spans and then asks
//! this module to break it into terminal lines. Wrapping has to be done here
//! rather than by `Paragraph::wrap` because every block carries a styled prefix
//! (quote bars, list markers, table borders) that must be re-emitted per line.

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn str_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| str_width(s.content.as_ref())).sum()
}

struct Chunk {
    text: String,
    style: Style,
    space: bool,
}

fn chunk_spans(spans: &[Span<'_>]) -> Vec<Chunk> {
    let mut out = Vec::new();
    for span in spans {
        let mut cur = String::new();
        let mut cur_space: Option<bool> = None;
        for ch in span.content.chars() {
            let is_space = ch.is_whitespace();
            if Some(is_space) != cur_space && !cur.is_empty() {
                out.push(Chunk {
                    text: std::mem::take(&mut cur),
                    style: span.style,
                    space: cur_space.unwrap_or(false),
                });
            }
            cur_space = Some(is_space);
            cur.push(if ch == '\n' || ch == '\t' { ' ' } else { ch });
        }
        if !cur.is_empty() {
            out.push(Chunk {
                text: cur,
                style: span.style,
                space: cur_space.unwrap_or(false),
            });
        }
    }
    out
}

/// Greedy word wrap. Returns at least one (possibly empty) line.
pub fn wrap(spans: &[Span<'_>], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let chunks = chunk_spans(spans);

    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut line: Vec<Span<'static>> = Vec::new();
    let mut line_w = 0usize;
    let mut pending: Vec<Chunk> = Vec::new();

    for chunk in chunks {
        if chunk.space {
            if !line.is_empty() {
                pending.push(chunk);
            }
            continue;
        }
        let word_w = str_width(&chunk.text);
        let pending_w: usize = pending.iter().map(|c| str_width(&c.text)).sum();

        if line_w > 0 && line_w + pending_w + word_w > width {
            lines.push(std::mem::take(&mut line));
            line_w = 0;
            pending.clear();
        } else if !pending.is_empty() {
            for p in pending.drain(..) {
                line_w += str_width(&p.text);
                line.push(Span::styled(p.text, p.style));
            }
        }

        if word_w + line_w <= width {
            line_w += word_w;
            line.push(Span::styled(chunk.text, chunk.style));
        } else {
            // A single word wider than the available width: hard-split it.
            let mut buf = String::new();
            let mut buf_w = 0usize;
            for ch in chunk.text.chars() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if line_w + buf_w + cw > width {
                    if !buf.is_empty() {
                        line.push(Span::styled(std::mem::take(&mut buf), chunk.style));
                    }
                    lines.push(std::mem::take(&mut line));
                    line_w = 0;
                    buf_w = 0;
                }
                buf.push(ch);
                buf_w += cw;
            }
            if !buf.is_empty() {
                line.push(Span::styled(buf, chunk.style));
                line_w += buf_w;
            }
        }
    }
    lines.push(line);
    lines
}

/// Clip styled spans to `width` columns, appending an ellipsis when cut.
pub fn truncate(spans: &[Span<'_>], width: usize, ellipsis_style: Style) -> Vec<Span<'static>> {
    if spans_width(spans) <= width {
        return spans
            .iter()
            .map(|s| Span::styled(s.content.to_string(), s.style))
            .collect();
    }
    if width == 0 {
        return Vec::new();
    }
    let budget = width.saturating_sub(1);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    'outer: for span in spans {
        let mut buf = String::new();
        for ch in span.content.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + cw > budget {
                if !buf.is_empty() {
                    out.push(Span::styled(buf, span.style));
                }
                break 'outer;
            }
            used += cw;
            buf.push(ch);
        }
        if !buf.is_empty() {
            out.push(Span::styled(buf, span.style));
        }
    }
    out.push(Span::styled("…", ellipsis_style));
    out
}

/// Pad styled spans out to `width` columns with `style`-colored blanks.
pub fn pad_to(spans: &mut Vec<Span<'static>>, width: usize, style: Style) {
    let w = spans_width(spans);
    if w < width {
        spans.push(Span::styled(" ".repeat(width - w), style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<Span<'static>> {
        vec![Span::raw(text.to_string())]
    }

    fn joined(lines: &[Vec<Span<'static>>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn wraps_on_word_boundaries() {
        let out = wrap(&spans("the quick brown fox"), 10);
        assert_eq!(joined(&out), vec!["the quick", "brown fox"]);
    }

    #[test]
    fn every_line_fits_the_width() {
        let out = wrap(&spans("alpha beta gamma delta epsilon zeta"), 12);
        for line in &out {
            assert!(spans_width(line) <= 12);
        }
    }

    #[test]
    fn splits_words_longer_than_the_width() {
        let out = wrap(&spans("aaaaaaaaaaaa"), 5);
        assert_eq!(joined(&out), vec!["aaaaa", "aaaaa", "aa"]);
    }

    #[test]
    fn keeps_styles_across_a_break() {
        let input = vec![
            Span::styled("hello ".to_string(), Style::default()),
            Span::styled("world".to_string(), Style::default()),
        ];
        let out = wrap(&input, 5);
        assert_eq!(joined(&out), vec!["hello", "world"]);
    }

    #[test]
    fn truncation_marks_the_cut() {
        let out = truncate(&spans("a rather long sentence"), 10, Style::default());
        let text: String = out.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.ends_with('…'));
        assert!(str_width(&text) <= 10);
    }

    #[test]
    fn truncation_leaves_short_input_alone() {
        let out = truncate(&spans("short"), 10, Style::default());
        let text: String = out.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "short");
    }

    #[test]
    fn wide_characters_count_as_two_columns() {
        assert_eq!(str_width("日本"), 4);
        let out = wrap(&spans("日本 語"), 4);
        assert_eq!(joined(&out), vec!["日本", "語"]);
    }
}
