//! The text buffer behind editor mode: cursor, selection, undo history, and the
//! markdown-aware editing tools (emphasis toggles, list/quote transforms, …).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::md::source::{self, BlockKind};
use crate::md::table;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EditKind {
    Insert,
    Delete,
    Other,
}

#[derive(Clone)]
struct Snap {
    lines: Vec<String>,
    cursor: Pos,
}

pub struct Editor {
    pub lines: Vec<String>,
    pub cursor: Pos,
    pub anchor: Option<Pos>,
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub dirty: bool,
    pub path: Option<PathBuf>,
    pub tab_width: usize,
    /// Bumped on every mutation so caches (rendered preview) can invalidate.
    pub revision: u64,
    /// Trailing newline present in the file as loaded, preserved on save.
    trailing_newline: bool,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last_kind: EditKind,
    last_edit: Option<Instant>,
    goal_col: Option<usize>,
    kinds: Option<Vec<BlockKind>>,
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn byte_of(s: &str, col: usize) -> usize {
    s.char_indices()
        .nth(col)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

impl Editor {
    pub fn new() -> Self {
        Self::from_str("")
    }

    pub fn from_str(text: &str) -> Self {
        let trailing_newline = text.ends_with('\n') || text.is_empty();
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines: Vec<String> = body.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
        Self {
            lines: if lines.is_empty() { vec![String::new()] } else { lines },
            cursor: Pos::default(),
            anchor: None,
            scroll_y: 0,
            scroll_x: 0,
            dirty: false,
            path: None,
            tab_width: 2,
            revision: 0,
            trailing_newline,
            undo: Vec::new(),
            redo: Vec::new(),
            last_kind: EditKind::Other,
            last_edit: None,
            goal_col: None,
            kinds: None,
        }
    }

    pub fn text(&self) -> String {
        let mut s = self.lines.join("\n");
        // A wholly empty buffer stays empty; anything else keeps the file's
        // original trailing-newline convention.
        if self.trailing_newline && !s.is_empty() {
            s.push('\n');
        }
        s
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn line_len(&self, i: usize) -> usize {
        char_len(self.line(i))
    }

    pub fn block_kinds(&mut self) -> &[BlockKind] {
        if self.kinds.is_none() {
            self.kinds = Some(source::scan(&self.lines));
        }
        self.kinds.as_ref().unwrap()
    }

    pub fn outline(&mut self) -> Vec<(u8, String, usize)> {
        let kinds = source::scan(&self.lines);
        source::outline(&self.lines, &kinds)
    }

    fn invalidate(&mut self) {
        self.kinds = None;
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        let words = self
            .lines
            .iter()
            .map(|l| l.split_whitespace().count())
            .sum();
        let chars: usize = self.lines.iter().map(|l| char_len(l)).sum();
        (self.lines.len(), words, chars)
    }

    /// Capture a snapshot of the editor state. Serves as a point for undo/redo.
    /// Undo/redo operations moves through snapshots.
    fn snapshot(&self) -> Snap {
        Snap {
            lines: self.lines.clone(),
            cursor: self.cursor,
        }
    }

    /// Record an undo point, coalescing runs of same-kind typing.
    fn checkpoint(&mut self, kind: EditKind) {
        let coalesce = kind != EditKind::Other
            && self.last_kind == kind
            && self
                .last_edit
                .is_some_and(|t| t.elapsed() < Duration::from_millis(700));
        self.last_kind = kind;
        self.last_edit = Some(Instant::now());
        if coalesce && !self.undo.is_empty() {
            return;
        }
        self.undo.push(self.snapshot());
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Force the next edit to start a new undo group.
    pub fn break_undo(&mut self) {
        self.last_kind = EditKind::Other;
    }

    pub fn undo(&mut self) -> bool {
        let Some(snap) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.lines = snap.lines;
        self.cursor = snap.cursor;
        self.clamp();
        self.anchor = None;
        self.invalidate();
        self.break_undo();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snap) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.lines = snap.lines;
        self.cursor = snap.cursor;
        self.clamp();
        self.anchor = None;
        self.invalidate();
        self.break_undo();
        true
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    fn clamp(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor.line = self.cursor.line.min(self.lines.len() - 1);
        self.cursor.col = self.cursor.col.min(self.line_len(self.cursor.line));
    }

    pub fn select_to(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    pub fn move_left(&mut self, extend: bool) {
        self.select_to(extend);
        self.goal_col = None;
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
        }
    }

    pub fn move_right(&mut self, extend: bool) {
        self.select_to(extend);
        self.goal_col = None;
        if self.cursor.col < self.line_len(self.cursor.line) {
            self.cursor.col += 1;
        } else if self.cursor.line + 1 < self.lines.len() {
            self.cursor.line += 1;
            self.cursor.col = 0;
        }
    }

    pub fn move_vertical(&mut self, delta: isize, extend: bool) {
        self.select_to(extend);
        let goal = *self.goal_col.get_or_insert(self.cursor.col);
        let target = self.cursor.line as isize + delta;
        self.cursor.line = target.clamp(0, self.lines.len() as isize - 1) as usize;
        self.cursor.col = goal.min(self.line_len(self.cursor.line));
    }

    pub fn move_home(&mut self, extend: bool) {
        self.select_to(extend);
        self.goal_col = None;
        // First press goes to the first non-blank character, second to column 0.
        let line = self.line(self.cursor.line);
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        self.cursor.col = if self.cursor.col == indent { 0 } else { indent };
    }

    pub fn move_end(&mut self, extend: bool) {
        self.select_to(extend);
        self.goal_col = None;
        self.cursor.col = self.line_len(self.cursor.line);
    }

    pub fn move_doc_start(&mut self, extend: bool) {
        self.select_to(extend);
        self.cursor = Pos::default();
        self.goal_col = None;
    }

    pub fn move_doc_end(&mut self, extend: bool) {
        self.select_to(extend);
        self.cursor.line = self.lines.len() - 1;
        self.cursor.col = self.line_len(self.cursor.line);
        self.goal_col = None;
    }

    pub fn move_word(&mut self, forward: bool, extend: bool) {
        self.select_to(extend);
        self.goal_col = None;
        let classify = |c: char| {
            if c.is_alphanumeric() || c == '_' {
                2
            } else if c.is_whitespace() {
                0
            } else {
                1
            }
        };
        if forward {
            loop {
                let line = chars(self.line(self.cursor.line));
                if self.cursor.col >= line.len() {
                    if self.cursor.line + 1 >= self.lines.len() {
                        return;
                    }
                    self.cursor.line += 1;
                    self.cursor.col = 0;
                    return;
                }
                let start = classify(line[self.cursor.col]);
                let mut col = self.cursor.col;
                while col < line.len() && classify(line[col]) == start {
                    col += 1;
                }
                while col < line.len() && classify(line[col]) == 0 {
                    col += 1;
                }
                self.cursor.col = col;
                return;
            }
        } else {
            if self.cursor.col == 0 {
                if self.cursor.line == 0 {
                    return;
                }
                self.cursor.line -= 1;
                self.cursor.col = self.line_len(self.cursor.line);
                return;
            }
            let line = chars(self.line(self.cursor.line));
            let mut col = self.cursor.col;
            while col > 0 && classify(line[col - 1]) == 0 {
                col -= 1;
            }
            if col > 0 {
                let kind = classify(line[col - 1]);
                while col > 0 && classify(line[col - 1]) == kind {
                    col -= 1;
                }
            }
            self.cursor.col = col;
        }
    }

    pub fn goto_line(&mut self, line: usize) {
        self.cursor.line = line.min(self.lines.len().saturating_sub(1));
        self.cursor.col = 0;
        self.anchor = None;
        self.goal_col = None;
    }

    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        Some(if a < self.cursor {
            (a, self.cursor)
        } else {
            (self.cursor, a)
        })
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        self.cursor = Pos {
            line: self.lines.len() - 1,
            col: self.line_len(self.lines.len() - 1),
        };
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        if a.line == b.line {
            let line = chars(self.line(a.line));
            return Some(line[a.col..b.col.min(line.len())].iter().collect());
        }
        let mut out = String::new();
        let first = chars(self.line(a.line));
        out.extend(&first[a.col.min(first.len())..]);
        for l in a.line + 1..b.line {
            out.push('\n');
            out.push_str(self.line(l));
        }
        out.push('\n');
        let last = chars(self.line(b.line));
        out.extend(&last[..b.col.min(last.len())]);
        Some(out)
    }

    fn remove_range(&mut self, a: Pos, b: Pos) {
        if a.line == b.line {
            let line = self.lines[a.line].clone();
            let s = byte_of(&line, a.col);
            let e = byte_of(&line, b.col);
            self.lines[a.line] = format!("{}{}", &line[..s], &line[e..]);
        } else {
            let head = {
                let line = &self.lines[a.line];
                line[..byte_of(line, a.col)].to_string()
            };
            let tail = {
                let line = &self.lines[b.line];
                line[byte_of(line, b.col)..].to_string()
            };
            self.lines.drain(a.line..=b.line);
            self.lines.insert(a.line, format!("{head}{tail}"));
        }
        self.cursor = a;
        self.anchor = None;
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            return false;
        };
        self.checkpoint(EditKind::Other);
        self.remove_range(a, b);
        self.invalidate();
        true
    }

    pub fn insert_char(&mut self, c: char) {
        self.checkpoint(EditKind::Insert);
        if self.selection().is_some() {
            let (a, b) = self.selection().unwrap();
            self.remove_range(a, b);
        }
        let line = &mut self.lines[self.cursor.line];
        let at = byte_of(line, self.cursor.col);
        line.insert(at, c);
        self.cursor.col += 1;
        self.goal_col = None;
        self.invalidate();
    }

    pub fn insert_str(&mut self, text: &str) {
        self.checkpoint(EditKind::Other);
        if let Some((a, b)) = self.selection() {
            self.remove_range(a, b);
        }
        for (i, piece) in text.split('\n').enumerate() {
            if i > 0 {
                self.raw_newline();
            }
            let line = &mut self.lines[self.cursor.line];
            let at = byte_of(line, self.cursor.col);
            line.insert_str(at, piece);
            self.cursor.col += char_len(piece);
        }
        self.invalidate();
    }

    fn raw_newline(&mut self) {
        let line = self.lines[self.cursor.line].clone();
        let at = byte_of(&line, self.cursor.col);
        self.lines[self.cursor.line] = line[..at].to_string();
        self.lines.insert(self.cursor.line + 1, line[at..].to_string());
        self.cursor.line += 1;
        self.cursor.col = 0;
    }

    /// Enter, with markdown continuation: keeps quote bars and list markers,
    /// increments ordered numbers, and clears the marker on an empty item.
    pub fn newline(&mut self, smart: bool) {
        self.checkpoint(EditKind::Other);
        if let Some((a, b)) = self.selection() {
            self.remove_range(a, b);
        }
        let cur = self.lines[self.cursor.line].clone();
        let prefix = if smart { continuation_prefix(&cur) } else { None };

        if let Some(ref p) = prefix {
            let after_marker: String = cur.chars().skip(char_len(&p.matched)).collect();
            if after_marker.trim().is_empty() && self.cursor.col >= char_len(&p.matched) {
                // "Enter on an empty list item" ends the list instead of nesting.
                self.lines[self.cursor.line] = String::new();
                self.cursor.col = 0;
                self.raw_newline();
                self.invalidate();
                return;
            }
        }
        self.raw_newline();
        if let Some(p) = prefix {
            let line = &mut self.lines[self.cursor.line];
            line.insert_str(0, &p.next);
            self.cursor.col = char_len(&p.next);
        }
        self.goal_col = None;
        self.invalidate();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.checkpoint(EditKind::Delete);
        if self.cursor.col > 0 {
            // Inside leading whitespace, delete a full indent step.
            let line = self.lines[self.cursor.line].clone();
            let before: String = line.chars().take(self.cursor.col).collect();
            let step = self.tab_width;
            if before.chars().all(|c| c == ' ') && !before.is_empty() && before.len() % step == 0 {
                let take = step.min(self.cursor.col);
                let s = byte_of(&line, self.cursor.col - take);
                let e = byte_of(&line, self.cursor.col);
                self.lines[self.cursor.line] = format!("{}{}", &line[..s], &line[e..]);
                self.cursor.col -= take;
            } else {
                let s = byte_of(&line, self.cursor.col - 1);
                let e = byte_of(&line, self.cursor.col);
                self.lines[self.cursor.line] = format!("{}{}", &line[..s], &line[e..]);
                self.cursor.col -= 1;
            }
        } else if self.cursor.line > 0 {
            let cur = self.lines.remove(self.cursor.line);
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
            self.lines[self.cursor.line].push_str(&cur);
        }
        self.goal_col = None;
        self.invalidate();
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.checkpoint(EditKind::Delete);
        let len = self.line_len(self.cursor.line);
        if self.cursor.col < len {
            let line = self.lines[self.cursor.line].clone();
            let s = byte_of(&line, self.cursor.col);
            let e = byte_of(&line, self.cursor.col + 1);
            self.lines[self.cursor.line] = format!("{}{}", &line[..s], &line[e..]);
        } else if self.cursor.line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor.line + 1);
            self.lines[self.cursor.line].push_str(&next);
        }
        self.invalidate();
    }

    pub fn delete_word_back(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.checkpoint(EditKind::Other);
        let start = self.cursor;
        self.move_word(false, false);
        let end = start;
        let begin = self.cursor;
        if begin != end {
            self.remove_range(begin, end);
        }
        self.invalidate();
    }

    pub fn kill_to_eol(&mut self) {
        self.checkpoint(EditKind::Other);
        let len = self.line_len(self.cursor.line);
        if self.cursor.col == len {
            self.delete();
        } else {
            let line = self.lines[self.cursor.line].clone();
            let s = byte_of(&line, self.cursor.col);
            self.lines[self.cursor.line] = line[..s].to_string();
        }
        self.invalidate();
    }

    pub fn delete_line(&mut self) -> String {
        self.checkpoint(EditKind::Other);
        let removed = self.lines.remove(self.cursor.line);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.clamp();
        self.anchor = None;
        self.invalidate();
        removed
    }

    pub fn duplicate_line(&mut self) {
        self.checkpoint(EditKind::Other);
        let line = self.lines[self.cursor.line].clone();
        self.lines.insert(self.cursor.line + 1, line);
        self.cursor.line += 1;
        self.invalidate();
    }

    pub fn move_line(&mut self, delta: isize) {
        let target = self.cursor.line as isize + delta;
        if target < 0 || target >= self.lines.len() as isize {
            return;
        }
        self.checkpoint(EditKind::Other);
        let target = target as usize;
        self.lines.swap(self.cursor.line, target);
        self.cursor.line = target;
        self.anchor = None;
        self.invalidate();
    }

    /// Range of lines the current selection (or cursor) touches.
    pub fn active_lines(&self) -> (usize, usize) {
        match self.selection() {
            Some((a, b)) => {
                let end = if b.col == 0 && b.line > a.line { b.line - 1 } else { b.line };
                (a.line, end)
            }
            None => (self.cursor.line, self.cursor.line),
        }
    }

    pub fn indent(&mut self, outdent: bool) {
        self.checkpoint(EditKind::Other);
        let (start, end) = self.active_lines();
        let pad = " ".repeat(self.tab_width);
        for i in start..=end {
            if outdent {
                let line = self.lines[i].clone();
                let strip = line
                    .chars()
                    .take(self.tab_width)
                    .take_while(|c| *c == ' ')
                    .count();
                if strip > 0 {
                    self.lines[i] = line.chars().skip(strip).collect();
                    if i == self.cursor.line {
                        self.cursor.col = self.cursor.col.saturating_sub(strip);
                    }
                    if let Some(a) = self.anchor.as_mut() {
                        if a.line == i {
                            a.col = a.col.saturating_sub(strip);
                        }
                    }
                }
            } else {
                self.lines[i].insert_str(0, &pad);
                if i == self.cursor.line {
                    self.cursor.col += self.tab_width;
                }
                if let Some(a) = self.anchor.as_mut() {
                    if a.line == i {
                        a.col += pad.chars().count();
                    }
                }
            }
        }
        self.invalidate();
    }

    /// Toggle an inline wrapper (`**`, `*`, `~~`, `` ` ``) around the selection
    /// or, with no selection, around the word under the cursor.
    pub fn toggle_wrap(&mut self, marker: &str) {
        self.checkpoint(EditKind::Other);
        let had_selection = self.selection().is_some();
        let (a, b) = match self.selection() {
            Some(sel) => sel,
            None => self.word_range(),
        };
        if a.line != b.line {
            // Multi-line: wrap the whole span.
            let head = self.lines[a.line].clone();
            let at = byte_of(&head, a.col);
            self.lines[a.line] = format!("{}{}{}", &head[..at], marker, &head[at..]);
            let tail = self.lines[b.line].clone();
            let at = byte_of(&tail, b.col + if a.line == b.line { marker.len() } else { 0 });
            self.lines[b.line] = format!("{}{}{}", &tail[..at], marker, &tail[at..]);
            self.anchor = None;
            self.invalidate();
            return;
        }

        let line = chars(self.line(a.line));
        let m: Vec<char> = marker.chars().collect();
        let inner: String = line[a.col..b.col.min(line.len())].iter().collect();

        // Already wrapped inside-out? (`**sel**` selected as `sel`)
        let outside = a.col >= m.len()
            && b.col + m.len() <= line.len()
            && line[a.col - m.len()..a.col] == m[..]
            && line[b.col..b.col + m.len()] == m[..];
        // Selection includes the markers?
        let inside = inner.starts_with(marker) && inner.ends_with(marker) && inner.len() > 2 * m.len();

        let mut wrapped = false;
        let (new_line, new_a, new_b) = if outside {
            let mut s: String = line[..a.col - m.len()].iter().collect();
            s.push_str(&inner);
            s.extend(&line[b.col + m.len()..]);
            (s, a.col - m.len(), b.col - m.len())
        } else if inside {
            let stripped = &inner[m.len()..inner.len() - marker.len()];
            let mut s: String = line[..a.col].iter().collect();
            s.push_str(stripped);
            s.extend(&line[b.col.min(line.len())..]);
            (s, a.col, b.col - 2 * m.len())
        } else {
            let mut s: String = line[..a.col].iter().collect();
            s.push_str(marker);
            s.push_str(&inner);
            s.push_str(marker);
            s.extend(&line[b.col.min(line.len())..]);
            wrapped = true;
            (s, a.col + m.len(), b.col + m.len())
        };
        self.lines[a.line] = new_line;
        // Only keep a selection if the caller had one; wrapping a bare word
        // must leave the cursor free, or the next keystroke would replace it.
        self.anchor = if had_selection && new_a != new_b {
            Some(Pos { line: a.line, col: new_a })
        } else {
            None
        };
        // With no prior selection the caret belongs *after* the closing marker,
        // except for an empty word where it belongs between the two markers.
        let col = if !had_selection && wrapped && new_a != new_b {
            new_b + m.len()
        } else {
            new_b
        };
        self.cursor = Pos { line: a.line, col };
        self.invalidate();
    }


    /// Replace line `i`, shifting the cursor/anchor by the change in length so
    /// prefix transforms don't drop the caret into the middle of a word.
    fn replace_line(&mut self, i: usize, new: String) {
        let old_len = char_len(&self.lines[i]);
        let new_len = char_len(&new);
        self.lines[i] = new;
        let shift = |pos: &mut Pos| {
            if pos.line == i {
                let col = pos.col as isize + (new_len as isize - old_len as isize);
                pos.col = col.clamp(0, new_len as isize) as usize;
            }
        };
        let mut cursor = self.cursor;
        shift(&mut cursor);
        self.cursor = cursor;
        if let Some(mut a) = self.anchor {
            shift(&mut a);
            self.anchor = Some(a);
        }
    }

    /// The run of table lines the cursor is sitting in, if any.
    fn table_range(&self) -> Option<(usize, usize)> {
        let is_row = |l: &String| {
            let t = l.trim();
            !t.is_empty() && t.contains('|')
        };
        if !is_row(&self.lines[self.cursor.line]) {
            return None;
        }
        let mut start = self.cursor.line;
        while start > 0 && is_row(&self.lines[start - 1]) {
            start -= 1;
        }
        let mut end = self.cursor.line;
        while end + 1 < self.lines.len() && is_row(&self.lines[end + 1]) {
            end += 1;
        }
        Some((start, end))
    }

    /// Re-align the pipe table under the cursor. Reports what happened so the
    /// caller can say something useful either way.
    pub fn format_table(&mut self) -> Result<usize, &'static str> {
        let Some((start, end)) = self.table_range() else {
            return Err("cursor is not in a table");
        };
        let block: Vec<&str> = self.lines[start..=end].iter().map(String::as_str).collect();
        let Some(formatted) = table::format(&block) else {
            return Err("not a table — a header row and a |---| row are required");
        };
        if formatted == self.lines[start..=end] {
            return Err("table is already aligned");
        }
        self.checkpoint(EditKind::Other);
        // The row count never changes, so keeping the cursor on its own line
        // and clamping the column is enough to stay put.
        let rows = formatted.len();
        self.lines.splice(start..=end, formatted);
        self.cursor.col = self.cursor.col.min(char_len(self.line(self.cursor.line)));
        self.anchor = None;
        self.invalidate();
        Ok(rows)
    }

    fn word_range(&self) -> (Pos, Pos) {
        let line = chars(self.line(self.cursor.line));
        let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'' || c == '-';
        let mut start = self.cursor.col.min(line.len());
        let mut end = start;
        while start > 0 && is_word(line[start - 1]) {
            start -= 1;
        }
        while end < line.len() && is_word(line[end]) {
            end += 1;
        }
        (
            Pos { line: self.cursor.line, col: start },
            Pos { line: self.cursor.line, col: end },
        )
    }

    /// Set (or with `level == 0`, clear) the ATX heading level of every
    /// selected line.
    pub fn set_heading(&mut self, level: usize) {
        self.checkpoint(EditKind::Other);
        let (start, end) = self.active_lines();
        for i in start..=end {
            let line = self.lines[i].clone();
            let body = line.trim_start_matches('#').trim_start().to_string();
            let new = if level == 0 {
                body
            } else {
                format!("{} {}", "#".repeat(level), body)
            };
            self.replace_line(i, new);
        }
        self.clamp();
        self.invalidate();
    }

    pub fn cycle_heading(&mut self) {
        let line = self.line(self.cursor.line).trim_start();
        let cur = line.chars().take_while(|c| *c == '#').count();
        self.set_heading(if cur >= 6 { 0 } else { cur + 1 });
    }

    /// Toggle `- `, `1. `, `- [ ] ` or `> ` prefixes over the active lines.
    pub fn toggle_prefix(&mut self, kind: PrefixKind) {
        self.checkpoint(EditKind::Other);
        let (start, end) = self.active_lines();
        let all_have = (start..=end).all(|i| kind.matches(&self.lines[i]));
        let mut n = 1u64;
        for i in start..=end {
            let line = self.lines[i].clone();
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let body = line[indent.len()..].to_string();
            let stripped = kind.strip(&body);
            let new = if all_have {
                format!("{indent}{stripped}")
            } else {
                let marker = kind.marker(n);
                n += 1;
                format!("{indent}{marker}{stripped}")
            };
            self.replace_line(i, new);
        }
        self.clamp();
        self.invalidate();
    }

    /// Flip `- [ ]` ⇄ `- [x]` on the active lines.
    pub fn toggle_task(&mut self) {
        self.checkpoint(EditKind::Other);
        let (start, end) = self.active_lines();
        for i in start..=end {
            let line = self.lines[i].clone();
            let lower = line.to_ascii_lowercase();
            if let Some(pos) = lower.find("[ ]") {
                let new = format!("{}[x]{}", &line[..pos], &line[pos + 3..]);
                self.replace_line(i, new);
            } else if let Some(pos) = lower.find("[x]") {
                let new = format!("{}[ ]{}", &line[..pos], &line[pos + 3..]);
                self.replace_line(i, new);
            } else {
                let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                let body = PrefixKind::Bullet.strip(&line[indent.len()..]);
                let new = format!("{indent}- [ ] {body}");
                self.replace_line(i, new);
            }
        }
        self.clamp();
        self.invalidate();
    }

    pub fn insert_link(&mut self, url: &str) {
        self.checkpoint(EditKind::Other);
        let (a, b) = match self.selection() {
            Some(sel) => sel,
            None => self.word_range(),
        };
        if a.line != b.line {
            return;
        }
        let line = chars(self.line(a.line));
        let label: String = line[a.col..b.col.min(line.len())].iter().collect();
        let label = if label.is_empty() { "text".to_string() } else { label };
        let mut s: String = line[..a.col].iter().collect();
        s.push_str(&format!("[{label}]({url})"));
        s.extend(&line[b.col.min(line.len())..]);
        self.lines[a.line] = s;
        self.anchor = None;
        self.cursor = Pos {
            line: a.line,
            col: a.col + label.chars().count() + url.chars().count() + 4,
        };
        self.invalidate();
    }

    pub fn insert_block(&mut self, text: &str) {
        self.checkpoint(EditKind::Other);
        self.anchor = None;
        let at = if self.line(self.cursor.line).trim().is_empty() {
            self.cursor.line
        } else {
            self.cursor.line + 1
        };
        let new: Vec<String> = text.split('\n').map(str::to_string).collect();
        let count = new.len();
        for (offset, line) in new.into_iter().enumerate() {
            self.lines.insert(at + offset, line);
        }
        self.cursor = Pos { line: at, col: 0 };
        let _ = count;
        self.invalidate();
    }

    /// Wrap the selection in a fenced code block (or insert an empty fence).
    pub fn fence_selection(&mut self, lang: &str) {
        self.checkpoint(EditKind::Other);
        let (start, end) = self.active_lines();
        if self.selection().is_none() && self.line(start).trim().is_empty() {
            self.lines[start] = format!("```{lang}");
            self.lines.insert(start + 1, String::new());
            self.lines.insert(start + 2, "```".into());
            self.cursor = Pos { line: start + 1, col: 0 };
        } else {
            self.lines.insert(end + 1, "```".into());
            self.lines.insert(start, format!("```{lang}"));
            self.cursor = Pos { line: start + 1, col: 0 };
        }
        self.anchor = None;
        self.invalidate();
    }

    pub fn find_all(&self, needle: &str, case_sensitive: bool) -> Vec<(Pos, usize)> {
        if needle.is_empty() {
            return Vec::new();
        }
        let needle_cmp = if case_sensitive {
            needle.to_string()
        } else {
            needle.to_lowercase()
        };
        let n = needle.chars().count();
        let mut out = Vec::new();
        for (li, line) in self.lines.iter().enumerate() {
            let hay = if case_sensitive {
                line.clone()
            } else {
                line.to_lowercase()
            };
            let mut from = 0usize;
            while let Some(idx) = hay[from..].find(&needle_cmp) {
                let byte = from + idx;
                let col = hay[..byte].chars().count();
                out.push((Pos { line: li, col }, n));
                from = byte + needle_cmp.len().max(1);
                if from > hay.len() {
                    break;
                }
            }
        }
        out
    }

    pub fn replace_all(&mut self, needle: &str, with: &str, case_sensitive: bool) -> usize {
        if needle.is_empty() {
            return 0;
        }
        self.checkpoint(EditKind::Other);
        let mut count = 0usize;
        for line in self.lines.iter_mut() {
            if case_sensitive {
                count += line.matches(needle).count();
                *line = line.replace(needle, with);
            } else {
                let lower = line.to_lowercase();
                let target = needle.to_lowercase();
                let mut out = String::new();
                let mut from = 0usize;
                while let Some(idx) = lower[from..].find(&target) {
                    let start = from + idx;
                    out.push_str(&line[from..start]);
                    out.push_str(with);
                    from = start + target.len();
                    count += 1;
                }
                out.push_str(&line[from..]);
                *line = out;
            }
        }
        self.clamp();
        self.invalidate();
        count
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrefixKind {
    Bullet,
    Ordered,
    Task,
    Quote,
}

impl PrefixKind {
    fn matches(&self, line: &str) -> bool {
        let t = line.trim_start();
        match self {
            PrefixKind::Bullet => t.starts_with("- ") && !t.starts_with("- ["),
            PrefixKind::Ordered => {
                let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
                d > 0 && t[d..].starts_with(". ")
            }
            PrefixKind::Task => {
                let l = t.to_ascii_lowercase();
                l.starts_with("- [ ] ") || l.starts_with("- [x] ")
            }
            PrefixKind::Quote => t.starts_with('>'),
        }
    }

    fn marker(&self, n: u64) -> String {
        match self {
            PrefixKind::Bullet => "- ".into(),
            PrefixKind::Ordered => format!("{n}. "),
            PrefixKind::Task => "- [ ] ".into(),
            PrefixKind::Quote => "> ".into(),
        }
    }

    /// Remove any list/quote marker so prefixes swap cleanly instead of stacking.
    fn strip(&self, body: &str) -> String {
        let mut s = body;
        loop {
            let before = s;
            if let Some(rest) = s.strip_prefix("> ").or_else(|| s.strip_prefix('>')) {
                if matches!(self, PrefixKind::Quote) {
                    s = rest;
                }
            }
            let lower = s.to_ascii_lowercase();
            if lower.starts_with("- [ ] ") || lower.starts_with("- [x] ") {
                s = &s[6..];
            } else if s.starts_with("- ") || s.starts_with("* ") || s.starts_with("+ ") {
                s = &s[2..];
            } else {
                let d = s.chars().take_while(|c| c.is_ascii_digit()).count();
                if d > 0 && (s[d..].starts_with(". ") || s[d..].starts_with(") ")) {
                    s = &s[d + 2..];
                }
            }
            if s == before {
                break;
            }
        }
        s.to_string()
    }
}

struct Continuation {
    /// The prefix present on the current line.
    matched: String,
    /// The prefix to place on the new line.
    next: String,
}

/// Work out what a new line should inherit from `line`.
fn continuation_prefix(line: &str) -> Option<Continuation> {
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let rest = &line[indent.len()..];

    // Blockquote (possibly repeated)
    let mut quote = String::new();
    let mut r = rest;
    while r.starts_with('>') {
        let take = if r[1..].starts_with(' ') { 2 } else { 1 };
        quote.push_str(&r[..take]);
        r = &r[take..];
    }
    let inner = continuation_prefix_list(r);
    match (quote.is_empty(), inner) {
        (true, None) => None,
        (true, Some((m, n))) => Some(Continuation {
            matched: format!("{indent}{m}"),
            next: format!("{indent}{n}"),
        }),
        (false, None) => Some(Continuation {
            matched: format!("{indent}{quote}"),
            next: format!("{indent}{quote}"),
        }),
        (false, Some((m, n))) => Some(Continuation {
            matched: format!("{indent}{quote}{m}"),
            next: format!("{indent}{quote}{n}"),
        }),
    }
}

fn continuation_prefix_list(rest: &str) -> Option<(String, String)> {
    let lower = rest.to_ascii_lowercase();
    if lower.starts_with("- [ ] ") || lower.starts_with("- [x] ") {
        return Some((rest[..6].to_string(), "- [ ] ".to_string()));
    }
    if lower.starts_with("* [ ] ") || lower.starts_with("* [x] ") {
        return Some((rest[..6].to_string(), "* [ ] ".to_string()));
    }
    let b = rest.as_bytes();
    if !b.is_empty() && matches!(b[0], b'-' | b'*' | b'+') && b.get(1) == Some(&b' ') {
        return Some((rest[..2].to_string(), rest[..2].to_string()));
    }
    let d = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if d > 0 && d <= 9 {
        let sep = rest.as_bytes().get(d).copied();
        if matches!(sep, Some(b'.') | Some(b')')) && rest.as_bytes().get(d + 1) == Some(&b' ') {
            let n: u64 = rest[..d].parse().unwrap_or(1);
            let sep = rest.as_bytes()[d] as char;
            return Some((rest[..d + 2].to_string(), format!("{}{} ", n + 1, sep)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(text: &str) -> Editor {
        Editor::from_str(text)
    }

    fn at(text: &str, line: usize, col: usize) -> Editor {
        let mut e = ed(text);
        e.cursor = Pos { line, col };
        e
    }

    #[test]
    fn round_trips_text() {
        for src in ["", "one", "one\ntwo\n", "trailing\n\n"] {
            assert_eq!(ed(src).text(), src);
        }
    }

    #[test]
    fn bold_wraps_the_word_under_the_cursor() {
        let mut e = at("make this bold now", 0, 11);
        e.toggle_wrap("**");
        assert_eq!(e.text(), "make this **bold** now");
        // The caret sits after the closing marker, not inside it.
        assert_eq!(e.cursor.col, 18);
        assert!(e.selection().is_none());
    }

    #[test]
    fn bold_toggles_back_off() {
        let mut e = at("make this **bold** now", 0, 13);
        e.toggle_wrap("**");
        assert_eq!(e.text(), "make this bold now");
    }

    #[test]
    fn wrapping_an_empty_spot_parks_the_caret_between_markers() {
        let mut e = at("a  b", 0, 2);
        e.toggle_wrap("`");
        assert_eq!(e.text(), "a `` b");
        assert_eq!(e.cursor.col, 3);
    }

    #[test]
    fn wrapping_a_selection_keeps_it_selected() {
        let mut e = ed("alpha beta");
        e.anchor = Some(Pos { line: 0, col: 0 });
        e.cursor = Pos { line: 0, col: 5 };
        e.toggle_wrap("*");
        assert_eq!(e.text(), "*alpha* beta");
        assert_eq!(e.selected_text().as_deref(), Some("alpha"));
    }

    #[test]
    fn headings_are_set_replaced_and_cleared() {
        let mut e = ed("Title");
        e.set_heading(2);
        assert_eq!(e.text(), "## Title");
        e.set_heading(4);
        assert_eq!(e.text(), "#### Title");
        e.set_heading(0);
        assert_eq!(e.text(), "Title");
    }

    #[test]
    fn heading_changes_carry_the_cursor() {
        let mut e = at("Title", 0, 5);
        e.set_heading(1);
        assert_eq!(e.cursor.col, 7, "caret stays at the end of the text");
    }

    #[test]
    fn heading_level_cycles_and_wraps_to_none() {
        let mut e = ed("###### deep");
        e.cycle_heading();
        assert_eq!(e.text(), "deep");
    }

    #[test]
    fn list_prefixes_swap_rather_than_stack() {
        let mut e = ed("item");
        e.toggle_prefix(PrefixKind::Bullet);
        assert_eq!(e.text(), "- item");
        e.toggle_prefix(PrefixKind::Ordered);
        assert_eq!(e.text(), "1. item");
        e.toggle_prefix(PrefixKind::Task);
        assert_eq!(e.text(), "- [ ] item");
        e.toggle_prefix(PrefixKind::Task);
        assert_eq!(e.text(), "item");
    }

    #[test]
    fn prefixes_apply_across_a_selection_and_number_in_order() {
        let mut e = ed("one\ntwo\nthree");
        e.anchor = Some(Pos { line: 0, col: 0 });
        e.cursor = Pos { line: 2, col: 5 };
        e.toggle_prefix(PrefixKind::Ordered);
        assert_eq!(e.text(), "1. one\n2. two\n3. three");
    }

    #[test]
    fn tasks_toggle_between_states() {
        let mut e = ed("- write tests");
        e.toggle_task();
        assert_eq!(e.text(), "- [ ] write tests");
        e.toggle_task();
        assert_eq!(e.text(), "- [x] write tests");
        e.toggle_task();
        assert_eq!(e.text(), "- [ ] write tests");
    }

    #[test]
    fn quotes_indent_and_unindent() {
        let mut e = ed("said");
        e.toggle_prefix(PrefixKind::Quote);
        assert_eq!(e.text(), "> said");
        e.toggle_prefix(PrefixKind::Quote);
        assert_eq!(e.text(), "said");
    }

    #[test]
    fn enter_continues_a_bullet_list() {
        let mut e = at("- first", 0, 7);
        e.newline(true);
        assert_eq!(e.text(), "- first\n- ");
        assert_eq!(e.cursor.col, 2);
    }

    #[test]
    fn enter_increments_ordered_lists() {
        let mut e = at("3. third", 0, 8);
        e.newline(true);
        assert_eq!(e.text(), "3. third\n4. ");
    }

    #[test]
    fn enter_resets_a_checked_task_and_keeps_indent() {
        let mut e = at("  - [x] done", 0, 12);
        e.newline(true);
        assert_eq!(e.text(), "  - [x] done\n  - [ ] ");
    }

    #[test]
    fn enter_on_an_empty_item_ends_the_list() {
        let mut e = at("- first", 0, 7);
        e.newline(true);
        e.newline(true);
        assert_eq!(e.text(), "- first\n\n");
    }

    #[test]
    fn enter_continues_block_quotes() {
        let mut e = at("> quoted", 0, 8);
        e.newline(true);
        assert_eq!(e.text(), "> quoted\n> ");
    }

    #[test]
    fn undo_and_redo_walk_the_history() {
        let mut e = ed("start");
        e.move_doc_end(false);
        e.insert_str(" more");
        assert_eq!(e.text(), "start more");
        assert!(e.undo());
        assert_eq!(e.text(), "start");
        assert!(e.redo());
        assert_eq!(e.text(), "start more");
        assert!(!e.redo());
    }

    #[test]
    fn undo_restores_the_cursor_position() {
        let mut e = at("abc", 0, 3);
        e.break_undo();
        e.insert_char('d');
        e.undo();
        assert_eq!(e.cursor, Pos { line: 0, col: 3 });
    }

    #[test]
    fn indent_and_outdent_move_whole_selections() {
        let mut e = ed("a\nb");
        e.anchor = Some(Pos { line: 0, col: 0 });
        e.cursor = Pos { line: 1, col: 1 };
        e.indent(false);
        assert_eq!(e.text(), "  a\n  b");
        e.indent(true);
        assert_eq!(e.text(), "a\nb");
    }

    #[test]
    fn backspace_removes_a_whole_indent_step() {
        let mut e = at("    text", 0, 4);
        e.backspace();
        assert_eq!(e.text(), "  text");
        assert_eq!(e.cursor.col, 2);
    }

    #[test]
    fn backspace_joins_lines_at_the_margin() {
        let mut e = at("one\ntwo", 1, 0);
        e.backspace();
        assert_eq!(e.text(), "onetwo");
        assert_eq!(e.cursor, Pos { line: 0, col: 3 });
    }

    #[test]
    fn word_motion_crosses_punctuation_and_lines() {
        let mut e = at("alpha beta\ngamma", 0, 0);
        e.move_word(true, false);
        assert_eq!(e.cursor.col, 6);
        e.move_word(true, false);
        assert_eq!(e.cursor.col, 10);
        e.move_word(true, false);
        assert_eq!(e.cursor, Pos { line: 1, col: 0 });
        e.move_word(false, false);
        assert_eq!(e.cursor, Pos { line: 0, col: 10 });
    }

    #[test]
    fn home_alternates_between_indent_and_column_zero() {
        let mut e = at("    indented", 0, 8);
        e.move_home(false);
        assert_eq!(e.cursor.col, 4);
        e.move_home(false);
        assert_eq!(e.cursor.col, 0);
    }

    #[test]
    fn selections_span_multiple_lines() {
        let mut e = ed("one\ntwo\nthree");
        e.anchor = Some(Pos { line: 0, col: 1 });
        e.cursor = Pos { line: 2, col: 2 };
        assert_eq!(e.selected_text().as_deref(), Some("ne\ntwo\nth"));
        e.delete_selection();
        assert_eq!(e.text(), "oree");
    }

    #[test]
    fn find_reports_every_occurrence() {
        let e = ed("aXa\nbXb\nXX");
        assert_eq!(e.find_all("X", true).len(), 4);
        assert_eq!(e.find_all("x", true).len(), 0);
        assert_eq!(e.find_all("x", false).len(), 4);
    }

    #[test]
    fn replace_all_is_case_aware() {
        let mut e = ed("Cat cat CAT");
        assert_eq!(e.replace_all("cat", "dog", true), 1);
        assert_eq!(e.text(), "Cat dog CAT");
        let mut e = ed("Cat cat CAT");
        assert_eq!(e.replace_all("cat", "dog", false), 3);
        assert_eq!(e.text(), "dog dog dog");
    }

    #[test]
    fn links_wrap_the_selected_label() {
        let mut e = ed("docs");
        e.anchor = Some(Pos { line: 0, col: 0 });
        e.cursor = Pos { line: 0, col: 4 };
        e.insert_link("https://x.dev");
        assert_eq!(e.text(), "[docs](https://x.dev)");
    }

    #[test]
    fn fencing_a_selection_brackets_the_lines() {
        let mut e = ed("let x = 1;\nlet y = 2;");
        e.anchor = Some(Pos { line: 0, col: 0 });
        e.cursor = Pos { line: 1, col: 10 };
        e.fence_selection("rust");
        assert_eq!(e.text(), "```rust\nlet x = 1;\nlet y = 2;\n```");
    }

    #[test]
    fn line_operations_move_and_duplicate() {
        let mut e = at("one\ntwo", 0, 0);
        e.duplicate_line();
        assert_eq!(e.text(), "one\none\ntwo");
        e.move_line(1);
        assert_eq!(e.text(), "one\ntwo\none");
        assert_eq!(e.delete_line(), "one");
        assert_eq!(e.text(), "one\ntwo");
    }

    #[test]
    fn outline_reflects_the_buffer() {
        let mut e = ed("# One\n\n## Two\n\ntext");
        let out = e.outline();
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], (2, "Two".to_string(), 2));
    }

    #[test]
    fn stats_count_lines_words_and_characters() {
        let e = ed("one two\nthree");
        assert_eq!(e.stats(), (2, 3, 12));
    }

    #[test]
    fn unicode_columns_are_character_based() {
        let mut e = at("héllo wörld", 0, 0);
        e.move_word(true, false);
        assert_eq!(e.cursor.col, 6);
        e.insert_char('ß');
        assert_eq!(e.text(), "héllo ßwörld");
    }
}
