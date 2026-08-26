---
title: GFM Showcase
author: mdre
tags: [markdown, terminal]
---

# GFM Showcase

A single document that exercises every renderer path in **mdre** — headings,
emphasis, lists, tables, alerts, footnotes and highlighted code.

## Inline formatting

Regular text with **bold**, *italic*, ***both***, ~~struck through~~ and
`inline_code()`. Links look like [the CommonMark spec](https://spec.commonmark.org)
and bare autolinks like <https://github.com> work too. Here is a footnote
reference[^why].

## Links Display
Press Shift+L (uppercase L) in Reader mode [to see this broken link flagged](doesntexist.md). 

[^why]: Footnotes are a GFM extension, rendered inline where they are defined.

## Lists

- Unordered item
- Item with **markup** inside
  - Nested one level
    - And two levels
- Back to the top level

1. First
2. Second
   1. Nested ordered
   2. Sibling
3. Third

### Task list

- [x] Parse GitHub-Flavored Markdown
- [x] Syntax highlight fenced code
- [ ] Ship a plugin API
- [ ] Sleep

## Quotes and alerts

> A plain block quote.
> It wraps across lines and keeps its bar.

> [!NOTE]
> Alerts are a GFM feature; each kind gets its own colour and icon.

> [!WARNING]
> Nested content works too:
> - a list
> - inside an alert

## Table

| Language | Extension | Highlighted | Notes                  |
| -------- | :-------: | ----------: | ---------------------- |
| Rust     | `.rs`     | yes         | keywords, types, attrs |
| Python   | `.py`     | yes         | docstrings tracked     |
| JSON     | `.json`   | yes         | keys tinted            |
| Cobol    | `.cbl`    | no          | falls back to plain    |
| Hey      | `.hey`    | ok          | sure                   |


## Code

```rust
/// Render a document to styled terminal lines.
pub fn render(src: &str, width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for (event, range) in Parser::new_ext(src, options()).into_offset_iter() {
        match event {
            Event::Text(t) => out.push(Line::raw(t.to_string())),
            _ => {} // everything else
        }
    }
    out
}
```

```python
def fib(n: int) -> int:
    """Classic, and still the best demo."""
    a, b = 0, 1

    for _ in range(n):
        a, b = b, a + b

    return a
```

```bash
# find every markdown file changed this week
find . -name '*.md' -mtime -7 | xargs wc -w
```

Indented code also renders:

    plain indented block
    no language, no highlighting

## Rules and breaks

Text before the rule.

---

Text after the rule, with a hard break at the end of this line  
and the continuation right here.

## Images and HTML

![A diagram](https://example.com/diagram.png)

<div class="raw-html">Raw HTML blocks are shown dimmed.</div>

[^ignored]: unused footnote definition
