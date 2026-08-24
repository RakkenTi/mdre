//! Color themes for every surface of the app.
//!
//! Everything visual pulls from here so that a single toggle (`F9`) can repaint
//! the whole UI: reader, editor syntax colors, code-block highlighting, chrome.

use ratatui::style::{Color, Modifier, Style};

const fn rgb(hex: u32) -> Color {
    Color::Rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

/// A complete palette. Fields are grouped by the surface that consumes them.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub name: &'static str,

    // Base surfaces
    pub bg: Color,
    pub panel_bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_alt: Color,

    // Chrome
    pub border: Color,
    pub border_focus: Color,
    pub status_bg: Color,
    pub status_fg: Color,
    pub mode_read_bg: Color,
    pub mode_edit_bg: Color,
    pub mode_files_bg: Color,
    pub sel_bg: Color,
    pub sel_fg: Color,
    pub cursor_line_bg: Color,
    pub match_bg: Color,

    // Semantic
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub info: Color,

    // Markdown rendering
    pub heading: [Color; 6],
    pub rule: Color,
    pub bullet: Color,
    pub number: Color,
    pub task_done: Color,
    pub task_todo: Color,
    pub quote_bar: Color,
    pub quote_fg: Color,
    pub link: Color,
    pub link_url: Color,
    pub image: Color,
    pub footnote: Color,
    pub table_border: Color,
    pub table_head: Color,
    pub html: Color,
    pub math: Color,
    pub meta_fg: Color,

    // Code blocks
    pub code_bg: Color,
    pub code_fg: Color,
    pub code_gutter: Color,
    pub code_label: Color,
    pub inline_code_bg: Color,
    pub inline_code_fg: Color,

    // Source syntax (code blocks *and* the markdown editor)
    pub syn_keyword: Color,
    pub syn_type: Color,
    pub syn_const: Color,
    pub syn_string: Color,
    pub syn_number: Color,
    pub syn_comment: Color,
    pub syn_func: Color,
    pub syn_punct: Color,
    pub syn_attr: Color,

    // Editor-only markdown source coloring
    pub src_marker: Color,
    pub src_gutter: Color,
    pub src_gutter_cur: Color,
    pub src_whitespace: Color,
}

pub const DARK: Theme = Theme {
    name: "dark",

    bg: rgb(0x12141A),
    panel_bg: rgb(0x161920),
    fg: rgb(0xD6DAE3),
    dim: rgb(0x8B93A7),
    faint: rgb(0x555D71),
    accent: rgb(0x6CB6FF),
    accent_alt: rgb(0xB78CFF),

    border: rgb(0x2A3040),
    border_focus: rgb(0x4C7FBF),
    status_bg: rgb(0x1D222C),
    status_fg: rgb(0xC3CAD9),
    mode_read_bg: rgb(0x2E7D9A),
    mode_edit_bg: rgb(0xC98A2B),
    mode_files_bg: rgb(0x6A5ACD),
    sel_bg: rgb(0x2C4A70),
    sel_fg: rgb(0xF2F5FA),
    cursor_line_bg: rgb(0x1B1F29),
    match_bg: rgb(0x5C4A12),

    ok: rgb(0x7EE787),
    warn: rgb(0xFFB454),
    err: rgb(0xFF7B72),
    info: rgb(0x6CB6FF),

    heading: [
        rgb(0xFFB454),
        rgb(0x6CB6FF),
        rgb(0x7EE787),
        rgb(0xB78CFF),
        rgb(0xF0A3C8),
        rgb(0x9AA3B8),
    ],
    rule: rgb(0x3A4152),
    bullet: rgb(0x6CB6FF),
    number: rgb(0xFFB454),
    task_done: rgb(0x7EE787),
    task_todo: rgb(0x8B93A7),
    quote_bar: rgb(0x556074),
    quote_fg: rgb(0xA8B0C2),
    link: rgb(0x6CB6FF),
    link_url: rgb(0x66708A),
    image: rgb(0xB78CFF),
    footnote: rgb(0xF0A3C8),
    table_border: rgb(0x3A4152),
    table_head: rgb(0xFFB454),
    html: rgb(0x6B7385),
    math: rgb(0x56B6C2),
    meta_fg: rgb(0x8B93A7),

    code_bg: rgb(0x1A1E27),
    code_fg: rgb(0xD6DAE3),
    code_gutter: rgb(0x3A4152),
    code_label: rgb(0x8B93A7),
    inline_code_bg: rgb(0x252B36),
    inline_code_fg: rgb(0xF0A3C8),

    syn_keyword: rgb(0xC678DD),
    syn_type: rgb(0x56B6C2),
    syn_const: rgb(0xD19A66),
    syn_string: rgb(0x98C379),
    syn_number: rgb(0xD19A66),
    syn_comment: rgb(0x6B7385),
    syn_func: rgb(0x61AFEF),
    syn_punct: rgb(0x9AA3B8),
    syn_attr: rgb(0xE5C07B),

    src_marker: rgb(0x6A7286),
    src_gutter: rgb(0x3A4152),
    src_gutter_cur: rgb(0xFFB454),
    src_whitespace: rgb(0x2A3040),
};

pub const LIGHT: Theme = Theme {
    name: "light",

    bg: rgb(0xFBFBFD),
    panel_bg: rgb(0xF2F3F7),
    fg: rgb(0x24292F),
    dim: rgb(0x5A6270),
    faint: rgb(0x8C93A0),
    accent: rgb(0x0969DA),
    accent_alt: rgb(0x8250DF),

    border: rgb(0xD0D7DE),
    border_focus: rgb(0x0969DA),
    status_bg: rgb(0xE7EAEF),
    status_fg: rgb(0x24292F),
    mode_read_bg: rgb(0x0969DA),
    mode_edit_bg: rgb(0xBF8700),
    mode_files_bg: rgb(0x8250DF),
    sel_bg: rgb(0xB6D7FF),
    sel_fg: rgb(0x0B1220),
    cursor_line_bg: rgb(0xEDF1F6),
    match_bg: rgb(0xFFE9A8),

    ok: rgb(0x1A7F37),
    warn: rgb(0x9A6700),
    err: rgb(0xCF222E),
    info: rgb(0x0969DA),

    heading: [
        rgb(0xBF3989),
        rgb(0x0969DA),
        rgb(0x1A7F37),
        rgb(0x8250DF),
        rgb(0xBC4C00),
        rgb(0x5A6270),
    ],
    rule: rgb(0xD0D7DE),
    bullet: rgb(0x0969DA),
    number: rgb(0xBC4C00),
    task_done: rgb(0x1A7F37),
    task_todo: rgb(0x5A6270),
    quote_bar: rgb(0xAFB8C1),
    quote_fg: rgb(0x545D68),
    link: rgb(0x0969DA),
    link_url: rgb(0x8C93A0),
    image: rgb(0x8250DF),
    footnote: rgb(0xBF3989),
    table_border: rgb(0xD0D7DE),
    table_head: rgb(0xBC4C00),
    html: rgb(0x8C93A0),
    math: rgb(0x1B7C83),
    meta_fg: rgb(0x5A6270),

    code_bg: rgb(0xF0F2F6),
    code_fg: rgb(0x24292F),
    code_gutter: rgb(0xCED4DA),
    code_label: rgb(0x5A6270),
    inline_code_bg: rgb(0xEAEEF2),
    inline_code_fg: rgb(0xBF3989),

    syn_keyword: rgb(0xCF222E),
    syn_type: rgb(0x1B7C83),
    syn_const: rgb(0x0550AE),
    syn_string: rgb(0x0A3069),
    syn_number: rgb(0x0550AE),
    syn_comment: rgb(0x6E7781),
    syn_func: rgb(0x8250DF),
    syn_punct: rgb(0x57606A),
    syn_attr: rgb(0x953800),

    src_marker: rgb(0x8C93A0),
    src_gutter: rgb(0xCED4DA),
    src_gutter_cur: rgb(0xBC4C00),
    src_whitespace: rgb(0xE1E5EA),
};

impl Theme {
    pub fn base(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }
    pub fn heading_style(&self, level: usize) -> Style {
        let idx = level.clamp(1, 6) - 1;
        let mut s = Style::default()
            .fg(self.heading[idx])
            .bg(self.bg)
            .add_modifier(Modifier::BOLD);
        if idx >= 3 {
            s = s.add_modifier(Modifier::ITALIC);
        }
        s
    }
    pub fn next(&self) -> &'static Theme {
        if self.name == "dark" { &LIGHT } else { &DARK }
    }
}

/// Naming palette entries from the config file.
///
/// The macro keeps the string names and the struct fields from drifting apart:
/// there is exactly one list, and it is the field list itself.
macro_rules! color_fields {
    ($($field:ident),* $(,)?) => {
        pub fn is_color_field(name: &str) -> bool {
            matches!(name, $(stringify!($field))|*) || heading_index(name).is_some()
        }

        /// Override one palette entry. Unknown names are checked by is_color_field (above).
        pub fn set_color_field(theme: &mut Theme, name: &str, color: Color) {
            match name {
                $(stringify!($field) => theme.$field = color,)*
                _ => {
                    if let Some(i) = heading_index(name) {
                        theme.heading[i] = color;
                    }
                }
            }
        }
    };
}

/// `heading1` … `heading6` address the per-level heading colours.
fn heading_index(name: &str) -> Option<usize> {
    let level: usize = name.strip_prefix("heading")?.parse().ok()?;
    (1..=6).contains(&level).then_some(level - 1)
}

color_fields![
    bg, panel_bg, fg, dim, faint, accent, accent_alt,
    border, border_focus, status_bg, status_fg,
    mode_read_bg, mode_edit_bg, mode_files_bg,
    sel_bg, sel_fg, cursor_line_bg, match_bg,
    ok, warn, err, info,
    rule, bullet, number, task_done, task_todo, quote_bar, quote_fg,
    link, link_url, image, footnote, table_border, table_head, html, math, meta_fg,
    code_bg, code_fg, code_gutter, code_label, inline_code_bg, inline_code_fg,
    syn_keyword, syn_type, syn_const, syn_string, syn_number, syn_comment,
    syn_func, syn_punct, syn_attr,
    src_marker, src_gutter, src_gutter_cur, src_whitespace,
];
