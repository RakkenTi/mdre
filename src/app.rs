//! Application state and the key dispatch table.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::clipboard;
use crate::editor::{Editor, PrefixKind};
use crate::link;
use crate::md::render::{self, RenderOpts, Rendered};
use crate::theme::{self, Theme};
use crate::workspace::{self, Workspace};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Browser,
    Read,
    Edit,
}

impl Mode {
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Browser => "FILES",
            Mode::Read => "READ",
            Mode::Edit => "EDIT",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Ok,
    Warn,
    Err,
}

pub struct Status {
    pub text: String,
    pub kind: StatusKind,
    pub at: Instant,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PromptKind {
    NewFile,
    NewDir,
    Rename,
    ConfirmDelete,
    ConfirmQuit,
    Find,
    Replace,
    ReplaceWith(String),
    GotoLine,
    LinkUrl,
    FenceLang,
    SaveAs,
    OpenPath,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub title: String,
    pub input: String,
    pub cursor: usize,
}

impl Prompt {
    fn new(kind: PromptKind, title: &str, initial: &str) -> Self {
        Self {
            kind,
            title: title.to_string(),
            input: initial.to_string(),
            cursor: initial.chars().count(),
        }
    }
    pub fn insert(&mut self, c: char) {
        let at = byte_at(&self.input, self.cursor);
        self.input.insert(at, c);
        self.cursor += 1;
    }
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let a = byte_at(&self.input, self.cursor - 1);
        let b = byte_at(&self.input, self.cursor);
        self.input.replace_range(a..b, "");
        self.cursor -= 1;
    }
    pub fn delete(&mut self) {
        if self.cursor >= self.input.chars().count() {
            return;
        }
        let a = byte_at(&self.input, self.cursor);
        let b = byte_at(&self.input, self.cursor + 1);
        self.input.replace_range(a..b, "");
    }
}

fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(b, _)| b).unwrap_or(s.len())
}

pub enum Overlay {
    None,
    Help { scroll: usize },
    Palette { input: String, sel: usize },
    Prompt(Prompt),
    Links { sel: usize, broken: Vec<bool> },
    Headings { sel: usize },
}

#[derive(Default)]
pub struct SearchState {
    pub needle: String,
    pub matches: Vec<usize>,
    pub current: usize,
    pub case_sensitive: bool,
}

pub struct App {
    pub theme: &'static Theme,
    pub ws: Workspace,
    pub mode: Mode,
    pub editor: Editor,
    pub doc: Option<Rendered>,
    render_key: Option<(u16, u64, String, bool, bool, bool, u16)>,
    pub reader_scroll: usize,
    pub sidebar: bool,
    pub split: bool,
    pub outline: bool,
    pub line_numbers: bool,
    /// Soft-wrap long lines in the editor instead of scrolling horizontally.
    pub wrap: bool,
    pub opts: RenderOpts,
    pub status: Option<Status>,
    pub overlay: Overlay,
    pub search: SearchState,
    pub clipboard: String,
    pub quit: bool,
    pub reader_area: Rect,
    pub editor_area: Rect,
    pub list_area: Rect,
    pub outline_sel: usize,
    /// In the browser, typing goes to the filter instead of hotkeys.
    pub filtering: bool,
    /// Documents we followed a link out of, newest last.
    pub history: Vec<Crumb>,
    /// A heading to land on once the document we just opened has rendered.
    pending_anchor: Option<String>,
}

/// Where we were before following a link, so we can go back to it.
pub struct Crumb {
    pub path: PathBuf,
    pub scroll: usize,
    pub mode: Mode,
}

impl App {
    pub fn new(root: PathBuf, open: Option<PathBuf>) -> Self {
        let mut app = Self {
            theme: &theme::DARK,
            ws: Workspace::new(root),
            mode: Mode::Browser,
            editor: Editor::new(),
            doc: None,
            render_key: None,
            reader_scroll: 0,
            sidebar: true,
            split: false,
            outline: false,
            line_numbers: true,
            wrap: true,
            opts: RenderOpts::default(),
            status: None,
            overlay: Overlay::None,
            search: SearchState::default(),
            clipboard: String::new(),
            quit: false,
            reader_area: Rect::ZERO,
            editor_area: Rect::ZERO,
            list_area: Rect::ZERO,
            outline_sel: 0,
            filtering: false,
            history: Vec::new(),
            pending_anchor: None,
        };
        if let Some(path) = open {
            app.open_path(&path, Mode::Read);
        }
        app
    }

    // ------------------------------------------------------------- plumbing

    pub fn info(&mut self, msg: impl Into<String>) {
        self.set_status(msg, StatusKind::Info);
    }
    pub fn ok(&mut self, msg: impl Into<String>) {
        self.set_status(msg, StatusKind::Ok);
    }
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.set_status(msg, StatusKind::Warn);
    }
    pub fn err(&mut self, msg: impl Into<String>) {
        self.set_status(msg, StatusKind::Err);
    }
    fn set_status(&mut self, msg: impl Into<String>, kind: StatusKind) {
        self.status = Some(Status {
            text: msg.into(),
            kind,
            at: Instant::now(),
        });
    }
    pub fn status_visible(&self) -> Option<&Status> {
        self.status
            .as_ref()
            .filter(|s| s.at.elapsed() < Duration::from_secs(5))
    }

    pub fn has_file(&self) -> bool {
        self.editor.path.is_some()
    }

    pub fn title(&self) -> String {
        match &self.editor.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.display().to_string()),
            None => "untitled".into(),
        }
    }

    /// Render (or reuse) the preview for the current buffer at `width`.
    pub fn document(&mut self, width: u16) -> &Rendered {
        let key = (
            width,
            self.editor.revision,
            self.theme.name.to_string(),
            self.opts.show_urls,
            self.opts.code_numbers,
            self.opts.heading_markers,
            self.opts.max_width,
        );
        if self.render_key.as_ref() != Some(&key) || self.doc.is_none() {
            let text = self.editor.text();
            self.doc = Some(render::render(&text, width, self.theme, self.opts));
            self.render_key = Some(key);
        }
        if let Some(anchor) = self.pending_anchor.take() {
            let hit = self.doc.as_ref().and_then(|d| {
                d.toc
                    .iter()
                    .find(|e| link::slug(&e.title) == anchor)
                    .map(|e| (e.line, e.src_line))
            });
            match hit {
                Some((line, src)) => {
                    self.reader_scroll = line;
                    self.editor.goto_line(src);
                }
                None => self.warn(format!("no heading #{anchor}")),
            }
        }
        self.doc.as_ref().unwrap()
    }

    pub fn open_path(&mut self, path: &Path, mode: Mode) {
        match fs::read_to_string(path) {
            Ok(text) => {
                let mut ed = Editor::from_str(&text);
                ed.path = Some(path.to_path_buf());
                self.editor = ed;
                self.doc = None;
                self.render_key = None;
                self.reader_scroll = 0;
                self.search.matches.clear();
                self.mode = mode;
                self.ok(format!("opened {}", path.display()));
            }
            Err(e) => self.err(format!("cannot open {}: {e}", path.display())),
        }
    }

    // ------------------------------------------------------------- clipboard

    /// Put `text` on both clipboards: ours, which always works, and the
    /// system's via the terminal, which usually does.
    fn copy(&mut self, text: String, verb: &str) {
        if text.is_empty() {
            return;
        }
        let chars = text.chars().count();
        self.clipboard = text;
        let note = match clipboard::set(&self.clipboard) {
            Ok(true) => "",
            Ok(false) => " (too large for the system clipboard)",
            Err(_) => " (system clipboard unavailable)",
        };
        self.ok(format!("{verb} {chars} chars{note}"));
    }

    // ---------------------------------------------------------- link travel

    /// Open the Links overlay, checking up front which targets actually exist
    /// so broken ones can be shown as broken.
    fn open_links(&mut self) {
        let base = self.editor.path.clone();
        let broken = self
            .doc
            .as_ref()
            .map(|d| {
                d.links
                    .iter()
                    .map(|l| match link::classify(&l.url, base.as_deref()) {
                        Some(link::Target::File { path, .. }) => {
                            link::resolve_file(&path).is_none()
                        }
                        Some(_) => false,
                        None => true,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.overlay = Overlay::Links { sel: 0, broken };
    }

    /// Go where a link points: another heading, another file, or out to the
    /// desktop. Anything that moves us to a new file leaves a crumb behind.
    pub fn follow_link(&mut self, url: &str) {
        let base = self.editor.path.clone();
        match link::classify(url, base.as_deref()) {
            None => self.warn("that link has no target"),
            Some(link::Target::Anchor(anchor)) => {
                self.pending_anchor = Some(anchor);
            }
            Some(link::Target::External(url)) => match link::open_external(&url) {
                Ok(()) => self.ok(format!("opened {url}")),
                Err(e) => self.err(format!("cannot open {url}: {e}")),
            },
            Some(link::Target::File { path, anchor }) => {
                let Some(target) = link::resolve_file(&path) else {
                    self.err(format!("no such file: {}", path.display()));
                    return;
                };
                if target.is_dir() {
                    self.ws.enter_dir(target.clone());
                    self.mode = Mode::Browser;
                    self.ok(format!("entered {}", target.display()));
                    return;
                }
                if self.editor.dirty {
                    self.warn("unsaved changes — save or reload before following");
                    return;
                }
                self.push_crumb();
                self.open_path(&target, Mode::Read);
                self.pending_anchor = anchor;
            }
        }
    }

    fn push_crumb(&mut self) {
        if let Some(path) = self.editor.path.clone() {
            // A long browse should not grow without bound; nobody retraces
            // more than a few dozen steps.
            if self.history.len() >= 64 {
                self.history.remove(0);
            }
            self.history.push(Crumb {
                path,
                scroll: self.reader_scroll,
                mode: self.mode,
            });
        }
    }

    /// Retrace one step, restoring the scroll position we left from.
    pub fn go_back(&mut self) {
        let Some(crumb) = self.history.pop() else {
            self.info("no earlier document");
            return;
        };
        if self.editor.dirty {
            self.warn("unsaved changes — save or reload before going back");
            self.history.push(crumb);
            return;
        }
        self.open_path(&crumb.path, crumb.mode);
        self.reader_scroll = crumb.scroll;
    }

    pub fn save(&mut self) {
        let Some(path) = self.editor.path.clone() else {
            self.overlay = Overlay::Prompt(Prompt::new(
                PromptKind::SaveAs,
                "Save as",
                &self.ws.root.join("untitled.md").display().to_string(),
            ));
            return;
        };
        match fs::write(&path, self.editor.text()) {
            Ok(()) => {
                self.editor.mark_saved();
                self.ws.refresh();
                let (lines, words, _) = self.editor.stats();
                self.ok(format!(
                    "saved {} · {lines} lines, {words} words",
                    path.display()
                ));
            }
            Err(e) => self.err(format!("save failed: {e}")),
        }
    }

    fn reload(&mut self) {
        if let Some(path) = self.editor.path.clone() {
            self.open_path(&path, self.mode);
        } else {
            self.warn("nothing to reload");
        }
    }

    fn toggle_theme(&mut self) {
        self.theme = self.theme.next();
        self.info(format!("{} theme", self.theme.name));
    }

    // ----------------------------------------------------------- navigation

    fn reader_height(&self) -> usize {
        self.reader_area.height.max(1) as usize
    }

    pub fn scroll_reader(&mut self, delta: isize) {
        let max = self
            .doc
            .as_ref()
            .map(|d| d.lines.len())
            .unwrap_or(0)
            .saturating_sub(self.reader_height().saturating_sub(1));
        let next = self.reader_scroll as isize + delta;
        self.reader_scroll = next.clamp(0, max as isize) as usize;
    }

    fn open_selected(&mut self) {
        let Some(entry) = self.ws.selected_entry().cloned() else {
            return;
        };
        if entry.is_dir {
            if entry.is_parent {
                self.ws.go_up();
            } else {
                self.ws.enter_dir(entry.path);
            }
        } else {
            self.open_path(&entry.path, Mode::Read);
        }
    }

    // --------------------------------------------------------- key dispatch

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        match &mut self.overlay {
            Overlay::None => {}
            _ => {
                self.overlay_key(key);
                return;
            }
        }
        match self.mode {
            Mode::Browser => self.browser_key(key),
            Mode::Read => self.reader_key(key),
            Mode::Edit => self.editor_key(key),
        }
    }

    pub fn on_paste(&mut self, text: String) {
        match (&mut self.overlay, self.mode) {
            (Overlay::Prompt(p), _) => {
                for c in text.chars().filter(|c| !c.is_control()) {
                    p.insert(c);
                }
            }
            (Overlay::Palette { input, .. }, _) => {
                input.push_str(text.trim());
            }
            (_, Mode::Edit) => {
                self.editor.insert_str(&text);
            }
            (_, Mode::Browser) => {
                self.ws.filter.push_str(text.trim());
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------- overlays

    fn overlay_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match &mut self.overlay {
            Overlay::Help { scroll } => match key.code {
                KeyCode::Down | KeyCode::Char('j') => *scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown | KeyCode::Char(' ') => *scroll += 10,
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                _ => self.overlay = Overlay::None,
            },
            Overlay::Links { sel, .. } => match key.code {
                KeyCode::Down | KeyCode::Char('j') => *sel += 1,
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                // Enter travels to the target; `g` goes to where the link sits
                // in the text, for when you want the surrounding sentence.
                KeyCode::Enter | KeyCode::Char('g') => {
                    let goto = key.code == KeyCode::Char('g');
                    let target = self
                        .doc
                        .as_ref()
                        .and_then(|d| d.links.get(*sel))
                        .map(|l| (l.line, l.url.clone()));
                    self.overlay = Overlay::None;
                    if let Some((line, url)) = target {
                        if goto {
                            self.reader_scroll = line.saturating_sub(2);
                            self.info(format!("link: {url}"));
                        } else {
                            self.follow_link(&url);
                        }
                    }
                }
                _ => self.overlay = Overlay::None,
            },
            Overlay::Headings { sel } => match key.code {
                KeyCode::Down | KeyCode::Char('j') => *sel += 1,
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                KeyCode::Enter => {
                    let target = self
                        .doc
                        .as_ref()
                        .and_then(|d| d.toc.get(*sel))
                        .map(|e| (e.line, e.src_line));
                    self.overlay = Overlay::None;
                    if let Some((line, src)) = target {
                        if self.mode == Mode::Edit {
                            self.editor.goto_line(src);
                        } else {
                            self.reader_scroll = line;
                        }
                    }
                }
                _ => self.overlay = Overlay::None,
            },
            Overlay::Palette { input, sel } => match key.code {
                KeyCode::Esc => self.overlay = Overlay::None,
                KeyCode::Backspace => {
                    input.pop();
                    *sel = 0;
                }
                KeyCode::Down => *sel += 1,
                KeyCode::Up => *sel = sel.saturating_sub(1),
                KeyCode::Char(c) if !ctrl => {
                    input.push(c);
                    *sel = 0;
                }
                KeyCode::Enter => {
                    let filtered = filter_commands(input);
                    let cmd = filtered.get(*sel).map(|c| c.cmd);
                    self.overlay = Overlay::None;
                    if let Some(cmd) = cmd {
                        self.run(cmd);
                    }
                }
                _ => {}
            },
            Overlay::Prompt(_) => self.prompt_key(key),
            Overlay::None => {}
        }
        // Keep palette selection inside the filtered list.
        if let Overlay::Palette { input, sel } = &mut self.overlay {
            let n = filter_commands(input).len();
            *sel = (*sel).min(n.saturating_sub(1));
        }
        if let Overlay::Links { sel, .. } = &mut self.overlay {
            let n = self.doc.as_ref().map(|d| d.links.len()).unwrap_or(0);
            *sel = (*sel).min(n.saturating_sub(1));
        }
        if let Overlay::Headings { sel } = &mut self.overlay {
            let n = self.doc.as_ref().map(|d| d.toc.len()).unwrap_or(0);
            *sel = (*sel).min(n.saturating_sub(1));
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        let Overlay::Prompt(prompt) = &mut self.overlay else {
            return;
        };
        // Yes/no prompts answer on a single key.
        if matches!(prompt.kind, PromptKind::ConfirmDelete | PromptKind::ConfirmQuit) {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let kind = prompt.kind.clone();
                    self.overlay = Overlay::None;
                    self.confirm(kind);
                }
                _ => {
                    self.overlay = Overlay::None;
                    self.info("cancelled");
                }
            }
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.info("cancelled");
            }
            KeyCode::Enter => {
                let kind = prompt.kind.clone();
                let value = prompt.input.clone();
                self.overlay = Overlay::None;
                self.submit(kind, value);
            }
            KeyCode::Backspace => prompt.backspace(),
            KeyCode::Delete => prompt.delete(),
            KeyCode::Left => prompt.cursor = prompt.cursor.saturating_sub(1),
            KeyCode::Right => {
                prompt.cursor = (prompt.cursor + 1).min(prompt.input.chars().count())
            }
            KeyCode::Home => prompt.cursor = 0,
            KeyCode::End => prompt.cursor = prompt.input.chars().count(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => prompt.insert(c),
            _ => {}
        }
    }

    fn confirm(&mut self, kind: PromptKind) {
        match kind {
            PromptKind::ConfirmDelete => match self.ws.delete_selected() {
                Ok(path) => self.ok(format!("deleted {}", path.display())),
                Err(e) => self.err(format!("delete failed: {e}")),
            },
            PromptKind::ConfirmQuit => self.quit = true,
            _ => {}
        }
    }

    fn submit(&mut self, kind: PromptKind, value: String) {
        match kind {
            PromptKind::NewFile => match self.ws.create_file(&value) {
                Ok(path) => {
                    self.open_path(&path, Mode::Edit);
                    self.ok("created");
                }
                Err(e) => self.err(format!("create failed: {e}")),
            },
            PromptKind::NewDir => match self.ws.create_dir(&value) {
                Ok(_) => self.ok("directory created"),
                Err(e) => self.err(format!("mkdir failed: {e}")),
            },
            PromptKind::Rename => match self.ws.rename(&value) {
                Ok(path) => {
                    if self.editor.path.as_deref() == Some(path.as_path()) {
                        self.editor.path = Some(path.clone());
                    }
                    self.ok(format!("renamed to {}", path.display()));
                }
                Err(e) => self.err(format!("rename failed: {e}")),
            },
            PromptKind::SaveAs => {
                let path = PathBuf::from(shellexpand(&value));
                match fs::write(&path, self.editor.text()) {
                    Ok(()) => {
                        self.editor.path = Some(path.clone());
                        self.editor.mark_saved();
                        self.ws.refresh();
                        self.ok(format!("saved {}", path.display()));
                    }
                    Err(e) => self.err(format!("save failed: {e}")),
                }
            }
            PromptKind::OpenPath => {
                let path = PathBuf::from(shellexpand(&value));
                if path.is_dir() {
                    self.ws.enter_dir(path);
                    self.mode = Mode::Browser;
                } else {
                    self.open_path(&path, Mode::Read);
                }
            }
            PromptKind::Find => {
                self.search.needle = value;
                self.run_search();
            }
            PromptKind::Replace => {
                if value.is_empty() {
                    return;
                }
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::ReplaceWith(value),
                    "Replace with",
                    "",
                ));
            }
            PromptKind::ReplaceWith(find) => {
                let n = self
                    .editor
                    .replace_all(&find, &value, self.search.case_sensitive);
                self.ok(format!("replaced {n} occurrence(s)"));
            }
            PromptKind::GotoLine => {
                if let Ok(n) = value.trim().parse::<usize>() {
                    let target = n.saturating_sub(1);
                    if self.mode == Mode::Edit {
                        self.editor.goto_line(target);
                    } else {
                        let line = self
                            .doc
                            .as_ref()
                            .map(|d| d.line_for_src(target))
                            .unwrap_or(0);
                        self.reader_scroll = line;
                    }
                } else {
                    self.err("not a line number");
                }
            }
            PromptKind::LinkUrl => {
                self.editor.insert_link(value.trim());
                self.ok("link inserted");
            }
            PromptKind::FenceLang => {
                self.editor.fence_selection(value.trim());
                self.mode = Mode::Edit;
            }
            PromptKind::ConfirmDelete | PromptKind::ConfirmQuit => {}
        }
    }

    fn run_search(&mut self) {
        let needle = self.search.needle.clone();
        if needle.is_empty() {
            self.search.matches.clear();
            return;
        }
        if self.mode == Mode::Edit {
            let hits = self.editor.find_all(&needle, self.search.case_sensitive);
            self.search.matches = hits.iter().map(|(p, _)| p.line).collect();
            if let Some((pos, _)) = hits.first() {
                self.editor.cursor = *pos;
                self.editor.anchor = None;
                self.ok(format!("{} match(es)", hits.len()));
            } else {
                self.warn("no matches");
            }
            self.search.current = 0;
        } else {
            let needle_l = needle.to_lowercase();
            let matches: Vec<usize> = self
                .doc
                .as_ref()
                .map(|d| {
                    d.lines
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| {
                            let text: String =
                                l.spans.iter().map(|s| s.content.as_ref()).collect();
                            text.to_lowercase().contains(&needle_l)
                        })
                        .map(|(i, _)| i)
                        .collect()
                })
                .unwrap_or_default();
            self.search.matches = matches;
            self.search.current = 0;
            if let Some(&line) = self.search.matches.first() {
                self.reader_scroll = line.saturating_sub(2);
                let n = self.search.matches.len();
                self.ok(format!("{n} match(es)"));
            } else {
                self.warn("no matches");
            }
        }
    }

    fn next_match(&mut self, forward: bool) {
        if self.search.matches.is_empty() {
            if !self.search.needle.is_empty() {
                self.run_search();
            }
            return;
        }
        let n = self.search.matches.len();
        self.search.current = if forward {
            (self.search.current + 1) % n
        } else {
            (self.search.current + n - 1) % n
        };
        let line = self.search.matches[self.search.current];
        if self.mode == Mode::Edit {
            self.editor.goto_line(line);
            if let Some((pos, _)) = self
                .editor
                .find_all(&self.search.needle, self.search.case_sensitive)
                .into_iter()
                .find(|(p, _)| p.line == line)
            {
                self.editor.cursor = pos;
            }
        } else {
            self.reader_scroll = line.saturating_sub(2);
        }
        self.info(format!("match {}/{}", self.search.current + 1, n));
    }

    // -------------------------------------------------------------- browser

    fn browser_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.filtering {
            self.filter_key(key);
            return;
        }
        if self.global_key(key) {
            return;
        }
        match key.code {
            KeyCode::Char('q') if !ctrl => self.request_quit(),
            KeyCode::Char('c') if ctrl => self.request_quit(),
            KeyCode::Down | KeyCode::Char('j') => self.ws.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.ws.move_selection(-1),
            KeyCode::PageDown => self.ws.move_selection(10),
            KeyCode::PageUp => self.ws.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => self.ws.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.ws.select_last(),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.open_selected(),
            KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => self.ws.go_up(),
            KeyCode::Char('e') => {
                if let Some(entry) = self.ws.selected_entry().cloned() {
                    if !entry.is_dir {
                        self.open_path(&entry.path, Mode::Edit);
                    }
                }
            }
            KeyCode::Char('n') => {
                self.overlay =
                    Overlay::Prompt(Prompt::new(PromptKind::NewFile, "New file", ""))
            }
            KeyCode::Char('N') => {
                self.overlay =
                    Overlay::Prompt(Prompt::new(PromptKind::NewDir, "New directory", ""))
            }
            KeyCode::Char('r') => {
                let name = self
                    .ws
                    .selected_entry()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                self.overlay = Overlay::Prompt(Prompt::new(PromptKind::Rename, "Rename", &name));
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                let name = self
                    .ws
                    .selected_entry()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                if !name.is_empty() && name != ".." {
                    self.overlay = Overlay::Prompt(Prompt::new(
                        PromptKind::ConfirmDelete,
                        &format!("Delete {name}? (y/n)"),
                        "",
                    ));
                }
            }
            KeyCode::Char('R') => {
                self.ws.refresh();
                self.info("refreshed");
            }
            KeyCode::Char('s') => {
                self.ws.sort = self.ws.sort.next();
                self.ws.refresh();
                let label = self.ws.sort.label();
                self.info(format!("sort by {label}"));
            }
            KeyCode::Char('S') => {
                self.ws.reverse = !self.ws.reverse;
                self.ws.refresh();
                self.info("sort order flipped");
            }
            KeyCode::Char('.') => {
                self.ws.show_hidden = !self.ws.show_hidden;
                self.ws.refresh();
                let on = self.ws.show_hidden;
                self.info(if on { "showing hidden" } else { "hiding hidden" });
            }
            KeyCode::Char('a') => {
                self.ws.md_only = !self.ws.md_only;
                self.ws.refresh();
                let md = self.ws.md_only;
                self.info(if md { "markdown only" } else { "all files" });
            }
            KeyCode::Char('*') => {
                self.ws.recursive = !self.ws.recursive;
                self.ws.refresh();
                let r = self.ws.recursive;
                self.info(if r { "recursive listing" } else { "this folder only" });
            }
            KeyCode::Char('/') => {
                self.ws.filter.clear();
                self.filtering = true;
                self.info("filter: type to narrow · Enter opens · Esc clears");
            }
            KeyCode::Esc => {
                if self.ws.filter.is_empty() {
                    if self.has_file() {
                        self.mode = Mode::Read;
                    }
                } else {
                    self.ws.filter.clear();
                }
            }
            _ => {}
        }
    }

    /// While filtering, letters narrow the list instead of triggering hotkeys.
    fn filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.ws.filter.clear();
                self.filtering = false;
            }
            KeyCode::Enter => {
                self.filtering = false;
                self.open_selected();
            }
            KeyCode::Backspace => {
                self.ws.filter.pop();
                if self.ws.filter.is_empty() {
                    self.filtering = false;
                }
                self.ws.select_first();
            }
            KeyCode::Down => self.ws.move_selection(1),
            KeyCode::Up => self.ws.move_selection(-1),
            KeyCode::PageDown => self.ws.move_selection(10),
            KeyCode::PageUp => self.ws.move_selection(-10),
            KeyCode::Tab => self.filtering = false,
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ws.filter.push(c);
                self.ws.select_first();
            }
            _ => {}
        }
    }

    // --------------------------------------------------------------- reader

    fn reader_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.global_key(key) {
            return;
        }
        let page = self.reader_height().saturating_sub(2).max(1) as isize;
        match key.code {
            KeyCode::Char('q') if !ctrl => self.request_quit(),
            KeyCode::Char('c') if ctrl => self.request_quit(),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_reader(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_reader(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_reader(page),
            KeyCode::PageUp => self.scroll_reader(-page),
            KeyCode::Char('d') => self.scroll_reader(page / 2),
            KeyCode::Char('u') if !ctrl => self.scroll_reader(-page / 2),
            KeyCode::Home | KeyCode::Char('g') => self.reader_scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll_reader(isize::MAX / 4),
            KeyCode::Backspace => self.go_back(),
            KeyCode::Char('y') => {
                let text = self.editor.text();
                self.copy(text, "copied the document —");
            }
            KeyCode::Char('}') | KeyCode::Char(']') => self.jump_heading(1),
            KeyCode::Char('{') | KeyCode::Char('[') => self.jump_heading(-1),
            KeyCode::Char('e') | KeyCode::Char('i') => self.mode = Mode::Edit,
            KeyCode::Tab => self.mode = Mode::Browser,
            KeyCode::Char('o') => {
                self.outline = !self.outline;
                self.sync_outline();
            }
            KeyCode::Char('L') => self.open_links(),
            KeyCode::Char('O') => self.overlay = Overlay::Headings { sel: 0 },
            KeyCode::Char('U') => {
                self.opts.show_urls = !self.opts.show_urls;
                let on = self.opts.show_urls;
                self.info(if on { "showing URLs" } else { "hiding URLs" });
            }
            KeyCode::Char('H') => {
                self.opts.heading_markers = !self.opts.heading_markers;
                let on = self.opts.heading_markers;
                self.info(if on {
                    "showing # heading markers"
                } else {
                    "hiding # heading markers"
                });
            }
            KeyCode::Char('#') => {
                self.opts.code_numbers = !self.opts.code_numbers;
                self.info("code line numbers toggled");
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.opts.max_width = (self.opts.max_width + 8).min(300);
                let w = self.opts.max_width;
                self.info(format!("text width {w}"));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.opts.max_width = self.opts.max_width.saturating_sub(8).max(32);
                let w = self.opts.max_width;
                self.info(format!("text width {w}"));
            }
            KeyCode::Char('/') => {
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::Find,
                    "Find in document",
                    &self.search.needle.clone(),
                ))
            }
            KeyCode::Char('n') => self.next_match(true),
            KeyCode::Char('N') => self.next_match(false),
            KeyCode::Esc => {
                if self.outline {
                    self.outline = false;
                } else {
                    self.mode = Mode::Browser;
                }
            }
            _ => {}
        }
    }

    fn jump_heading(&mut self, dir: isize) {
        let Some(doc) = self.doc.as_ref() else { return };
        let cur = self.reader_scroll;
        let target = if dir > 0 {
            doc.toc.iter().find(|t| t.line > cur).map(|t| t.line)
        } else {
            doc.toc.iter().rev().find(|t| t.line + 1 < cur).map(|t| t.line)
        };
        if let Some(line) = target {
            self.reader_scroll = line;
        }
    }

    fn sync_outline(&mut self) {
        if let Some(doc) = self.doc.as_ref() {
            let scroll = self.reader_scroll;
            self.outline_sel = doc
                .toc
                .iter()
                .rposition(|t| t.line <= scroll)
                .unwrap_or(0);
        }
    }

    // --------------------------------------------------------------- editor

    fn editor_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if self.global_key(key) {
            return;
        }

        // Markdown tools: Alt-based so plain typing stays untouched.
        if alt {
            match key.code {
                KeyCode::Char('1') => return self.editor.set_heading(1),
                KeyCode::Char('2') => return self.editor.set_heading(2),
                KeyCode::Char('3') => return self.editor.set_heading(3),
                KeyCode::Char('4') => return self.editor.set_heading(4),
                KeyCode::Char('5') => return self.editor.set_heading(5),
                KeyCode::Char('6') => return self.editor.set_heading(6),
                KeyCode::Char('0') => return self.editor.set_heading(0),
                KeyCode::Char('h') => return self.editor.cycle_heading(),
                KeyCode::Char('i') => return self.editor.toggle_wrap("*"),
                KeyCode::Char('e') => return self.editor.toggle_wrap("`"),
                KeyCode::Char('k') => return self.editor.kill_to_eol(),
                KeyCode::Char('l') => return self.editor.toggle_prefix(PrefixKind::Bullet),
                KeyCode::Char('o') => return self.editor.toggle_prefix(PrefixKind::Ordered),
                KeyCode::Char('t') => return self.editor.toggle_prefix(PrefixKind::Task),
                KeyCode::Char('x') => return self.editor.toggle_task(),
                KeyCode::Char('q') => return self.editor.toggle_prefix(PrefixKind::Quote),
                KeyCode::Char('s') => return self.editor.toggle_wrap("~~"),
                KeyCode::Char('-') => {
                    self.editor.insert_block("\n---\n");
                    return;
                }
                KeyCode::Char('b') => {
                    self.editor.insert_block(TABLE_SKELETON);
                    return self.info("table inserted");
                }
                KeyCode::Char('a') => return self.run(Cmd::FormatTable),
                KeyCode::Char('c') => {
                    self.overlay = Overlay::Prompt(Prompt::new(
                        PromptKind::FenceLang,
                        "Code fence language",
                        "",
                    ));
                    return;
                }
                KeyCode::Up => return self.editor.move_line(-1),
                KeyCode::Down => return self.editor.move_line(1),
                _ => {}
            }
        }

        if ctrl {
            match key.code {
                KeyCode::Char('s') => return self.save(),
                KeyCode::Char('z') => {
                    if !self.editor.undo() {
                        self.warn("nothing to undo");
                    }
                    return;
                }
                KeyCode::Char('y') => {
                    if !self.editor.redo() {
                        self.warn("nothing to redo");
                    }
                    return;
                }
                KeyCode::Char('b') => return self.editor.toggle_wrap("**"),
                KeyCode::Char('k') => {
                    self.overlay =
                        Overlay::Prompt(Prompt::new(PromptKind::LinkUrl, "Link URL", "https://"));
                    return;
                }
                KeyCode::Char('a') => return self.editor.select_all(),
                KeyCode::Char('d') => return self.editor.duplicate_line(),
                KeyCode::Char('l') => {
                    let line = self.editor.delete_line();
                    self.copy(line, "cut line —");
                    return;
                }
                KeyCode::Char('c') => {
                    if let Some(sel) = self.editor.selected_text() {
                        self.copy(sel, "copied");
                    }
                    return;
                }
                KeyCode::Char('x') => {
                    if let Some(sel) = self.editor.selected_text() {
                        self.editor.delete_selection();
                        self.copy(sel, "cut");
                    }
                    return;
                }
                KeyCode::Char('v') => {
                    let text = self.clipboard.clone();
                    if !text.is_empty() {
                        self.editor.insert_str(&text);
                    }
                    return;
                }
                KeyCode::Char('f') => {
                    self.overlay = Overlay::Prompt(Prompt::new(
                        PromptKind::Find,
                        "Find",
                        &self.search.needle.clone(),
                    ));
                    return;
                }
                KeyCode::Char('r') => {
                    self.overlay =
                        Overlay::Prompt(Prompt::new(PromptKind::Replace, "Replace what", ""));
                    return;
                }
                KeyCode::Char('g') => {
                    self.overlay =
                        Overlay::Prompt(Prompt::new(PromptKind::GotoLine, "Go to line", ""));
                    return;
                }
                KeyCode::Char('u') => return self.editor.delete_word_back(),
                KeyCode::Left => return self.editor.move_word(false, shift),
                KeyCode::Right => return self.editor.move_word(true, shift),
                KeyCode::Home => return self.editor.move_doc_start(shift),
                KeyCode::End => return self.editor.move_doc_end(shift),
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.editor.anchor = None;
                self.mode = Mode::Read;
                self.reader_scroll = self
                    .doc
                    .as_ref()
                    .map(|d| d.line_for_src(self.editor.cursor.line))
                    .unwrap_or(0)
                    .saturating_sub(2);
            }
            KeyCode::Char(c) => self.editor.insert_char(c),
            KeyCode::Enter => self.editor.newline(true),
            KeyCode::Backspace => self.editor.backspace(),
            KeyCode::Delete => self.editor.delete(),
            KeyCode::Tab => self.editor.indent(false),
            KeyCode::BackTab => self.editor.indent(true),
            KeyCode::Left => self.editor.move_left(shift),
            KeyCode::Right => self.editor.move_right(shift),
            KeyCode::Up => self.editor.move_vertical(-1, shift),
            KeyCode::Down => self.editor.move_vertical(1, shift),
            KeyCode::Home => self.editor.move_home(shift),
            KeyCode::End => self.editor.move_end(shift),
            KeyCode::PageUp => {
                let h = self.editor_area.height.max(3) as isize - 2;
                self.editor.move_vertical(-h, shift);
            }
            KeyCode::PageDown => {
                let h = self.editor_area.height.max(3) as isize - 2;
                self.editor.move_vertical(h, shift);
            }
            KeyCode::F(3) => self.next_match(!shift),
            _ => {}
        }
    }

    // -------------------------------------------------------- global chords

    /// Keys that work in every mode. Returns true when handled.
    fn global_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            // Reachable from the editor too, where Tab means indent.
            KeyCode::Char('f') if alt => {
                self.mode = Mode::Browser;
                true
            }
            KeyCode::F(1) => {
                self.overlay = Overlay::Help { scroll: 0 };
                true
            }
            KeyCode::Char('?') if self.mode != Mode::Edit => {
                self.overlay = Overlay::Help { scroll: 0 };
                true
            }
            KeyCode::F(2) => {
                self.sidebar = !self.sidebar;
                true
            }
            KeyCode::F(4) => {
                self.outline = !self.outline;
                self.sync_outline();
                true
            }
            KeyCode::F(5) => {
                self.reload();
                true
            }
            KeyCode::F(6) => {
                self.wrap = !self.wrap;
                let on = self.wrap;
                self.info(if on { "soft wrap on" } else { "soft wrap off" });
                true
            }
            KeyCode::F(9) => {
                self.toggle_theme();
                true
            }
            KeyCode::Char('p') if ctrl => {
                self.overlay = Overlay::Palette {
                    input: String::new(),
                    sel: 0,
                };
                true
            }
            KeyCode::Char('o') if ctrl => {
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::OpenPath,
                    "Open path",
                    &self.ws.root.display().to_string(),
                ));
                true
            }
            KeyCode::Char('q') if ctrl => {
                self.request_quit();
                true
            }
            KeyCode::Char('w') if ctrl => {
                self.split = !self.split;
                if self.split && self.mode != Mode::Edit {
                    self.mode = Mode::Edit;
                }
                let on = self.split;
                self.info(if on { "split preview on" } else { "split preview off" });
                true
            }
            KeyCode::Char('e') if ctrl => {
                self.mode = match self.mode {
                    Mode::Edit => Mode::Read,
                    _ => Mode::Edit,
                };
                true
            }
            KeyCode::Char('n') if ctrl => {
                self.overlay = Overlay::Prompt(Prompt::new(PromptKind::NewFile, "New file", ""));
                true
            }
            KeyCode::Tab if self.mode != Mode::Edit => {
                self.mode = match self.mode {
                    Mode::Browser if self.has_file() => Mode::Read,
                    _ => Mode::Browser,
                };
                true
            }
            _ => false,
        }
    }

    pub fn request_quit(&mut self) {
        if self.editor.dirty {
            self.overlay = Overlay::Prompt(Prompt::new(
                PromptKind::ConfirmQuit,
                "Unsaved changes — quit anyway? (y/n)",
                "",
            ));
        } else {
            self.quit = true;
        }
    }

    // ------------------------------------------------------------- commands

    pub fn run(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Save => self.save(),
            Cmd::SaveAs => {
                let initial = self
                    .editor
                    .path
                    .clone()
                    .unwrap_or_else(|| self.ws.root.join("untitled.md"));
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::SaveAs,
                    "Save as",
                    &initial.display().to_string(),
                ));
            }
            Cmd::Open => {
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::OpenPath,
                    "Open path",
                    &self.ws.root.display().to_string(),
                ))
            }
            Cmd::Reload => self.reload(),
            Cmd::Quit => self.request_quit(),
            Cmd::ModeRead => self.mode = Mode::Read,
            Cmd::ModeEdit => self.mode = Mode::Edit,
            Cmd::ModeFiles => self.mode = Mode::Browser,
            Cmd::ToggleSplit => {
                self.split = !self.split;
                if self.split {
                    self.mode = Mode::Edit;
                }
            }
            Cmd::ToggleSidebar => self.sidebar = !self.sidebar,
            Cmd::ToggleOutline => {
                self.outline = !self.outline;
                self.sync_outline();
            }
            Cmd::ToggleTheme => self.toggle_theme(),
            Cmd::ToggleUrls => self.opts.show_urls = !self.opts.show_urls,
            Cmd::ToggleCodeNumbers => self.opts.code_numbers = !self.opts.code_numbers,
            Cmd::ToggleHeadingMarkers => {
                self.opts.heading_markers = !self.opts.heading_markers
            }
            Cmd::ToggleLineNumbers => self.line_numbers = !self.line_numbers,
            Cmd::ToggleWrap => self.wrap = !self.wrap,
            Cmd::Bold => self.editor.toggle_wrap("**"),
            Cmd::Italic => self.editor.toggle_wrap("*"),
            Cmd::Strike => self.editor.toggle_wrap("~~"),
            Cmd::InlineCode => self.editor.toggle_wrap("`"),
            Cmd::CodeFence => {
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::FenceLang,
                    "Code fence language",
                    "",
                ))
            }
            Cmd::Link => {
                self.overlay =
                    Overlay::Prompt(Prompt::new(PromptKind::LinkUrl, "Link URL", "https://"))
            }
            Cmd::Heading(n) => self.editor.set_heading(n),
            Cmd::CycleHeading => self.editor.cycle_heading(),
            Cmd::Bullet => self.editor.toggle_prefix(PrefixKind::Bullet),
            Cmd::Ordered => self.editor.toggle_prefix(PrefixKind::Ordered),
            Cmd::Task => self.editor.toggle_prefix(PrefixKind::Task),
            Cmd::ToggleTask => self.editor.toggle_task(),
            Cmd::Quote => self.editor.toggle_prefix(PrefixKind::Quote),
            Cmd::Rule => self.editor.insert_block("\n---\n"),
            Cmd::Table => self.editor.insert_block(TABLE_SKELETON),
            Cmd::Indent => self.editor.indent(false),
            Cmd::Outdent => self.editor.indent(true),
            Cmd::Undo => {
                if !self.editor.undo() {
                    self.warn("nothing to undo");
                }
            }
            Cmd::Redo => {
                if !self.editor.redo() {
                    self.warn("nothing to redo");
                }
            }
            Cmd::Find => {
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::Find,
                    "Find",
                    &self.search.needle.clone(),
                ))
            }
            Cmd::Replace => {
                self.overlay = Overlay::Prompt(Prompt::new(PromptKind::Replace, "Replace what", ""))
            }
            Cmd::GotoLine => {
                self.overlay = Overlay::Prompt(Prompt::new(PromptKind::GotoLine, "Go to line", ""))
            }
            Cmd::ToggleCase => {
                self.search.case_sensitive = !self.search.case_sensitive;
                let on = self.search.case_sensitive;
                self.info(if on {
                    "search: case sensitive"
                } else {
                    "search: ignore case"
                });
            }
            Cmd::SelectAll => self.editor.select_all(),
            Cmd::DeleteLine => {
                let line = self.editor.delete_line();
                self.copy(line, "cut line —");
            }
            Cmd::DuplicateLine => self.editor.duplicate_line(),
            Cmd::MoveLineUp => self.editor.move_line(-1),
            Cmd::MoveLineDown => self.editor.move_line(1),
            Cmd::NewFile => {
                self.overlay = Overlay::Prompt(Prompt::new(PromptKind::NewFile, "New file", ""))
            }
            Cmd::NewDir => {
                self.overlay =
                    Overlay::Prompt(Prompt::new(PromptKind::NewDir, "New directory", ""))
            }
            Cmd::RenameFile => {
                let name = self
                    .ws
                    .selected_entry()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                self.overlay = Overlay::Prompt(Prompt::new(PromptKind::Rename, "Rename", &name));
            }
            Cmd::DeleteFile => {
                let name = self
                    .ws
                    .selected_entry()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                self.overlay = Overlay::Prompt(Prompt::new(
                    PromptKind::ConfirmDelete,
                    &format!("Delete {name}? (y/n)"),
                    "",
                ));
            }
            Cmd::Refresh => {
                self.ws.refresh();
                self.info("refreshed");
            }
            Cmd::ToggleHidden => {
                self.ws.show_hidden = !self.ws.show_hidden;
                self.ws.refresh();
            }
            Cmd::ToggleRecursive => {
                self.ws.recursive = !self.ws.recursive;
                self.ws.refresh();
            }
            Cmd::ToggleMdOnly => {
                self.ws.md_only = !self.ws.md_only;
                self.ws.refresh();
            }
            Cmd::CycleSort => {
                self.ws.sort = self.ws.sort.next();
                self.ws.refresh();
            }
            Cmd::Help => self.overlay = Overlay::Help { scroll: 0 },
            Cmd::Links => self.open_links(),
            Cmd::Headings => self.overlay = Overlay::Headings { sel: 0 },
            Cmd::Back => self.go_back(),
            Cmd::FormatTable => match self.editor.format_table() {
                Ok(rows) => self.ok(format!("table aligned — {rows} rows")),
                Err(why) => self.warn(why),
            },
            Cmd::CopyAll => {
                let text = self.editor.text();
                self.copy(text, "copied the document —");
            }
            Cmd::WordCount => {
                let (lines, words, chars) = self.editor.stats();
                let mins = (words as f64 / 220.0).ceil().max(1.0) as usize;
                self.ok(format!(
                    "{lines} lines · {words} words · {chars} chars · ~{mins} min read"
                ));
            }
        }
    }
}

pub const TABLE_SKELETON: &str = "\n| Column | Column | Column |\n| ------ | :----: | -----: |\n| left   | center | right  |\n| cell   | cell   | cell   |\n";

fn shellexpand(path: &str) -> String {
    let path = path.trim();
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest).display().to_string();
        }
    }
    path.to_string()
}

// ------------------------------------------------------------------ commands

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Save,
    SaveAs,
    Open,
    Reload,
    Quit,
    ModeRead,
    ModeEdit,
    ModeFiles,
    ToggleSplit,
    ToggleSidebar,
    ToggleOutline,
    ToggleTheme,
    ToggleUrls,
    ToggleCodeNumbers,
    ToggleHeadingMarkers,
    ToggleLineNumbers,
    ToggleWrap,
    Bold,
    Italic,
    Strike,
    InlineCode,
    CodeFence,
    Link,
    Heading(usize),
    CycleHeading,
    Bullet,
    Ordered,
    Task,
    ToggleTask,
    Quote,
    Rule,
    Table,
    Indent,
    Outdent,
    Undo,
    Redo,
    Find,
    Replace,
    GotoLine,
    ToggleCase,
    SelectAll,
    DeleteLine,
    DuplicateLine,
    MoveLineUp,
    MoveLineDown,
    NewFile,
    NewDir,
    RenameFile,
    DeleteFile,
    Refresh,
    ToggleHidden,
    ToggleRecursive,
    ToggleMdOnly,
    CycleSort,
    Help,
    Links,
    Headings,
    WordCount,
    Back,
    CopyAll,
    FormatTable,
}

pub struct CommandInfo {
    pub name: &'static str,
    pub keys: &'static str,
    pub group: &'static str,
    pub cmd: Cmd,
}

pub const COMMANDS: &[CommandInfo] = &[
    // File
    CommandInfo { name: "Save file", keys: "Ctrl+S", group: "File", cmd: Cmd::Save },
    CommandInfo { name: "Save as…", keys: "", group: "File", cmd: Cmd::SaveAs },
    CommandInfo { name: "Open path…", keys: "Ctrl+O", group: "File", cmd: Cmd::Open },
    CommandInfo { name: "Reload from disk", keys: "F5", group: "File", cmd: Cmd::Reload },
    CommandInfo { name: "New file", keys: "Ctrl+N", group: "File", cmd: Cmd::NewFile },
    CommandInfo { name: "New directory", keys: "N", group: "File", cmd: Cmd::NewDir },
    CommandInfo { name: "Rename file", keys: "r", group: "File", cmd: Cmd::RenameFile },
    CommandInfo { name: "Delete file", keys: "d", group: "File", cmd: Cmd::DeleteFile },
    CommandInfo { name: "Quit", keys: "Ctrl+Q", group: "File", cmd: Cmd::Quit },
    // View
    CommandInfo { name: "Reader mode", keys: "Ctrl+E", group: "View", cmd: Cmd::ModeRead },
    CommandInfo { name: "Editor mode", keys: "Ctrl+E", group: "View", cmd: Cmd::ModeEdit },
    CommandInfo { name: "File browser", keys: "Tab", group: "View", cmd: Cmd::ModeFiles },
    CommandInfo { name: "Toggle split preview", keys: "Ctrl+W", group: "View", cmd: Cmd::ToggleSplit },
    CommandInfo { name: "Toggle sidebar", keys: "F2", group: "View", cmd: Cmd::ToggleSidebar },
    CommandInfo { name: "Toggle outline", keys: "F4", group: "View", cmd: Cmd::ToggleOutline },
    CommandInfo { name: "Toggle theme (dark/light)", keys: "F9", group: "View", cmd: Cmd::ToggleTheme },
    CommandInfo { name: "Toggle URL display", keys: "U", group: "View", cmd: Cmd::ToggleUrls },
    CommandInfo { name: "Toggle code line numbers", keys: "#", group: "View", cmd: Cmd::ToggleCodeNumbers },
    CommandInfo { name: "Toggle # heading markers", keys: "H", group: "View", cmd: Cmd::ToggleHeadingMarkers },
    CommandInfo { name: "Toggle editor line numbers", keys: "", group: "View", cmd: Cmd::ToggleLineNumbers },
    CommandInfo { name: "Toggle editor soft wrap", keys: "F6", group: "View", cmd: Cmd::ToggleWrap },
    CommandInfo { name: "Show link list", keys: "L", group: "View", cmd: Cmd::Links },
    CommandInfo { name: "Back to previous document", keys: "Backspace", group: "View", cmd: Cmd::Back },
    CommandInfo { name: "Copy whole document to clipboard", keys: "y", group: "Edit", cmd: Cmd::CopyAll },
    CommandInfo { name: "Align the table under the cursor", keys: "Alt+A", group: "Edit", cmd: Cmd::FormatTable },
    CommandInfo { name: "Go to heading…", keys: "O", group: "View", cmd: Cmd::Headings },
    CommandInfo { name: "Document statistics", keys: "", group: "View", cmd: Cmd::WordCount },
    CommandInfo { name: "Help", keys: "F1", group: "View", cmd: Cmd::Help },
    // Format
    CommandInfo { name: "Bold", keys: "Ctrl+B", group: "Format", cmd: Cmd::Bold },
    CommandInfo { name: "Italic", keys: "Alt+I", group: "Format", cmd: Cmd::Italic },
    CommandInfo { name: "Strikethrough", keys: "Alt+S", group: "Format", cmd: Cmd::Strike },
    CommandInfo { name: "Inline code", keys: "Alt+E", group: "Format", cmd: Cmd::InlineCode },
    CommandInfo { name: "Code fence…", keys: "Alt+C", group: "Format", cmd: Cmd::CodeFence },
    CommandInfo { name: "Insert link…", keys: "Ctrl+K", group: "Format", cmd: Cmd::Link },
    CommandInfo { name: "Heading 1", keys: "Alt+1", group: "Format", cmd: Cmd::Heading(1) },
    CommandInfo { name: "Heading 2", keys: "Alt+2", group: "Format", cmd: Cmd::Heading(2) },
    CommandInfo { name: "Heading 3", keys: "Alt+3", group: "Format", cmd: Cmd::Heading(3) },
    CommandInfo { name: "Clear heading", keys: "Alt+0", group: "Format", cmd: Cmd::Heading(0) },
    CommandInfo { name: "Cycle heading level", keys: "Alt+H", group: "Format", cmd: Cmd::CycleHeading },
    CommandInfo { name: "Bullet list", keys: "Alt+L", group: "Format", cmd: Cmd::Bullet },
    CommandInfo { name: "Numbered list", keys: "Alt+O", group: "Format", cmd: Cmd::Ordered },
    CommandInfo { name: "Task list", keys: "Alt+T", group: "Format", cmd: Cmd::Task },
    CommandInfo { name: "Toggle task done", keys: "Alt+X", group: "Format", cmd: Cmd::ToggleTask },
    CommandInfo { name: "Block quote", keys: "Alt+Q", group: "Format", cmd: Cmd::Quote },
    CommandInfo { name: "Horizontal rule", keys: "Alt+-", group: "Format", cmd: Cmd::Rule },
    CommandInfo { name: "Insert table", keys: "Alt+B", group: "Format", cmd: Cmd::Table },
    CommandInfo { name: "Indent", keys: "Tab", group: "Format", cmd: Cmd::Indent },
    CommandInfo { name: "Outdent", keys: "Shift+Tab", group: "Format", cmd: Cmd::Outdent },
    // Edit
    CommandInfo { name: "Undo", keys: "Ctrl+Z", group: "Edit", cmd: Cmd::Undo },
    CommandInfo { name: "Redo", keys: "Ctrl+Y", group: "Edit", cmd: Cmd::Redo },
    CommandInfo { name: "Find…", keys: "Ctrl+F", group: "Edit", cmd: Cmd::Find },
    CommandInfo { name: "Replace…", keys: "Ctrl+R", group: "Edit", cmd: Cmd::Replace },
    CommandInfo { name: "Toggle case sensitivity", keys: "", group: "Edit", cmd: Cmd::ToggleCase },
    CommandInfo { name: "Go to line…", keys: "Ctrl+G", group: "Edit", cmd: Cmd::GotoLine },
    CommandInfo { name: "Select all", keys: "Ctrl+A", group: "Edit", cmd: Cmd::SelectAll },
    CommandInfo { name: "Delete line", keys: "Ctrl+L", group: "Edit", cmd: Cmd::DeleteLine },
    CommandInfo { name: "Duplicate line", keys: "Ctrl+D", group: "Edit", cmd: Cmd::DuplicateLine },
    CommandInfo { name: "Move line up", keys: "Alt+↑", group: "Edit", cmd: Cmd::MoveLineUp },
    CommandInfo { name: "Move line down", keys: "Alt+↓", group: "Edit", cmd: Cmd::MoveLineDown },
    // Browser
    CommandInfo { name: "Refresh listing", keys: "R", group: "Files", cmd: Cmd::Refresh },
    CommandInfo { name: "Toggle hidden files", keys: ".", group: "Files", cmd: Cmd::ToggleHidden },
    CommandInfo { name: "Toggle recursive listing", keys: "*", group: "Files", cmd: Cmd::ToggleRecursive },
    CommandInfo { name: "Toggle markdown-only", keys: "a", group: "Files", cmd: Cmd::ToggleMdOnly },
    CommandInfo { name: "Cycle sort order", keys: "s", group: "Files", cmd: Cmd::CycleSort },
];

pub fn filter_commands(query: &str) -> Vec<&'static CommandInfo> {
    COMMANDS
        .iter()
        .filter(|c| {
            query.is_empty()
                || workspace::fuzzy(c.name, query)
                || workspace::fuzzy(c.group, query)
        })
        .collect()
}
