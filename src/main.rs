//! mdui — a terminal markdown manager: browse, read (GFM-rendered) and edit.

mod ansi;
mod app;
mod clipboard;
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
mdui — terminal markdown manager

USAGE:
    mdui [OPTIONS] [PATH]

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
                println!("mdui {}", env!("CARGO_PKG_VERSION"));
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

    let palette = if args.light { &theme::LIGHT } else { &theme::DARK };
    if args.render {
        let Some(file) = open else {
            anyhow::bail!("--render needs a markdown file\n\n{USAGE}");
        };
        return render_to_stdout(&file, palette, args.width);
    }

    let mut app = App::new(root, open.clone());
    if args.light {
        app.theme = &theme::LIGHT;
    }
    app.sidebar = args.sidebar;
    if open.is_some() {
        app.mode = if args.edit { Mode::Edit } else { Mode::Read };
    }
    if args.split && open.is_some() {
        app.split = true;
        app.mode = Mode::Edit;
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
fn render_to_stdout(path: &PathBuf, theme: &'static theme::Theme, width: Option<u16>) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let width = width
        .or_else(|| crossterm::terminal::size().ok().map(|(w, _)| w))
        .unwrap_or(80)
        .max(20);

    let opts = md::render::RenderOpts { max_width: width, ..Default::default() };
    let doc = md::render::render(&text, width.min(opts.max_width), theme, opts);
    let mut out = io::stdout().lock();
    ansi::write(&mut out, &doc.lines, theme.bg)
        .or_else(|e| if e.kind() == io::ErrorKind::BrokenPipe { Ok(()) } else { Err(e) })?;
    Ok(())
}
