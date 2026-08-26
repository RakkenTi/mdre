//! Searching across the whole directory rather than inside one document.
//!
//! Two questions a folder of notes raises constantly: *which file mentions
//! this?* and *what links here?* Both are a walk over the markdown files with
//! a different test at the bottom, so they share one traversal.

use std::fs;
use std::path::{Path, PathBuf};

use crate::link::{self, Target};
use crate::workspace::is_markdown;

/// One matching line.
#[derive(Clone, Debug)]
pub struct Hit {
    pub path: PathBuf,
    /// Zero-based, to match the editor's line indexing.
    pub line: usize,
    /// The line itself, trimmed and shortened for display.
    pub text: String,
    /// Where the match starts within `text`, in characters, for highlighting.
    pub col: usize,
}

/// Stop before a runaway walk hangs the UI. Generous enough that a real notes
/// directory never notices.
const MAX_HITS: usize = 500;
const MAX_DEPTH: usize = 8;
const MAX_FILE_BYTES: u64 = 4 << 20;
const SKIP_DIRS: &[&str] = &["node_modules", "target", "vendor", ".git", ".venv"];

/// Every line under `root` containing `needle`, case-insensitively.
pub fn grep(root: &Path, needle: &str, show_hidden: bool) -> Vec<Hit> {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    walk(root, show_hidden, &mut |path, text| {
        for (i, line) in text.lines().enumerate() {
            if hits.len() >= MAX_HITS {
                return;
            }
            let lower = line.to_lowercase();
            if let Some(byte) = lower.find(&needle) {
                hits.push(Hit {
                    path: path.to_path_buf(),
                    line: i,
                    col: line[..byte].chars().count(),
                    text: line.trim_end().to_string(),
                });
            }
        }
    });
    hits
}

/// Every line under `root` holding a link that resolves to `target`.
pub fn backlinks(root: &Path, target: &Path) -> Vec<Hit> {
    let Ok(target) = target.canonicalize() else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    walk(root, false, &mut |path, text| {
        if path == target {
            return;
        }
        for (i, line) in text.lines().enumerate() {
            if hits.len() >= MAX_HITS {
                return;
            }
            if let Some(col) = line_links_to(line, path, &target) {
                hits.push(Hit {
                    path: path.to_path_buf(),
                    line: i,
                    col,
                    text: line.trim_end().to_string(),
                });
            }
        }
    });
    hits
}

/// Character offset of the first link on `line` that resolves to `target`.
fn line_links_to(line: &str, source: &Path, target: &Path) -> Option<usize> {
    for (offset, url) in urls_in(line) {
        let Some(Target::File { path, .. }) = link::classify(&url, Some(source)) else {
            continue;
        };
        let resolved = link::resolve_file(&path)?;
        if resolved.canonicalize().is_ok_and(|p| p == target) {
            return Some(offset);
        }
    }
    None
}

/// Pull `(char offset, url)` out of every `](…)` on a line.
fn urls_in(line: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == ']' && chars[i + 1] == '(' {
            let start = i + 2;
            let mut depth = 1;
            let mut end = start;
            while end < chars.len() {
                match chars[end] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            if end < chars.len() {
                // A title after the destination — `](path "Title")` — is not
                // part of the path.
                let inner: String = chars[start..end].iter().collect();
                let url = inner.split_whitespace().next().unwrap_or("").to_string();
                if !url.is_empty() {
                    found.push((start, url));
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    found
}

/// Read every markdown file under `root`, handing each to `visit`.
fn walk(root: &Path, show_hidden: bool, visit: &mut impl FnMut(&Path, &str)) {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            continue;
        }
        let Ok(read) = fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && !show_hidden {
                continue;
            }
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if !is_markdown(&path) || meta.len() > MAX_FILE_BYTES {
                continue;
            }
            // Binary or mis-encoded files simply have no lines to offer.
            if let Ok(text) = fs::read_to_string(&path) {
                visit(&path, &text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_come_out_with_their_offsets() {
        let line = "see [a](./one.md) and [b](../two.md#x)";
        let got = urls_in(line);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].1, "./one.md");
        assert_eq!(got[1].1, "../two.md#x");
        assert_eq!(&line[got[0].0..got[0].0 + 8], "./one.md");
    }

    #[test]
    fn a_title_is_not_part_of_the_destination() {
        assert_eq!(urls_in(r#"[a](./one.md "Title")"#)[0].1, "./one.md");
    }

    #[test]
    fn parentheses_inside_a_destination_are_balanced() {
        assert_eq!(urls_in("[a](./f(1).md)")[0].1, "./f(1).md");
    }

    #[test]
    fn an_unclosed_destination_is_ignored() {
        assert!(urls_in("[a](./one.md").is_empty());
        assert!(urls_in("no links here").is_empty());
    }
}
