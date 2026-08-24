//! MDRE is a rust-based terminal markdown manager that lets users read and write github-flavoured markdown.
// Opus 5 was used to scaffold the initial 13 commits, after which I manually audited and reviewed everything.

mod ansi;
mod app;
mod clipboard;
mod config;
mod editor;
mod link;
mod md;
mod search;
mod syntax;
mod theme;
mod ui;
mod workspace;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind,
};
use crossterm::execute;

use app::{App, Mode};

const USAGE: &str = "\
mdre — terminal markdown manager

USAGE:
    mdre [OPTIONS] [PATH]

PATH may be a markdown file (opened for reading) or a directory to browse.
Defaults to the current directory.

OPTIONS:
    -e, --edit       open PATH straight in the editor
    -l, --light      start with the light theme
    -w, --split      start in split view (editor + live preview)
        --no-sidebar hide the file sidebar
    -r, --render     render PATH to stdout as ANSI and exit (no TUI)
        --width N    column width for --render (default: terminal, else 80)
    -h, --help       show this help
    -V, --version    show the version

Settings are read from $XDG_CONFIG_HOME/mdre/config.toml (default
~/.config/mdre/config.toml); see config.example.toml. Flags win over the file.
";

struct Args {
    path: Option<PathBuf>,
    edit: bool,
    light: bool,
    split: bool,
    sidebar: bool,
    render: bool,
    width: Option<u16>,
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args {
        path: None,
        edit: false,
        light: false,
        split: false,
        sidebar: true,
        render: false,
        width: None,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("mdre {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "-e" | "--edit" => args.edit = true,
            "-l" | "--light" => args.light = true,
            "-w" | "--split" => args.split = true,
            "--no-sidebar" => args.sidebar = false,
            "-r" | "--render" => args.render = true,
            "--width" => {
                let value = argv
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--width needs a number\n\n{USAGE}"))?;
                args.width = Some(value.parse().map_err(|_| {
                    anyhow::anyhow!("--width takes a number, got {value:?}\n\n{USAGE}")
                })?);
            }
            other if other.starts_with('-') => {
                anyhow::bail!("unknown option: {other}\n\n{USAGE}");
            }
            other => args.path = Some(PathBuf::from(other)),
        }
    }
    Ok(Some(args))
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };

    let cwd = std::env::current_dir()?;
    // p is expected to be a directory or a file.
    let (root, open) = match args.path {
        Some(p) if p.is_dir() => (p.canonicalize().unwrap_or(p), None),
        Some(p) => {
            let parent = p
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .map(|d| d.to_path_buf())
                .unwrap_or(cwd);
            let file = p.canonicalize().unwrap_or(p);
            (parent.canonicalize().unwrap_or(parent), Some(file))
        }
        None => (cwd, None),
    };

    // CLI flags override config values so config is loaded first, before the cli flags.
    let cfg = config::load();
    let default_theme: &'static theme::Theme = if args.light {
        &theme::LIGHT
    } else {
        cfg.theme.unwrap_or(&theme::DARK)
    };
    let palette = cfg.palette(default_theme);

    if args.render {
        let Some(file) = open else {
            anyhow::bail!("--render needs a markdown file\n\n{USAGE}");
        };
        return render_to_stdout(&file, palette, args.width.or(cfg.width), &cfg);
    }

    let mut app = App::new(root, open.clone());
    app.theme = palette;
    apply_config(&mut app, &cfg);

    app.sidebar = args.sidebar && cfg.sidebar.unwrap_or(true);
    if open.is_some() {
        app.mode = if args.edit { Mode::Edit } else { Mode::Read };
    }
    if args.split && open.is_some() {
        app.split = true;
        app.mode = Mode::Edit;
    }
    if !cfg.problems.is_empty() {
        // The status bar is one line and an absolute path would eat all of it,
        // so lead with what is actually wrong.
        app.warn(format!("config.toml — {}", cfg.problems.join("; ")));
    }

    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableBracketedPaste)?;
    install_panic_hook();

    let result = run(&mut terminal, &mut app);

    let _ = execute!(io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll so transient status messages expire without a keypress.
        if !event::poll(Duration::from_millis(500))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => app.on_key(key),
            Event::Paste(text) => app.on_paste(text),
            Event::Resize(_, _) => {}
            _ => {}
        }
        if app.quit {
            return Ok(());
        }
    }
}

/// Restore the terminal before printing a panic, so the message is readable.
fn install_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        ratatui::restore();
        hook(info);
    }));
}

/// `--render`: draw the document once, straight to stdout, and exit. Piping to
/// a pager or a file is the point, so the width comes from the terminal only
/// when there is one.
fn render_to_stdout(
    path: &PathBuf,
    theme: &'static theme::Theme,
    width: Option<u16>,
    cfg: &config::Config,
) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let width = width
        .or_else(|| crossterm::terminal::size().ok().map(|(w, _)| w))
        .unwrap_or(80)
        .max(20);

    let mut opts = md::render::RenderOpts::default();
    cfg.apply_render_opts(&mut opts);
    opts.max_width = width;
    let doc = md::render::render(&text, width.min(opts.max_width), theme, opts);
    let mut out = io::stdout().lock();
    ansi::write(&mut out, &doc.lines, theme.bg)
        .or_else(|e| if e.kind() == io::ErrorKind::BrokenPipe { Ok(()) } else { Err(e) })?;
    Ok(())
}

/// Fold the config file into a freshly built app. Only settings the file
/// actually mentions are touched; everything else keeps its default.
fn apply_config(app: &mut App, cfg: &config::Config) {
    cfg.apply_render_opts(&mut app.opts);
    if let Some(mode) = cfg.mode {
        app.mode = mode;
    }
    if let Some(sort) = cfg.sort {
        app.ws.sort = sort;
    }
    if let Some(v) = cfg.line_numbers {
        app.line_numbers = v;
    }
    if let Some(v) = cfg.wrap {
        app.wrap = v;
    }
    if let Some(v) = cfg.split {
        app.split = v;
    }
    if let Some(v) = cfg.tab_width {
        app.editor.tab_width = v;
    }
    if let Some(v) = cfg.hidden {
        app.ws.show_hidden = v;
    }
    if let Some(v) = cfg.markdown_only {
        app.ws.md_only = v;
    }
    if let Some(v) = cfg.recursive {
        app.ws.recursive = v;
    }
    app.ws.refresh();
}
