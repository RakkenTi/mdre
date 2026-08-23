# mdui

A terminal markdown manager. Browse a folder of markdown, **read** it rendered as
GitHub-Flavored Markdown, and **edit** it with colour-coded source and a set of
formatting tools — all in one TUI.

```
cargo run --release -- path/to/notes        # browse a folder
cargo run --release -- README.md            # open a file in the reader
cargo run --release -- -e README.md         # open it in the editor
cargo run --release -- -w README.md         # editor beside a live preview
```

## Three modes, one buffer

| Mode | What it is |
| ---- | ---------- |
| **Files** | Fuzzy-filterable browser with titles, sizes and timestamps; create, rename and delete without leaving the app |
| **Read** | The document rendered: headings, tables, alerts, task lists, footnotes, highlighted code |
| **Edit** | The document's source, colour-coded, with markdown-aware editing tools |

`Ctrl+E` flips between reading and editing, `Ctrl+W` shows both at once, and the
preview always reflects the buffer — including unsaved changes.

## Reader

Rendered with `pulldown-cmark` in full GFM mode:

- **Headings** with level colours and a rule under `#`/`##`
- **Tables** drawn in box characters, honouring `:---`/`---:` alignment and
  shrinking columns to fit the viewport
- **GFM alerts** — `> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`
- **Task lists** as `☑`/`☐`, nested lists with per-depth bullets
- **Fenced code** in a labelled box with syntax highlighting for ~40 languages
- **Footnotes** renumbered and rendered inline, **strikethrough**, autolinks,
  images, inline HTML, YAML/TOML front matter, definition lists, math
- Block quotes keep their bar across wrapped lines and arbitrary nesting

Plus an outline panel that tracks your position, a heading jump list,
in-document search, and an adjustable text column.

### Following links

`L` lists every link in the document; `Enter` goes where it points and
`Backspace` retraces your steps, so a directory of notes reads like a wiki.

- `./spec.md`, `../api.md`, `spec` (the `.md` is optional) and `sub/` (its
  `README.md`) all resolve relative to the open file
- `#section` scrolls to that heading; `guide.md#section` opens the file *and*
  lands on the heading
- `http(s)` and `mailto:` links are handed to the desktop opener
- Targets that do not exist are marked `✗` in the list, so a stale cross-
  reference is visible before you follow it

## Editor

Colour-coded markdown source — the buffer is shown exactly as it is on disk,
only tinted. Fenced blocks are highlighted in their own language.

Formatting tools work on the selection, or on the word under the cursor:

| Key | Tool |
| --- | ---- |
| `Ctrl+B` / `Alt+I` | bold / italic (toggles off again) |
| `Alt+S` / `Alt+E` | strikethrough / inline code |
| `Ctrl+K` | wrap in a link |
| `Alt+C` | wrap in a code fence |
| `Alt+1`…`Alt+6`, `Alt+0`, `Alt+H` | set / clear / cycle heading level |
| `Alt+L` `Alt+O` `Alt+T` | bullet, numbered, task list (prefixes swap, never stack) |
| `Alt+X` | toggle a task done |
| `Alt+Q` | block quote |
| `Alt+B` / `Alt+-` | insert a table / horizontal rule |
| `Alt+A` | re-align the pipes of the table under the cursor |
| `Tab` / `Shift+Tab` | indent / outdent the selection |

Editing conveniences: soft wrap, undo/redo with keystroke coalescing, multi-line
selection, word motion, line duplicate/move/delete, find and replace, go-to-line,
and a smart `Enter` that continues lists and quotes — incrementing ordered
numbers, resetting checkboxes, and ending the list when you press it twice.

Press `Ctrl+P` for a searchable palette of every command, or `F1` for the full
key reference.

## Clipboard

`Ctrl+C` / `Ctrl+X` and `y` (copy the whole document) reach the **system**
clipboard via OSC 52 — the terminal does the copying, so it works over SSH,
inside tmux and inside screen with no X11 or Wayland dependency. The internal
buffer stays as a fallback when the terminal refuses.

## Options

```
-e, --edit       open PATH straight in the editor
-l, --light      start with the light theme      (F9 toggles at runtime)
-w, --split      start in split view
    --no-sidebar hide the file sidebar           (F2 toggles at runtime)
```

## Build

```
cargo build --release
cargo test          # 97 tests over the renderer, editor, tables and links
```

Dependencies: `ratatui`, `crossterm`, `pulldown-cmark`, `unicode-width`, `anyhow`.
Syntax highlighting is built in, so there is no grammar bundle to ship.
