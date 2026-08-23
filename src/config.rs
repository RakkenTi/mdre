//! Reading `~/.config/mdre/config.toml`.
//!
//! A deliberately small slice of TOML — `key = value` under `[section]`
//! headers, with `#` comments — because that is all a settings file needs and
//! a parser for it is shorter than the dependency that would replace it.
//! Anything unrecognised is reported rather than ignored, so a typo in a key
//! name doesn't silently do nothing.

use std::fs;
use std::path::PathBuf;

use ratatui::style::Color;

use crate::app::Mode;
use crate::md::render::RenderOpts;
use crate::theme::{self, Theme};
use crate::workspace::Sort;

#[derive(Default)]
pub struct Config {
    pub theme: Option<&'static Theme>,
    pub mode: Option<Mode>,
    pub sort: Option<Sort>,
    pub width: Option<u16>,
    pub tab_width: Option<usize>,
    pub show_urls: Option<bool>,
    pub heading_markers: Option<bool>,
    pub code_numbers: Option<bool>,
    pub line_numbers: Option<bool>,
    pub wrap: Option<bool>,
    pub sidebar: Option<bool>,
    pub split: Option<bool>,
    pub hidden: Option<bool>,
    pub markdown_only: Option<bool>,
    pub recursive: Option<bool>,
    /// Palette overrides, applied on top of whichever theme was chosen.
    pub colors: Vec<(String, Color)>,
    /// Complaints about the file, shown once on startup.
    pub problems: Vec<String>,
}

/// Where the settings file lives: `%APPDATA%\mdre\config.toml` on Windows,
/// `$XDG_CONFIG_HOME/mdre/config.toml` elsewhere, falling back to `~/.config`.
///
/// Windows sets neither `XDG_CONFIG_HOME` nor `HOME`, so a Unix-only lookup
/// finds nothing there and the config silently never loads.
pub fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("mdre").join("config.toml"))
}

#[cfg(windows)]
fn config_dir() -> Option<PathBuf> {
    // Roaming APPDATA is where per-user application settings belong; fall back
    // to the profile root for the rare environment that does not set it.
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(not(windows))]
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Load the config, or return an empty one if there isn't a file. A malformed
/// file never stops the program — it starts on defaults and says why.
pub fn load() -> Config {
    let mut config = Config::default();
    let Some(path) = path() else {
        return config;
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        // Not having a config is the normal case, not an error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return config,
        Err(e) => {
            config.problems.push(format!("{}: {e}", path.display()));
            return config;
        }
    };
    parse(&text, &mut config);
    config
}

fn parse(text: &str, config: &mut Config) {
    let mut section = String::new();
    for (n, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
            if !matches!(section.as_str(), "options" | "colors") {
                config
                    .problems
                    .push(format!("line {}: unknown section [{section}]", n + 1));
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            config
                .problems
                .push(format!("line {}: expected `key = value`", n + 1));
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = unquote(value.trim());
        let outcome = match section.as_str() {
            "colors" => set_color(config, &key, &value),
            // A bare key before any section header is treated as an option,
            // so a one-line config does not need a header.
            "options" | "" => set_option(config, &key, &value),
            _ => Ok(()),
        };
        if let Err(why) = outcome {
            config.problems.push(format!("line {}: {why}", n + 1));
        }
    }
}

fn set_option(config: &mut Config, key: &str, value: &str) -> Result<(), String> {
    match key {
        "theme" => {
            config.theme = Some(match value.to_ascii_lowercase().as_str() {
                "dark" => &theme::DARK,
                "light" => &theme::LIGHT,
                other => return Err(format!("theme must be dark or light, got {other:?}")),
            })
        }
        "mode" => {
            config.mode = Some(match value.to_ascii_lowercase().as_str() {
                "read" => Mode::Read,
                "edit" => Mode::Edit,
                "files" | "browser" => Mode::Browser,
                other => return Err(format!("mode must be read, edit or files, got {other:?}")),
            })
        }
        "sort" => {
            config.sort = Some(match value.to_ascii_lowercase().as_str() {
                "name" => Sort::Name,
                "size" => Sort::Size,
                "modified" | "time" => Sort::Modified,
                other => {
                    return Err(format!(
                        "sort must be name, size or modified, got {other:?}"
                    ));
                }
            })
        }
        "width" => config.width = Some(number(value)?.clamp(20, 400) as u16),
        "tab_width" => config.tab_width = Some(number(value)?.clamp(1, 16) as usize),
        "show_urls" => config.show_urls = Some(boolean(value)?),
        "heading_markers" => config.heading_markers = Some(boolean(value)?),
        "code_numbers" => config.code_numbers = Some(boolean(value)?),
        "line_numbers" => config.line_numbers = Some(boolean(value)?),
        "wrap" => config.wrap = Some(boolean(value)?),
        "sidebar" => config.sidebar = Some(boolean(value)?),
        "split" => config.split = Some(boolean(value)?),
        "hidden" => config.hidden = Some(boolean(value)?),
        "markdown_only" => config.markdown_only = Some(boolean(value)?),
        "recursive" => config.recursive = Some(boolean(value)?),
        other => return Err(format!("unknown option {other:?}")),
    }
    Ok(())
}

fn set_color(config: &mut Config, key: &str, value: &str) -> Result<(), String> {
    let color = parse_color(value)?;
    if !theme::is_color_field(key) {
        return Err(format!("unknown colour {key:?}"));
    }
    config.colors.push((key.to_string(), color));
    Ok(())
}

/// `#RRGGBB`, `RRGGBB`, or a 0-255 terminal palette index.
fn parse_color(value: &str) -> Result<Color, String> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let n = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
        return Ok(Color::Rgb(
            ((n >> 16) & 0xff) as u8,
            ((n >> 8) & 0xff) as u8,
            (n & 0xff) as u8,
        ));
    }
    if let Ok(index) = value.parse::<u8>() {
        return Ok(Color::Indexed(index));
    }
    Err(format!("{value:?} is not #RRGGBB or a 0-255 index"))
}

fn boolean(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        other => Err(format!("expected true or false, got {other:?}")),
    }
}

fn number(value: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("expected a number, got {value:?}"))
}

/// Drop a trailing `# comment`, but not a `#` inside a quoted string — which
/// is exactly where colour values live.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted && line[..i].trim_end().ends_with(['=', ']']) => {}
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

impl Config {
    /// Build the palette to run with: the chosen theme plus any overrides.
    ///
    /// The result is leaked because it lives for the whole process and every
    /// draw path already takes `&'static Theme`; one small allocation at
    /// startup is cheaper than threading a lifetime through the UI.
    pub fn palette(&self, fallback: &'static Theme) -> &'static Theme {
        let base = self.theme.unwrap_or(fallback);
        if self.colors.is_empty() {
            return base;
        }
        let mut theme = *base;
        for (field, color) in &self.colors {
            theme::set_color_field(&mut theme, field, *color);
        }
        Box::leak(Box::new(theme))
    }

    pub fn apply_render_opts(&self, opts: &mut RenderOpts) {
        if let Some(w) = self.width {
            opts.max_width = w;
        }
        if let Some(v) = self.show_urls {
            opts.show_urls = v;
        }
        if let Some(v) = self.heading_markers {
            opts.heading_markers = v;
        }
        if let Some(v) = self.code_numbers {
            opts.code_numbers = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> Config {
        let mut c = Config::default();
        parse(text, &mut c);
        c
    }

    #[test]
    fn the_config_path_resolves_on_this_platform() {
        // Guards the Windows branch: a Unix-only lookup returns None there,
        // which is how the config file came to be silently ignored.
        let path = path().expect("no config directory on this platform");
        assert!(path.ends_with("mdre/config.toml") || path.ends_with("mdre\\config.toml"));
        assert!(path.is_absolute());
    }

    #[test]
    fn options_and_colors_both_land() {
        let c = cfg(
            "[options]\ntheme = \"light\"\nwidth = 100\nwrap = false\n\
             [colors]\naccent = \"#FF8800\"\nlink = 45\n",
        );
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.theme.map(|t| t.name), Some("light"));
        assert_eq!(c.width, Some(100));
        assert_eq!(c.wrap, Some(false));
        assert_eq!(c.colors[0], ("accent".into(), Color::Rgb(0xFF, 0x88, 0x00)));
        assert_eq!(c.colors[1], ("link".into(), Color::Indexed(45)));
    }

    #[test]
    fn a_header_is_optional_for_options() {
        assert_eq!(cfg("split = true").split, Some(true));
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let c = cfg("# a note\n\n  width = 80  # trailing\n");
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.width, Some(80));
    }

    #[test]
    fn a_hash_inside_a_colour_is_not_a_comment() {
        let c = cfg("[colors]\naccent = \"#123456\"\nfg = #abcdef\n");
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.colors.len(), 2);
    }

    #[test]
    fn typos_are_reported_rather_than_ignored() {
        let c = cfg("[options]\nwith = 90\ntheme = \"solarized\"\n[colours]\n");
        assert_eq!(c.problems.len(), 3);
        assert!(c.problems[0].contains("unknown option"));
        assert!(c.problems[1].contains("dark or light"));
        assert!(c.problems[2].contains("unknown section"));
    }

    #[test]
    fn out_of_range_values_are_clamped_not_rejected() {
        assert_eq!(cfg("width = 5").width, Some(20));
        assert_eq!(cfg("width = 9999").width, Some(400));
    }

    #[test]
    fn unknown_colour_names_are_caught() {
        let c = cfg("[colors]\nbackgrund = \"#000000\"\n");
        assert!(c.problems[0].contains("unknown colour"));
    }

    #[test]
    fn overrides_apply_on_top_of_the_chosen_theme() {
        let c = cfg("[colors]\naccent = \"#010203\"\n");
        let palette = c.palette(&theme::DARK);
        assert_eq!(palette.accent, Color::Rgb(1, 2, 3));
        assert_eq!(palette.fg, theme::DARK.fg);
    }
}
