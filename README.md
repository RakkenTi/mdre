# mdui

A terminal markdown manager. Browse a folder of markdown, **read** it rendered as
GitHub-Flavored Markdown, and **edit** it with colour-coded source and a set of
formatting tools — all in one TUI.

```
mdui path/to/notes             # browse a folder
mdui README.md                 # open a file in the reader
mdui -e README.md              # open it in the editor
mdui -w README.md              # editor beside a live preview
mdui -r README.md | less -R    # render to stdout, no TUI
```

## Install

**Linux and macOS**

```sh
curl -fsSL https://raw.githubusercontent.com/RakkenTi/mdui/main/install.sh | sh
```

Installs to `~/.local/bin` — no `sudo`, ever. The script verifies the download
against the release's `SHA256SUMS` before unpacking, and tells you if
`~/.local/bin` is not on your `PATH`.

Prefer to read it before running it? That is the right instinct:

```sh
curl -fsSL https://raw.githubusercontent.com/RakkenTi/mdui/main/install.sh -o install.sh
less install.sh
sh install.sh
```

Set `MDUI_VERSION=v0.1.0` to pin a version, or `MDUI_INSTALL_DIR=~/bin` to
install elsewhere.

**Windows**

Download `mdui-<version>-x86_64-pc-windows-msvc.zip` from the
[releases page](https://github.com/RakkenTi/mdui/releases), then:

1. Extract it to `%LOCALAPPDATA%\Programs\mdui`
2. Add that folder to your `PATH` — in PowerShell, once:

   ```powershell
   $dir = "$env:LOCALAPPDATA\Programs\mdui"
   [Environment]::SetEnvironmentVariable(
       "Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";$dir", "User")
   ```

3. Open a new terminal and run `mdui --version`

Use [Windows Terminal](https://aka.ms/terminal) — the legacy console host does
not handle 24-bit colour. There is no PowerShell install script yet.

**With Cargo**

```sh
cargo install mdui           # builds from source; needs a Rust toolchain
cargo binstall mdui          # downloads the prebuilt binary instead
```

On Windows `cargo install` also needs the Visual Studio Build Tools for the
MSVC linker; `cargo binstall` does not.

**Prebuilt targets**

`x86_64` and `aarch64` for Linux (static musl — no glibc requirement, works on
Alpine) and macOS, plus `x86_64` Windows.

## If you only want to read

Use [glow](https://github.com/charmbracelet/glow). It renders markdown in the
terminal beautifully, it is mature, it has a stash and it is one `brew install`
away. mdui's reader owes it the idea.

mdui earns its place when reading is not the whole job — when you want to edit
what you are reading, jump between linked notes, search a folder rather than a
file, or see the source and the render side by side. If none of that is what you
came for, glow is the better tool and you should reach for it.

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

## Searching the whole folder

`/` searches the open document. `f` searches **every** markdown file under the
root and lists the matching lines; `Enter` opens one at that line.

`b` answers the other question — *what links here?* It walks the tree resolving
every link the way following one would, so `./notes.md`, `notes` and
`../guide/notes.md#section` are all recognised as pointing at the file you are
reading.

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
-r, --render     render PATH to stdout as ANSI and exit
    --width N    column width for --render
```

## Configuration

`~/.config/mdui/config.toml` (or `$XDG_CONFIG_HOME/mdui/config.toml`) sets the
defaults; flags on the command line win over it. Copy
[`config.example.toml`](config.example.toml) to start.

```toml
[options]
theme           = "light"
width           = 88
heading_markers = false
markdown_only   = true

[colors]
accent   = "#FF6600"   # any palette entry, as #RRGGBB or a 0-255 index
heading1 = "#00C2A8"
```

A typo is reported in the status bar on startup — `config.toml — line 2:
unknown option "with"` — rather than silently doing nothing.

## Build from source

```
git clone https://github.com/RakkenTi/mdui && cd mdui
cargo run --release -- README.md    # run without installing
cargo build --release
cargo test          # 114 tests over the renderer, editor, links, search and config
```

Dependencies: `ratatui`, `crossterm`, `pulldown-cmark`, `unicode-width`, `anyhow`.
Syntax highlighting is built in, so there is no grammar bundle to ship.
