//! Directory browsing and file operations for the manager pane.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    Name,
    Modified,
    Size,
}

impl Sort {
    pub fn label(&self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Modified => "modified",
            Sort::Size => "size",
        }
    }
    pub fn next(self) -> Sort {
        match self {
            Sort::Name => Sort::Modified,
            Sort::Modified => Sort::Size,
            Sort::Size => Sort::Name,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    /// Display name — the file name, or a relative path in recursive mode.
    pub name: String,
    pub is_dir: bool,
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// First `# heading` of a markdown file, when cheap to read.
    pub title: Option<String>,
}

pub struct Workspace {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub filter: String,
    pub sort: Sort,
    pub reverse: bool,
    pub show_hidden: bool,
    pub md_only: bool,
    pub recursive: bool,
    pub error: Option<String>,
}

const MD_EXT: &[&str] = &["md", "markdown", "mdown", "mkd", "mdx", "qmd", "rmd"];

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MD_EXT.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

impl Workspace {
    pub fn new(root: PathBuf) -> Self {
        let mut ws = Self {
            root,
            entries: Vec::new(),
            selected: 0,
            filter: String::new(),
            sort: Sort::Name,
            reverse: false,
            show_hidden: false,
            md_only: true,
            recursive: false,
            error: None,
        };
        ws.refresh();
        ws
    }

    pub fn refresh(&mut self) {
        let keep = self.selected_path();
        self.entries.clear();
        self.error = None;

        if self.root.parent().is_some() {
            self.entries.push(Entry {
                path: self.root.parent().unwrap().to_path_buf(),
                name: "..".into(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
                title: None,
            });
        }

        let result = if self.recursive {
            self.collect_recursive()
        } else {
            self.collect_flat()
        };
        if let Err(e) = result {
            self.error = Some(format!("{}: {e}", self.root.display()));
        }
        self.sort_entries();

        // Try to keep the cursor on the same file across refreshes.
        self.selected = keep
            .and_then(|p| self.entries.iter().position(|e| e.path == p))
            .unwrap_or(0)
            .min(self.entries.len().saturating_sub(1));
    }

    fn collect_flat(&mut self) -> io::Result<()> {
        let mut found = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !self.show_hidden && name.starts_with('.') {
                continue;
            }
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            if !is_dir && self.md_only && !is_markdown(&path) {
                continue;
            }
            found.push(Entry {
                title: (!is_dir && is_markdown(&path))
                    .then(|| read_title(&path))
                    .flatten(),
                name,
                is_dir,
                is_parent: false,
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
                path,
            });
        }
        self.entries.extend(found);
        Ok(())
    }

    fn collect_recursive(&mut self) -> io::Result<()> {
        let mut stack = vec![(self.root.clone(), 0usize)];
        let mut found = Vec::new();
        while let Some((dir, depth)) = stack.pop() {
            if depth > 8 || found.len() > 5000 {
                continue;
            }
            let Ok(read) = fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') && !self.show_hidden {
                    continue;
                }
                if matches!(name.as_str(), "node_modules" | "target" | "vendor" | ".git") {
                    continue;
                }
                let meta = entry.metadata().ok();
                let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                if is_dir {
                    stack.push((path, depth + 1));
                    continue;
                }
                if self.md_only && !is_markdown(&path) {
                    continue;
                }
                let rel = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                found.push(Entry {
                    title: is_markdown(&path).then(|| read_title(&path)).flatten(),
                    name: rel,
                    is_dir: false,
                    is_parent: false,
                    size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                    modified: meta.as_ref().and_then(|m| m.modified().ok()),
                    path,
                });
            }
        }
        self.entries.extend(found);
        Ok(())
    }

    fn sort_entries(&mut self) {
        let sort = self.sort;
        let reverse = self.reverse;
        self.entries.sort_by(|a, b| {
            if a.is_parent != b.is_parent {
                return b.is_parent.cmp(&a.is_parent);
            }
            if a.is_dir != b.is_dir {
                return b.is_dir.cmp(&a.is_dir);
            }
            let ord = match sort {
                Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Sort::Modified => b.modified.cmp(&a.modified),
                Sort::Size => b.size.cmp(&a.size),
            };
            let ord = if ord == Ordering::Equal {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            } else {
                ord
            };
            if reverse { ord.reverse() } else { ord }
        });
    }

    /// Indices of entries matching the current filter (fuzzy subsequence).
    pub fn visible(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.entries.len()).collect();
        }
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                // ".." is not a filter candidate; it would always sort first
                // and steal the selection from the real matches.
                !e.is_parent
                    && (fuzzy(&e.name, &self.filter)
                        || e.title.as_deref().is_some_and(|t| fuzzy(t, &self.filter)))
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_entry().map(|e| e.path.clone())
    }

    pub fn move_selection(&mut self, delta: isize) {
        let vis = self.visible();
        if vis.is_empty() {
            return;
        }
        let cur = vis.iter().position(|i| *i == self.selected).unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, vis.len() as isize - 1) as usize;
        self.selected = vis[next];
    }

    pub fn select_first(&mut self) {
        if let Some(&i) = self.visible().first() {
            self.selected = i;
        }
    }

    pub fn select_last(&mut self) {
        if let Some(&i) = self.visible().last() {
            self.selected = i;
        }
    }

    pub fn enter_dir(&mut self, path: PathBuf) {
        self.root = path.canonicalize().unwrap_or(path);
        self.filter.clear();
        self.selected = 0;
        self.refresh();
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.root.parent().map(Path::to_path_buf) {
            let from = self.root.clone();
            self.enter_dir(parent);
            if let Some(i) = self.entries.iter().position(|e| e.path == from) {
                self.selected = i;
            }
        }
    }

    pub fn create_file(&mut self, name: &str) -> io::Result<PathBuf> {
        let mut name = name.trim().to_string();
        if name.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty name"));
        }
        if Path::new(&name).extension().is_none() {
            name.push_str(".md");
        }
        let path = self.root.join(&name);
        if path.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "file exists"));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        fs::write(&path, format!("# {stem}\n\n"))?;
        self.refresh();
        if let Some(i) = self.entries.iter().position(|e| e.path == path) {
            self.selected = i;
        }
        Ok(path)
    }

    pub fn create_dir(&mut self, name: &str) -> io::Result<PathBuf> {
        let path = self.root.join(name.trim());
        fs::create_dir_all(&path)?;
        self.refresh();
        Ok(path)
    }

    pub fn rename(&mut self, new_name: &str) -> io::Result<PathBuf> {
        let Some(entry) = self.selected_entry().cloned() else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "nothing selected"));
        };
        if entry.is_parent {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot rename .."));
        }
        let target = entry
            .path
            .parent()
            .unwrap_or(&self.root)
            .join(new_name.trim());
        fs::rename(&entry.path, &target)?;
        self.refresh();
        if let Some(i) = self.entries.iter().position(|e| e.path == target) {
            self.selected = i;
        }
        Ok(target)
    }

    pub fn delete_selected(&mut self) -> io::Result<PathBuf> {
        let Some(entry) = self.selected_entry().cloned() else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "nothing selected"));
        };
        if entry.is_parent {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot delete .."));
        }
        if entry.is_dir {
            fs::remove_dir(&entry.path)?;
        } else {
            fs::remove_file(&entry.path)?;
        }
        self.refresh();
        Ok(entry.path)
    }
}

/// Case-insensitive subsequence match, as used by fuzzy finders.
pub fn fuzzy(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars().flat_map(char::to_lowercase);
    needle
        .chars()
        .flat_map(char::to_lowercase)
        .all(|n| chars.any(|h| h == n))
}

fn read_title(path: &Path) -> Option<String> {
    let data = read_prefix(path, 4096)?;
    let mut in_front_matter = false;
    for (i, line) in data.lines().enumerate() {
        let t = line.trim();
        if i == 0 && t == "---" {
            in_front_matter = true;
            continue;
        }
        if in_front_matter {
            if t == "---" {
                in_front_matter = false;
            } else if let Some(rest) = t.strip_prefix("title:") {
                let v = rest.trim().trim_matches(['"', '\'']).to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("# ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn read_prefix(path: &Path, limit: usize) -> Option<String> {
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; limit];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).to_string())
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = bytes as f64;
    let mut i = 0usize;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes}{}", UNITS[0])
    } else {
        format!("{v:.1}{}", UNITS[i])
    }
}

pub fn human_time(t: SystemTime) -> String {
    let Ok(elapsed) = t.elapsed() else {
        return "now".into();
    };
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        86400..=2591999 => format!("{}d ago", secs / 86400),
        _ => format!("{}mo ago", secs / 2592000),
    }
}
