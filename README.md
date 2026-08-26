# mdre

mdre is a terminal markdown (GFM) manager. It supports reading, editing, and
browsing markdown files from a single TUI.

## Install

**Linux / macOS**

```sh
curl -fsSL https://raw.githubusercontent.com/RakkenTi/mdre/main/install.sh | sh
```

**Windows**

Download the zip from the [releases page](https://github.com/RakkenTi/mdre/releases),
extract to `%LOCALAPPDATA%\Programs\mdre`, and add it to your `PATH`:

```powershell
$dir = "$env:LOCALAPPDATA\Programs\mdre"
[Environment]::SetEnvironmentVariable(
    "Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";$dir", "User")
```

**Cargo**

```sh
cargo install mdre           # builds from source
cargo binstall mdre          # downloads the prebuilt binary instead
```

## Usage

```sh
cd path/to/notes
mdre                           # browse a folder

# Other ways to use
mdre README.md                 # open a file directly in the reader
mdre -e README.md              # open it in the editor
mdre -w README.md              # editor beside a live preview
mdre -r README.md | less -R    # render to stdout, no TUI
```

Inside the TUI, `F1` shows the complete key reference and `Ctrl+P` opens a
searchable command palette. Links between notes resolve like a wiki
(`./spec.md`, `spec`, `guide.md#section`), and `b` lists every file that links
to the current one. For read-only rendering,
[glow](https://github.com/charmbracelet/glow) is excellent.

## Configuration

`~/.config/mdre/config.toml` sets defaults (theme, width, colours); command-line
flags win over it. See [`config.example.toml`](config.example.toml).

## Build from source

```
git clone https://github.com/RakkenTi/mdre && cd mdre
cargo run --release -- README.md
cargo test
```

Dependencies: `ratatui`, `crossterm`, `pulldown-cmark`, `unicode-width`, `anyhow`,
[`tohki`](https://crates.io/crates/tohki) (syntax highlighting).
