# skimd GFM showcase

A paragraph with **bold**, *italic*, ***bold italic***, `inline code`,
~~strikethrough~~, a [link](https://example.com), and an autolink
<https://example.com/auto>.

## Headings

### Third level

#### Fourth level

##### Fifth level

###### Sixth level

Setext heading one
==================

Setext heading two
------------------

## Lists

- Unordered item one
- Item two with **bold** and `code`
  - Nested item
  - Another nested item
- Item three

1. Ordered one
2. Ordered two
3. Ordered three

## Task lists

- [x] Completed task
- [ ] Open task with a [link](https://example.com)
- [ ] Another open task

## Table

| Language | Paradigm       | Year |
|----------|----------------|-----:|
| Rust     | Multi-paradigm | 2015 |
| Haskell  | Functional     | 1990 |
| C        | Imperative     | 1972 |

## Blockquote

> The best code is no code at all.
>
> > Nested quote with `inline code`.

## Code fences

```rust
fn segment(source: &str) -> Vec<Range<usize>> {
    let parser = Parser::new_ext(source, OPTIONS);
    parser.into_offset_iter().collect_blocks()
}
```

```python
def splice(source: str, start: int, end: int, edited: str) -> str:
    return source[:start] + edited + source[end:]
```

```json
{ "name": "skimd", "fast": true, "startup_ms": 42 }
```

```sh
hyperfine --warmup 3 './target/release/skimd --bench-first-frame samples/torture.md'
```

```
plain fence with no language
```

## Rule

---

## Reference link

This paragraph uses a [reference link][spec].

[spec]: https://spec.commonmark.org

Final paragraph after everything, with a hard break  
and a second line.
