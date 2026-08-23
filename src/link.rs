//! Resolving the targets of markdown links.
//!
//! A link in a document is just a string; this module decides what it means —
//! a heading in the current file, another file on disk, or something the OS
//! should handle — and turns relative paths into real ones.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `#section` — a heading in the document being read.
    Anchor(String),
    /// A path on disk, with an optional heading to land on once it is open.
    File { path: PathBuf, anchor: Option<String> },
    /// Anything carrying a scheme; handed to the desktop's opener.
    External(String),
}

/// Work out what `url` points at. `base` is the file the link was found in,
/// which relative paths resolve against.
pub fn classify(url: &str, base: Option<&Path>) -> Option<Target> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    if let Some(frag) = url.strip_prefix('#') {
        let anchor = slug(&decode(frag));
        return (!anchor.is_empty()).then_some(Target::Anchor(anchor));
    }
    if has_scheme(url) || url.starts_with("//") {
        return Some(Target::External(url.to_string()));
    }

    let (path, anchor) = match url.split_once('#') {
        Some((p, frag)) => (p, Some(slug(&decode(frag)))),
        None => (url, None),
    };
    // A bare `#` after the path, or a path that was nothing but a fragment.
    let anchor = anchor.filter(|a| !a.is_empty());
    if path.is_empty() {
        return anchor.map(Target::Anchor);
    }

    let decoded = decode(path);
    let raw = Path::new(&decoded);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        match base.and_then(Path::parent) {
            Some(dir) => dir.join(raw),
            None => raw.to_path_buf(),
        }
    };
    Some(Target::File { path: normalize(&joined), anchor })
}

/// Find the file a link actually names, allowing for the shorthands people
/// write by hand: `spec` for `spec.md`, and a directory for its index page.
pub fn resolve_file(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    if path.is_dir() {
        for index in ["README.md", "readme.md", "index.md"] {
            let candidate = path.join(index);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        // A real directory with no index is still a place we can go.
        return Some(path.to_path_buf());
    }
    if path.extension().is_none() {
        for ext in ["md", "markdown"] {
            let candidate = path.with_extension(ext);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// GitHub's heading-anchor slug: lowercase, punctuation dropped, spaces
/// hyphenated. Applied to both sides of a comparison so hand-written
/// `#My Section` matches a generated `#my-section`.
pub fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() {
            out.push('-');
        }
    }
    out
}

/// Hand a URL to the desktop. Every stream is null so a chatty opener cannot
/// scribble over the terminal we are drawing in.
pub fn open_external(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// True if `url` starts with something like `https:` — but not a Windows
/// drive letter, which is a path.
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
    if scheme.len() < 2 || url[..colon].contains(['/', '?', '#']) {
        return false;
    }
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Percent-decoding, enough for the `%20`s that appear in file links.
fn decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Collapse `.` and `..` without touching the disk, so links to files that do
/// not exist still report a tidy path.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("/notes/guide/intro.md")
    }

    fn file(url: &str) -> (PathBuf, Option<String>) {
        match classify(url, Some(&base())) {
            Some(Target::File { path, anchor }) => (path, anchor),
            other => panic!("expected a file target, got {other:?}"),
        }
    }

    #[test]
    fn schemes_go_to_the_desktop() {
        for url in ["https://example.com", "http://x.dev/a", "mailto:a@b.c"] {
            assert_eq!(
                classify(url, Some(&base())),
                Some(Target::External(url.to_string()))
            );
        }
    }

    #[test]
    fn fragments_stay_in_the_document() {
        assert_eq!(
            classify("#My Section!", None),
            Some(Target::Anchor("my-section".into()))
        );
    }

    #[test]
    fn relative_paths_resolve_against_the_open_file() {
        assert_eq!(file("./spec.md").0, PathBuf::from("/notes/guide/spec.md"));
        assert_eq!(file("../root.md").0, PathBuf::from("/notes/root.md"));
        assert_eq!(file("a/b/../c.md").0, PathBuf::from("/notes/guide/a/c.md"));
    }

    #[test]
    fn a_path_can_carry_an_anchor() {
        let (path, anchor) = file("../api.md#Return Values");
        assert_eq!(path, PathBuf::from("/notes/api.md"));
        assert_eq!(anchor.as_deref(), Some("return-values"));
    }

    #[test]
    fn escaped_spaces_survive() {
        assert_eq!(
            file("my%20notes.md").0,
            PathBuf::from("/notes/guide/my notes.md")
        );
    }

    #[test]
    fn a_windows_drive_is_not_a_scheme() {
        assert!(matches!(
            classify(r"C:\notes\x.md", None),
            Some(Target::File { .. })
        ));
    }

    #[test]
    fn empty_and_bare_fragments_resolve_to_nothing() {
        assert_eq!(classify("", None), None);
        assert_eq!(classify("   ", None), None);
        assert_eq!(classify("#", None), None);
    }

    #[test]
    fn slugs_match_github() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("`code` and _under_"), "code-and-_under_");
        assert_eq!(slug("Étage 2"), "étage-2");
    }

    #[test]
    fn parent_of_a_relative_root_is_kept() {
        assert_eq!(
            classify("../../x.md", Some(Path::new("a.md"))),
            Some(Target::File {
                path: PathBuf::from("../../x.md"),
                anchor: None
            })
        );
    }
}
