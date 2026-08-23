//! Turning rendered lines into ANSI text, for `mdui --render`.
//!
//! The reader draws into a ratatui buffer; a pipe wants bytes. This walks the
//! same styled lines and emits SGR escapes, so `mdui --render notes.md | less
//! -R` shows exactly what the TUI would.

use std::io::{self, Write};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

/// Write `lines` as ANSI. Backgrounds equal to `page_bg` are left alone so the
/// output sits on the terminal's own background instead of painting ragged
/// blocks the width of each span; code blocks and tables keep theirs.
pub fn write(out: &mut impl Write, lines: &[Line<'_>], page_bg: Color) -> io::Result<()> {
    for line in lines {
        // The renderer emits a span per word, nearly all of them identically
        // styled; only switch when the style actually changes or the output is
        // several times larger than the text it carries.
        // Nothing is painted at the start of a line, and every line ends
        // with a reset, so an empty run is the honest starting state.
        let mut active: Vec<String> = Vec::new();
        for span in &line.spans {
            if span.content.is_empty() {
                continue;
            }
            let sgr = codes(span.style, page_bg);
            if active != sgr {
                if sgr.is_empty() {
                    out.write_all(b"\x1b[0m")?;
                } else {
                    write!(out, "\x1b[0;{}m", sgr.join(";"))?;
                }
                active = sgr;
            }
            out.write_all(span.content.as_bytes())?;
        }
        if !active.is_empty() {
            out.write_all(b"\x1b[0m")?;
        }
        out.write_all(b"\n")?;
    }
    out.flush()
}

fn codes(style: Style, page_bg: Color) -> Vec<String> {
    let mut codes = Vec::new();
    let m = style.add_modifier;
    for (modifier, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::REVERSED, "7"),
        (Modifier::CROSSED_OUT, "9"),
    ] {
        if m.contains(modifier) {
            codes.push(code.to_string());
        }
    }
    if let Some(fg) = style.fg {
        codes.extend(color(fg, false));
    }
    if let Some(bg) = style.bg.filter(|bg| *bg != page_bg) {
        codes.extend(color(bg, true));
    }
    codes
}

fn color(c: Color, background: bool) -> Vec<String> {
    let offset = if background { 10 } else { 0 };
    let basic = |n: u8| vec![(n + offset).to_string()];
    match c {
        Color::Reset => vec![(39 + offset).to_string()],
        Color::Black => basic(30),
        Color::Red => basic(31),
        Color::Green => basic(32),
        Color::Yellow => basic(33),
        Color::Blue => basic(34),
        Color::Magenta => basic(35),
        Color::Cyan => basic(36),
        Color::Gray => basic(37),
        Color::DarkGray => basic(90),
        Color::LightRed => basic(91),
        Color::LightGreen => basic(92),
        Color::LightYellow => basic(93),
        Color::LightBlue => basic(94),
        Color::LightMagenta => basic(95),
        Color::LightCyan => basic(96),
        Color::White => basic(97),
        Color::Indexed(i) => vec![(38 + offset).to_string(), "5".into(), i.to_string()],
        Color::Rgb(r, g, b) => vec![
            (38 + offset).to_string(),
            "2".into(),
            r.to_string(),
            g.to_string(),
            b.to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    fn render(spans: Vec<Span<'static>>, page_bg: Color) -> String {
        let mut out = Vec::new();
        write(&mut out, &[Line::from(spans)], page_bg).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn truecolor_and_modifiers_become_sgr() {
        let style = Style::default()
            .fg(Color::Rgb(1, 2, 3))
            .add_modifier(Modifier::BOLD);
        let got = render(vec![Span::styled("hi", style)], Color::Reset);
        assert_eq!(got, "\x1b[0;1;38;2;1;2;3mhi\x1b[0m\n");
    }

    #[test]
    fn the_page_background_is_left_to_the_terminal() {
        let bg = Color::Rgb(10, 10, 10);
        let plain = render(vec![Span::styled("x", Style::default().bg(bg))], bg);
        assert_eq!(plain, "x\n");

        let panel = Color::Rgb(20, 20, 20);
        let code = render(vec![Span::styled("x", Style::default().bg(panel))], bg);
        assert_eq!(code, "\x1b[0;48;2;20;20;20mx\x1b[0m\n");
    }

    #[test]
    fn a_run_of_one_style_is_written_once() {
        let red = Style::default().fg(Color::Red);
        let got = render(
            vec![
                Span::styled("a", red),
                Span::styled("b", red),
                Span::styled("c", red),
            ],
            Color::Reset,
        );
        assert_eq!(got, "\x1b[0;31mabc\x1b[0m\n");
    }

    #[test]
    fn styling_is_reset_between_spans_so_nothing_bleeds() {
        let got = render(
            vec![
                Span::styled("a", Style::default().fg(Color::Red)),
                Span::raw("b"),
            ],
            Color::Reset,
        );
        assert_eq!(got, "\x1b[0;31ma\x1b[0mb\n");
    }

    #[test]
    fn indexed_colors_use_the_256_form() {
        let got = render(
            vec![Span::styled("x", Style::default().fg(Color::Indexed(200)))],
            Color::Reset,
        );
        assert_eq!(got, "\x1b[0;38;5;200mx\x1b[0m\n");
    }
}
