//! A small, dependency-free syntax highlighter used for fenced code blocks.
//!
//! It is deliberately generic: one tokenizer driven by a per-language table of
//! comment markers, string delimiters and keyword sets. That covers the
//! languages that actually show up inside markdown documents without dragging a
//! multi-megabyte grammar bundle into the binary.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Text,
    Keyword,
    Type,
    Const,
    Str,
    Number,
    Comment,
    Func,
    Punct,
    Attr,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HlState {
    /// Inside a `/* ... */`-style block comment.
    pub block_comment: bool,
    /// Inside a multi-line string; index into `Lang::multiline`.
    pub multiline: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Generic,
    Diff,
    Keyed,
}

pub struct Lang {
    names: &'static [&'static str],
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// Paired multi-line string delimiters, checked before single-char quotes.
    multiline: &'static [(&'static str, &'static str)],
    quotes: &'static [char],
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    consts: &'static [&'static str],
    /// Sigils that start an annotation/attribute token (`@Override`, `#[derive]`).
    attr_prefix: &'static [char],
    /// Treat `CamelCase` identifiers as type names.
    upper_is_type: bool,
    /// `'x'` is a character literal, so a bare `'` is a lifetime, not a string.
    char_quotes: bool,
    flavor: Flavor,
}

const NO_ML: &[(&str, &str)] = &[];
const DQ: &[char] = &['"'];
const DQ_SQ: &[char] = &['"', '\''];
const DQ_SQ_BT: &[char] = &['"', '\'', '`'];

const LANGS: &[Lang] = &[
    Lang {
        names: &["rust", "rs"],
        line_comment: &["//"],
        block_comment: Some(("/*", "*/")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
            "trait", "true", "type", "unsafe", "use", "where", "while", "union",
        ],
        types: &[
            "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16",
            "u32", "u64", "u128", "usize", "str", "String", "Vec", "Option", "Result", "Box",
            "HashMap", "HashSet", "Rc", "Arc", "RefCell",
        ],
        consts: &["None", "Some", "Ok", "Err"],
        attr_prefix: &['#'],
        upper_is_type: true,
        char_quotes: true,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["python", "py"],
        line_comment: &["#"],
        block_comment: None,
        multiline: &[("\"\"\"", "\"\"\""), ("'''", "'''")],
        quotes: DQ_SQ,
        keywords: &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
            "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
            "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
            "with", "yield", "match", "case",
        ],
        types: &[
            "int", "float", "str", "bool", "bytes", "list", "dict", "set", "tuple", "object",
            "frozenset", "complex",
        ],
        consts: &["True", "False", "None", "self", "cls", "__name__"],
        attr_prefix: &['@'],
        upper_is_type: true,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["javascript", "js", "jsx", "mjs", "cjs", "typescript", "ts", "tsx"],
        line_comment: &["//"],
        block_comment: Some(("/*", "*/")),
        multiline: NO_ML,
        quotes: DQ_SQ_BT,
        keywords: &[
            "as", "async", "await", "break", "case", "catch", "class", "const", "continue",
            "debugger", "default", "delete", "do", "else", "enum", "export", "extends", "finally",
            "for", "from", "function", "get", "if", "implements", "import", "in", "instanceof",
            "interface", "let", "new", "of", "private", "protected", "public", "readonly",
            "return", "satisfies", "set", "static", "super", "switch", "this", "throw", "try",
            "type", "typeof", "var", "void", "while", "yield", "declare", "namespace", "keyof",
            "infer", "abstract",
        ],
        types: &[
            "any", "bigint", "boolean", "never", "number", "object", "string", "symbol",
            "unknown", "Array", "Promise", "Record", "Partial", "Map", "Set", "Date", "RegExp",
        ],
        consts: &["true", "false", "null", "undefined", "NaN", "Infinity"],
        attr_prefix: &['@'],
        upper_is_type: true,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["go", "golang"],
        line_comment: &["//"],
        block_comment: Some(("/*", "*/")),
        multiline: &[("`", "`")],
        quotes: DQ_SQ,
        keywords: &[
            "break", "case", "chan", "const", "continue", "default", "defer", "else",
            "fallthrough", "for", "func", "go", "goto", "if", "import", "interface", "map",
            "package", "range", "return", "select", "struct", "switch", "type", "var",
        ],
        types: &[
            "bool", "byte", "complex64", "complex128", "error", "float32", "float64", "int",
            "int8", "int16", "int32", "int64", "rune", "string", "uint", "uint8", "uint16",
            "uint32", "uint64", "uintptr", "any",
        ],
        consts: &["true", "false", "nil", "iota"],
        attr_prefix: &[],
        upper_is_type: true,
        char_quotes: true,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["c", "h", "cpp", "c++", "cc", "hpp", "objc"],
        line_comment: &["//"],
        block_comment: Some(("/*", "*/")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "auto", "break", "case", "catch", "class", "const", "constexpr", "continue",
            "default", "delete", "do", "else", "enum", "explicit", "extern", "for", "friend",
            "goto", "if", "inline", "namespace", "new", "noexcept", "operator", "override",
            "private", "protected", "public", "register", "return", "sizeof", "static",
            "struct", "switch", "template", "this", "throw", "try", "typedef", "typename",
            "union", "using", "virtual", "volatile", "while", "include", "define", "ifdef",
            "ifndef", "endif", "pragma",
        ],
        types: &[
            "bool", "char", "double", "float", "int", "long", "short", "signed", "unsigned",
            "void", "size_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t",
            "int32_t", "int64_t", "string", "vector", "map",
        ],
        consts: &["true", "false", "NULL", "nullptr"],
        attr_prefix: &['#'],
        upper_is_type: true,
        char_quotes: true,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["java", "kotlin", "kt", "scala", "groovy", "csharp", "cs", "c#", "swift", "dart"],
        line_comment: &["//"],
        block_comment: Some(("/*", "*/")),
        multiline: &[("\"\"\"", "\"\"\"")],
        quotes: DQ_SQ,
        keywords: &[
            "abstract", "as", "async", "await", "base", "break", "case", "catch", "class",
            "companion", "const", "continue", "data", "default", "do", "else", "enum", "extends",
            "extension", "final", "finally", "for", "fun", "func", "get", "guard", "if",
            "implements", "import", "in", "init", "instanceof", "interface", "internal", "is",
            "lateinit", "let", "namespace", "new", "object", "open", "operator", "out",
            "override", "package", "private", "protected", "public", "readonly", "return",
            "sealed", "set", "static", "struct", "super", "suspend", "switch", "synchronized",
            "this", "throw", "throws", "trait", "try", "typealias", "using", "val", "var",
            "when", "where", "while", "yield",
        ],
        types: &[
            "Boolean", "Byte", "Char", "Double", "Float", "Int", "Integer", "Long", "Short",
            "String", "Unit", "Void", "bool", "byte", "char", "decimal", "double", "float",
            "int", "long", "object", "short", "string", "var", "void", "List", "Map", "Set",
        ],
        consts: &["true", "false", "null", "nil", "None"],
        attr_prefix: &['@'],
        upper_is_type: true,
        char_quotes: true,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["ruby", "rb"],
        line_comment: &["#"],
        block_comment: Some(("=begin", "=end")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "alias", "begin", "break", "case", "class", "def", "defined?", "do", "else", "elsif",
            "end", "ensure", "for", "if", "in", "module", "next", "not", "or", "and", "redo",
            "rescue", "retry", "return", "self", "super", "then", "undef", "unless", "until",
            "when", "while", "yield", "require", "require_relative", "attr_accessor",
        ],
        types: &["Array", "Hash", "String", "Symbol", "Integer", "Float", "Struct"],
        consts: &["true", "false", "nil", "__FILE__"],
        attr_prefix: &['@', '$'],
        upper_is_type: true,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["php"],
        line_comment: &["//", "#"],
        block_comment: Some(("/*", "*/")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "abstract", "and", "array", "as", "break", "callable", "case", "catch", "class",
            "clone", "const", "continue", "declare", "default", "do", "echo", "else", "elseif",
            "empty", "enddeclare", "endfor", "endforeach", "endif", "endswitch", "endwhile",
            "extends", "final", "finally", "fn", "for", "foreach", "function", "global", "if",
            "implements", "include", "instanceof", "interface", "isset", "list", "match",
            "namespace", "new", "or", "print", "private", "protected", "public", "readonly",
            "require", "return", "static", "switch", "throw", "trait", "try", "unset", "use",
            "var", "while", "yield",
        ],
        types: &["bool", "float", "int", "iterable", "mixed", "object", "string", "void"],
        consts: &["true", "false", "null", "TRUE", "FALSE", "NULL", "this"],
        attr_prefix: &['$', '#'],
        upper_is_type: true,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["bash", "sh", "shell", "zsh", "console", "shell-session", "fish"],
        line_comment: &["#"],
        block_comment: None,
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case",
            "esac", "function", "in", "select", "return", "break", "continue", "local",
            "export", "readonly", "declare", "source", "alias", "unset", "trap", "shift", "set",
        ],
        types: &[
            "echo", "cd", "ls", "cat", "grep", "sed", "awk", "curl", "git", "cargo", "npm",
            "make", "sudo", "chmod", "mkdir", "rm", "cp", "mv", "find", "xargs", "docker",
        ],
        consts: &["true", "false"],
        attr_prefix: &['$'],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["json", "jsonc", "json5"],
        line_comment: &["//"],
        block_comment: Some(("/*", "*/")),
        multiline: NO_ML,
        quotes: DQ,
        keywords: &[],
        types: &[],
        consts: &["true", "false", "null"],
        attr_prefix: &[],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Keyed,
    },
    Lang {
        names: &["yaml", "yml"],
        line_comment: &["#"],
        block_comment: None,
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[],
        types: &[],
        consts: &["true", "false", "null", "yes", "no", "on", "off", "~"],
        attr_prefix: &[],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Keyed,
    },
    Lang {
        names: &["toml", "ini", "cfg", "conf", "properties"],
        line_comment: &["#", ";"],
        block_comment: None,
        multiline: &[("\"\"\"", "\"\"\""), ("'''", "'''")],
        quotes: DQ_SQ,
        keywords: &[],
        types: &[],
        consts: &["true", "false"],
        attr_prefix: &[],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Keyed,
    },
    Lang {
        names: &["html", "xml", "svg", "vue", "svelte", "xhtml"],
        line_comment: &[],
        block_comment: Some(("<!--", "-->")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "html", "head", "body", "div", "span", "a", "p", "ul", "ol", "li", "table", "tr",
            "td", "th", "script", "style", "link", "meta", "img", "input", "button", "form",
            "section", "header", "footer", "nav", "main", "template", "h1", "h2", "h3", "code",
            "pre", "svg", "path", "g",
        ],
        types: &[],
        consts: &[],
        attr_prefix: &[],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["css", "scss", "sass", "less"],
        line_comment: &["//"],
        block_comment: Some(("/*", "*/")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "import", "media", "supports", "keyframes", "mixin", "include", "extend", "use",
            "from", "to", "and", "not", "only",
        ],
        types: &[
            "color", "background", "display", "position", "margin", "padding", "border",
            "width", "height", "flex", "grid", "font", "line-height", "opacity", "transform",
            "transition", "z-index", "content", "gap", "overflow",
        ],
        consts: &["none", "auto", "inherit", "initial", "unset", "absolute", "relative", "block"],
        attr_prefix: &['@', '$', '-'],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["sql", "postgres", "postgresql", "mysql", "sqlite"],
        line_comment: &["--"],
        block_comment: Some(("/*", "*/")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "select", "from", "where", "insert", "into", "values", "update", "set", "delete",
            "create", "table", "drop", "alter", "add", "index", "join", "left", "right",
            "inner", "outer", "on", "group", "order", "by", "having", "limit", "offset",
            "union", "all", "distinct", "as", "and", "or", "not", "in", "like", "between",
            "case", "when", "then", "else", "end", "with", "returning", "primary", "key",
            "foreign", "references", "constraint", "default", "cascade", "begin", "commit",
        ],
        types: &[
            "int", "integer", "bigint", "smallint", "serial", "text", "varchar", "char",
            "boolean", "date", "timestamp", "timestamptz", "numeric", "decimal", "real",
            "json", "jsonb", "uuid", "bytea",
        ],
        consts: &["true", "false", "null"],
        attr_prefix: &[],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["lua"],
        line_comment: &["--"],
        block_comment: Some(("--[[", "]]")),
        multiline: &[("[[", "]]")],
        quotes: DQ_SQ,
        keywords: &[
            "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if",
            "in", "local", "not", "or", "repeat", "return", "then", "until", "while",
        ],
        types: &["string", "table", "number", "math", "io", "os"],
        consts: &["true", "false", "nil", "self"],
        attr_prefix: &[],
        upper_is_type: true,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["haskell", "hs", "elm"],
        line_comment: &["--"],
        block_comment: Some(("{-", "-}")),
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "case", "class", "data", "deriving", "do", "else", "if", "import", "in", "infix",
            "instance", "let", "module", "newtype", "of", "then", "type", "where", "port",
            "exposing", "as",
        ],
        types: &["Int", "Integer", "Float", "Double", "Char", "String", "Bool", "Maybe", "IO"],
        consts: &["True", "False", "Nothing", "Just"],
        attr_prefix: &[],
        upper_is_type: true,
        char_quotes: true,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["elixir", "ex", "exs", "erlang"],
        line_comment: &["#"],
        block_comment: None,
        multiline: &[("\"\"\"", "\"\"\"")],
        quotes: DQ_SQ,
        keywords: &[
            "def", "defp", "defmodule", "defstruct", "defmacro", "do", "else", "end", "case",
            "cond", "if", "unless", "fn", "import", "alias", "require", "use", "when", "with",
            "receive", "after", "rescue", "try", "catch", "raise", "for",
        ],
        types: &["Enum", "Map", "List", "String", "Atom", "Tuple", "Process", "GenServer"],
        consts: &["true", "false", "nil", "__MODULE__"],
        attr_prefix: &['@'],
        upper_is_type: true,
        char_quotes: false,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["zig", "nim", "odin", "v"],
        line_comment: &["//", "#"],
        block_comment: None,
        multiline: NO_ML,
        quotes: DQ_SQ,
        keywords: &[
            "const", "var", "fn", "pub", "return", "if", "else", "while", "for", "switch",
            "struct", "enum", "union", "error", "try", "catch", "defer", "comptime", "inline",
            "test", "import", "proc", "let", "type", "of", "when", "discard",
        ],
        types: &[
            "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize", "isize", "f32",
            "f64", "bool", "void", "anytype", "string", "int", "float",
        ],
        consts: &["true", "false", "null", "undefined", "nil"],
        attr_prefix: &['@'],
        upper_is_type: true,
        char_quotes: true,
        flavor: Flavor::Generic,
    },
    Lang {
        names: &["diff", "patch", "udiff"],
        line_comment: &[],
        block_comment: None,
        multiline: NO_ML,
        quotes: &[],
        keywords: &[],
        types: &[],
        consts: &[],
        attr_prefix: &[],
        upper_is_type: false,
        char_quotes: false,
        flavor: Flavor::Diff,
    },
];

/// Resolve a fence info string (`rust`, `js title=x`, `Dockerfile`) to a language.
pub fn lang_for(info: &str) -> Option<&'static Lang> {
    let token = info
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if token.is_empty() {
        return None;
    }
    LANGS.iter().find(|l| l.names.contains(&token.as_str()))
}

/// Human label shown on the code-block chrome.
pub fn display_name(info: &str) -> String {
    let token = info
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if token.is_empty() { "text".into() } else { token }
}

fn is_ident_start(c: char, lang: &Lang) -> bool {
    c.is_alphabetic() || c == '_' || lang.attr_prefix.contains(&c)
}

fn is_ident_cont(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '?' || c == '!'
}

/// Highlight one line, threading `state` across lines of the same block.
pub fn highlight(lang: Option<&'static Lang>, line: &str, state: &mut HlState) -> Vec<(String, Tok)> {
    let Some(lang) = lang else {
        return vec![(line.to_string(), Tok::Text)];
    };
    match lang.flavor {
        Flavor::Diff => return diff_line(line),
        _ => {}
    }

    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<(String, Tok)> = Vec::new();
    let mut i = 0usize;

    // Continuation of a block comment from a previous line.
    if state.block_comment {
        let end = lang.block_comment.map(|(_, e)| e).unwrap_or("*/");
        match find_at(&chars, 0, end) {
            Some(pos) => {
                let stop = pos + end.chars().count();
                push(&mut out, &chars[..stop], Tok::Comment);
                state.block_comment = false;
                i = stop;
            }
            None => {
                push(&mut out, &chars, Tok::Comment);
                return out;
            }
        }
    }
    // Continuation of a multi-line string.
    if let Some(idx) = state.multiline {
        let end = lang.multiline[idx].1;
        match find_at(&chars, i, end) {
            Some(pos) => {
                let stop = pos + end.chars().count();
                push(&mut out, &chars[i..stop], Tok::Str);
                state.multiline = None;
                i = stop;
            }
            None => {
                push(&mut out, &chars[i..], Tok::Str);
                return out;
            }
        }
    }

    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut Vec<(String, Tok)>| {
        if !plain.is_empty() {
            out.push((std::mem::take(plain), Tok::Text));
        }
    };

    while i < chars.len() {
        let c = chars[i];

        // Line comments
        if let Some(marker) = lang
            .line_comment
            .iter()
            .find(|m| starts_with_at(&chars, i, m))
        {
            // `#` inside shell interpolation like `${#x}` is rare enough to ignore,
            // but a `#` that is not at a token boundary in CSS is a color literal.
            let _ = marker;
            flush(&mut plain, &mut out);
            push(&mut out, &chars[i..], Tok::Comment);
            return out;
        }

        // Block comments
        if let Some((start, end)) = lang.block_comment {
            if starts_with_at(&chars, i, start) {
                flush(&mut plain, &mut out);
                match find_at(&chars, i + start.chars().count(), end) {
                    Some(pos) => {
                        let stop = pos + end.chars().count();
                        push(&mut out, &chars[i..stop], Tok::Comment);
                        i = stop;
                    }
                    None => {
                        push(&mut out, &chars[i..], Tok::Comment);
                        state.block_comment = true;
                        return out;
                    }
                }
                continue;
            }
        }

        // Multi-line strings
        if let Some((idx, (start, end))) = lang
            .multiline
            .iter()
            .enumerate()
            .find(|(_, (s, _))| starts_with_at(&chars, i, s))
            .map(|(idx, pair)| (idx, *pair))
        {
            flush(&mut plain, &mut out);
            match find_at(&chars, i + start.chars().count(), end) {
                Some(pos) => {
                    let stop = pos + end.chars().count();
                    push(&mut out, &chars[i..stop], Tok::Str);
                    i = stop;
                }
                None => {
                    push(&mut out, &chars[i..], Tok::Str);
                    state.multiline = Some(idx);
                    return out;
                }
            }
            continue;
        }

        // Strings
        if lang.quotes.contains(&c) {
            flush(&mut plain, &mut out);
            let mut j = i + 1;
            let mut escaped = false;
            let mut closed = false;
            while j < chars.len() {
                if escaped {
                    escaped = false;
                } else if chars[j] == '\\' {
                    escaped = true;
                } else if chars[j] == c {
                    j += 1;
                    closed = true;
                    break;
                }
                j += 1;
            }
            // In C-family languages `'` is a character literal: anything that
            // doesn't look like one (`'a'`, `'\n'`) is a lifetime, and a stray
            // `'` elsewhere is an apostrophe — neither may swallow the line.
            let char_literal = closed
                && lang.char_quotes
                && j > i + 2
                && (j - i - 2) <= 8
                && chars[i + 1..j - 1].iter().all(|c| !c.is_whitespace())
                && (j - i - 2 == 1 || chars[i + 1] == '\\');
            let bad_quote = if lang.char_quotes { !char_literal } else { !closed };
            if bad_quote && c == '\'' {
                if chars.get(i + 1).is_some_and(|c| c.is_alphabetic() || *c == '_') {
                    let mut k = i + 1;
                    while k < chars.len() && is_ident_cont(chars[k]) {
                        k += 1;
                    }
                    push(&mut out, &chars[i..k], Tok::Type);
                    i = k;
                } else {
                    out.push((c.to_string(), Tok::Punct));
                    i += 1;
                }
                continue;
            }
            push(&mut out, &chars[i..j.min(chars.len())], Tok::Str);
            i = j.min(chars.len());
            continue;
        }

        // Numbers
        if c.is_ascii_digit()
            && (i == 0 || !is_ident_cont(chars[i - 1]))
        {
            flush(&mut plain, &mut out);
            let mut j = i;
            while j < chars.len()
                && (chars[j].is_ascii_alphanumeric()
                    || chars[j] == '.'
                    || chars[j] == '_'
                    || ((chars[j] == '+' || chars[j] == '-')
                        && j > i
                        && matches!(chars[j - 1], 'e' | 'E')))
            {
                j += 1;
            }
            push(&mut out, &chars[i..j], Tok::Number);
            i = j;
            continue;
        }

        // Identifiers / keywords
        if is_ident_start(c, lang) {
            flush(&mut plain, &mut out);
            let attr = lang.attr_prefix.contains(&c);
            let mut j = i + 1;
            while j < chars.len() && is_ident_cont(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let bare = word.trim_start_matches(|ch| lang.attr_prefix.contains(&ch));
            let lower = bare.to_ascii_lowercase();
            let next = chars[j..].iter().find(|c| !c.is_whitespace()).copied();

            let kind = if attr && bare.len() > 0 {
                Tok::Attr
            } else if lang.consts.contains(&bare) || lang.consts.contains(&lower.as_str()) {
                Tok::Const
            } else if lang.keywords.contains(&bare) || lang.keywords.contains(&lower.as_str()) {
                Tok::Keyword
            } else if lang.types.contains(&bare) || lang.types.contains(&lower.as_str()) {
                Tok::Type
            } else if next == Some('(') {
                Tok::Func
            } else if lang.upper_is_type && bare.chars().next().is_some_and(|c| c.is_uppercase()) {
                Tok::Type
            } else if lang.flavor == Flavor::Keyed && next == Some(':') {
                Tok::Attr
            } else {
                Tok::Text
            };
            push(&mut out, &chars[i..j], kind);
            i = j;
            continue;
        }

        // Punctuation
        if !c.is_alphanumeric() && !c.is_whitespace() {
            flush(&mut plain, &mut out);
            out.push((c.to_string(), Tok::Punct));
            i += 1;
            continue;
        }

        plain.push(c);
        i += 1;
    }
    flush(&mut plain, &mut out);
    out
}

fn diff_line(line: &str) -> Vec<(String, Tok)> {
    let kind = match line.chars().next() {
        Some('+') if !line.starts_with("+++") => Tok::Str,
        Some('-') if !line.starts_with("---") => Tok::Keyword,
        Some('@') => Tok::Func,
        Some('d') if line.starts_with("diff ") => Tok::Attr,
        Some('+') | Some('-') => Tok::Attr,
        _ => Tok::Comment,
    };
    vec![(line.to_string(), kind)]
}

fn push(out: &mut Vec<(String, Tok)>, chars: &[char], kind: Tok) {
    if chars.is_empty() {
        return;
    }
    out.push((chars.iter().collect(), kind));
}

fn starts_with_at(chars: &[char], at: usize, pat: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    if p.is_empty() || at + p.len() > chars.len() {
        return false;
    }
    chars[at..at + p.len()] == p[..]
}

fn find_at(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    let p: Vec<char> = pat.chars().collect();
    if p.is_empty() {
        return None;
    }
    (from..chars.len().saturating_sub(p.len() - 1)).find(|&i| chars[i..i + p.len()] == p[..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(lang: &str, line: &str) -> Vec<(String, Tok)> {
        let mut st = HlState::default();
        highlight(lang_for(lang), line, &mut st)
    }

    fn kind_of(v: &[(String, Tok)], text: &str) -> Option<Tok> {
        v.iter().find(|(t, _)| t == text).map(|(_, k)| *k)
    }

    #[test]
    fn resolves_language_aliases() {
        assert!(lang_for("rs").is_some());
        assert!(lang_for("TypeScript").is_some());
        assert!(lang_for("rust,ignore").is_some());
        assert!(lang_for("brainfuck").is_none());
        assert_eq!(display_name(""), "text");
        assert_eq!(display_name("rust title=x"), "rust");
    }

    #[test]
    fn classifies_rust_tokens() {
        let v = toks("rust", "pub fn main(x: u32) { // hi");
        assert_eq!(kind_of(&v, "pub"), Some(Tok::Keyword));
        assert_eq!(kind_of(&v, "main"), Some(Tok::Func));
        assert_eq!(kind_of(&v, "u32"), Some(Tok::Type));
        assert_eq!(kind_of(&v, "// hi"), Some(Tok::Comment));
    }

    #[test]
    fn lifetime_is_not_an_unterminated_string() {
        let v = toks("rust", "struct S<'a> { x: &'a str }");
        assert_eq!(kind_of(&v, "'a"), Some(Tok::Type));
        // The rest of the line must still be tokenized normally.
        assert_eq!(kind_of(&v, "str"), Some(Tok::Type));
    }

    #[test]
    fn apostrophes_do_not_swallow_shell_lines() {
        let v = toks("bash", "echo don't stop");
        assert!(v.iter().all(|(_, k)| *k != Tok::Str));
    }

    #[test]
    fn block_comments_span_lines() {
        let mut st = HlState::default();
        let lang = lang_for("c");
        let first = highlight(lang, "int x; /* start", &mut st);
        assert!(st.block_comment);
        assert_eq!(kind_of(&first, "/* start"), Some(Tok::Comment));
        let mid = highlight(lang, "still comment", &mut st);
        assert_eq!(mid[0].1, Tok::Comment);
        let last = highlight(lang, "end */ int y;", &mut st);
        assert!(!st.block_comment);
        assert_eq!(kind_of(&last, "int"), Some(Tok::Type));
    }

    #[test]
    fn python_docstrings_span_lines() {
        let mut st = HlState::default();
        let lang = lang_for("python");
        highlight(lang, "def f():", &mut st);
        highlight(lang, "    \"\"\"doc", &mut st);
        assert!(st.multiline.is_some());
        highlight(lang, "    more doc\"\"\"", &mut st);
        assert!(st.multiline.is_none());
    }

    #[test]
    fn diff_lines_are_classified_by_prefix() {
        assert_eq!(toks("diff", "+added")[0].1, Tok::Str);
        assert_eq!(toks("diff", "-removed")[0].1, Tok::Keyword);
        assert_eq!(toks("diff", "@@ -1 +1 @@")[0].1, Tok::Func);
    }

    #[test]
    fn unknown_language_passes_through() {
        let mut st = HlState::default();
        let v = highlight(None, "anything at all", &mut st);
        assert_eq!(v, vec![("anything at all".to_string(), Tok::Text)]);
    }
}
