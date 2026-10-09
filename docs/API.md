# fmt-lang &mdash; API Reference

> Complete reference for every public item in `fmt-lang`, with examples.
> **Status: pre-1.0 (v0.2.0, Foundation).** The surface is designed across the
> 0.x series and frozen at `1.0.0`; until then a minor release may change it.
> See [`../dev/ROADMAP.md`](../dev/ROADMAP.md).

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>

## Table of contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Concepts](#concepts)
  - [Gaps and how spacing is resolved](#gaps-and-how-spacing-is-resolved)
  - [Groups and indentation](#groups-and-indentation)
  - [Comment attachment](#comment-attachment)
  - [Error nodes and verbatim kinds](#error-nodes-and-verbatim-kinds)
  - [Kept regions (lexical errors)](#kept-regions-lexical-errors)
  - [Lists and trailing separators](#lists-and-trailing-separators)
  - [When tokens may touch](#when-tokens-may-touch)
  - [Line endings, the start and the end of the file](#line-endings-the-start-and-the-end-of-the-file)
- [`format`](#format)
- [`format_keeping`](#format_keeping)
- [`format_doc`](#format_doc)
- [`can_touch`](#can_touch)
- [`Rules`](#rules)
- [`NodeRule`](#noderule)
- [`TokenRule`](#tokenrule)
- [`Space`](#space)
- [`Indent`](#indent)
- [`Trailing`](#trailing)
- [`Style`](#style)
- [`RuleError`](#ruleerror)
- [`FormatError`](#formaterror)
- [`MAX_INDENT_STEP`](#max_indent_step)
- [Re-exports](#re-exports)
- [Feature flags](#feature-flags)
- [Guarantees](#guarantees)
- [Limits](#limits)

## Overview

`fmt-lang` formats source code from declarative rules over a lossless
[`syntax-lang`](https://crates.io/crates/syntax-lang) tree, and renders the
result through [`pretty-lang`](https://crates.io/crates/pretty-lang). A
language gets a formatter from configuration, not code: its layout is
described as [`Rules`](#rules) (plain data, keyed by kind names, the way a
LexerSketch sketch names them), [compiled](#rulescompile) once against the
language's kinds into a [`Style`](#style), and applied to any tree of that
language with [`format`](#format).

| Item | Kind | Purpose |
|---|---|---|
| [`format`](#format) | fn | **Tier-1.** Format a tree at a width. |
| [`format_keeping`](#format_keeping) | fn | Format, leaving given regions (lexical errors) as written. |
| [`format_doc`](#format_doc) | fn | Build the `pretty_lang::Doc` instead of rendering it. |
| [`can_touch`](#can_touch) | fn | The default test for whether two tokens may be written touching. |
| [`Rules`](#rules) | struct | A language's formatting rules, as data. |
| [`NodeRule`](#noderule) | struct | Layout rules for one node kind. |
| [`TokenRule`](#tokenrule) | struct | Spacing rules for one token kind (or every token). |
| [`Space`](#space) | enum | What goes between two neighbouring pieces of output. |
| [`Indent`](#indent) | enum | How a node indents its contents. |
| [`Trailing`](#trailing) | enum | Trailing-separator policy for a delimited list. |
| [`Style`](#style) | struct | Rules compiled against one language. |
| [`RuleError`](#ruleerror) | enum | Why rules could not be compiled. |
| [`FormatError`](#formaterror) | enum | Why a tree could not be formatted. |
| [`MAX_INDENT_STEP`](#max_indent_step) | const | The widest indentation step. |
| [`syntax_lang`, `pretty_lang`](#re-exports) | re-exports | The tree and document crates, whole. |

## Installation

```toml
[dependencies]
fmt-lang = "0.2"
```

Without the standard library (needs only `alloc`):

```toml
[dependencies]
fmt-lang = { version = "0.2", default-features = false }
```

## Quick start

A JSON-like language forged with [`lang-forge`](https://crates.io/crates/lang-forge),
and its whole formatter:

```rust
use fmt_lang::{format, Indent, NodeRule, Rules, Space, TokenRule, Trailing};
use lang_forge::Language;

let json = Language::from_lsf(r#"
    [language]
    name = "json"
    [lexer]
    strings = ['"']
    [rules]
    document = "value"
    value    = "object | array | STRING | NUMBER | 'true' | 'false' | 'null'"
    object   = "'{' (member (',' member)* ','?)? '}'"
    member   = "STRING ':' value"
    array    = "'[' (value (',' value)* ','?)? ']'"
"#)?;

let style = Rules::new()
    .indent(2)
    .verbatim("ERROR")
    .token(TokenRule::new(":").before(Space::None).after(Space::Single))
    .node(NodeRule::new("object").group().indent(Indent::Block)
        .delimiters("{", "}", Space::Line)
        .separator(",", Space::Line, Trailing::Never))
    .node(NodeRule::new("array").group().indent(Indent::Block)
        .delimiters("[", "]", Space::SoftLine)
        .separator(",", Space::Line, Trailing::Never))
    .compile(|name| json.kind(name))?;

let parse = json.parse("{\"a\":[1,2,],   \"b\" :{}}");
assert_eq!(format(parse.tree(), parse.source(), &style, 80)?, "{ \"a\": [1, 2], \"b\": {} }\n");
assert_eq!(
    format(parse.tree(), parse.source(), &style, 16)?,
    "{\n  \"a\": [1, 2],\n  \"b\": {}\n}\n",
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Concepts

### Gaps and how spacing is resolved

The formatter writes significant tokens in order, and decides everything
between two neighbouring significant tokens (a *gap*) at once. Trivia
(whitespace and comments) never counts as a neighbour. Every rule that speaks
about a gap contributes a [`Space`](#space):

1. the left token's `after` and the right token's `before`, each taken from the
   most specific [`TokenRule`](#tokenrule): a rule in the parent node's
   [`NodeRule`](#noderule) for that exact kind, then the parent's rule for any
   token, then a top-level rule for the kind, then a top-level rule for any
   token (a node's separator role, if the token is its separator, comes first);
2. the `after` of every node that ends at the gap and the `before` of every
   node that starts there;
3. the delimiter spacing of the innermost node holding both tokens, if one of
   them is its opening or closing delimiter.

The contributions are **joined**: a gap gets a space if any rule asks for one,
may break if any allows it, and always breaks if any demands it. If no rule
contributes, the gap keeps the source's whitespace exactly, except that spaces
at the ends of lines are removed and `\r\n` becomes `\n`.

Two cases override the join. A delimited node whose body is empty (opening
delimiter followed directly by the closing one) gets its
[`empty`](#noderuleempty) spacing alone. A trailing separator (the last
separator before a closing delimiter, written or inserted) takes only the
closing delimiter's spacing, so `[1, 2,]` stays tight when flat.

A gap that resolves to [`Space::Hard`](#space) keeps up to the configured
number of blank lines the source had there.

### Groups and indentation

A [`NodeRule::group`](#noderulegroup) node is one pretty-lang group: all of
its [`Line`](#space) and [`SoftLine`](#space) gaps stay flat if the node fits
in the remaining width, and all break otherwise. Groups nest; an outer group
breaks before an inner one.

[`Indent::Block`](#indent) indents the body between the node's delimiters;
[`Indent::Hanging`](#indent) indents every break inside the node. A hanging
node directly inside a node with the same rule (a chain such as
`a + b + c`, which parses as `(a + b) + c`) shares its parent's indentation
rather than adding another level. Indentation stops growing at
[`Rules::max_indent`](#rulesmax_indent).

A gap's line break is written inside the innermost node holding both of its
tokens, so a break before a node is indented by the node's parent, and a
comment that ends a line never sits inside a sibling's group.

### Comment attachment

Comments are trivia tokens whose text is not all whitespace; that includes
characters a lexer did not recognize. In each gap they are attached by where
they sit in the source:

- **Trailing**: no line break between the comment and the token before the
  gap. It stays on that token's line, after one space.
- **Leading**: the comment started a line. It starts a line in the output, at
  the indentation of the gap, keeping up to the configured blank lines before
  it. If the next token shared its line in the source, it stays on that line
  (unless the gap is [`Hard`](#space)).
- **Dangling**: a leading comment in the last gap of an
  [`Indent::Block`](#indent) body, just before the closing delimiter. It is
  indented with the body, and the closing delimiter returns to the node's
  level.

A comment followed by a line break in the source is followed by one in the
output, so a line comment always ends its line. Comments never cross a
significant token, so their order is preserved, and their text is written
unchanged (a multi-line block comment keeps its inner lines exactly).

### Error nodes and verbatim kinds

[`Rules::verbatim`](#rulesverbatim) names node kinds, such as a parser's
`ERROR` node, whose contents are written exactly as in the source, from their
first to their last significant token. Comments and whitespace at their edges
are still attached and spaced normally, and node rules naming the kind still
apply around it. Lists containing an error node anywhere inside are never given
[trailing-separator edits](#lists-and-trailing-separators).

### Kept regions (lexical errors)

Some tokens end where the source's whitespace happens to be: an unterminated
string usually ends at the line break, an unterminated block comment at the end
of the input. Removing that line break, or appending one, would change the
token, and the formatter cannot tell such a token from a complete one.
[`format_keeping`](#format_keeping) takes the regions of lexical errors (an
editor integration has them as diagnostics) and leaves every gap that touches
one byte for byte as written; no separator is inserted into or removed from a
list holding one. Idempotence then holds as long as the regions mark the same
tokens on the next run, which lexical errors do.

### Lists and trailing separators

A node rule with [`delimiters`](#noderuledelimiters) and a
[`separator`](#noderuleseparator) describes a list. The
[`Trailing`](#trailing) policy may add a separator after the last element
(`Always`) or remove a trailing one (`Never`). Because those policies change
the token stream, they apply only to lists that are well formed: both
delimiters present, elements and separators strictly alternating, at least one
element, no error node and no node missing one of its delimiters anywhere
inside, and no kept region inside. Everything else is left as written.

An inserted separator is spaced exactly as one written in the source would be,
and comments trailing the last element move after it.

### When tokens may touch

Where a rule would remove whitespace the source had between two tokens, the
formatter first asks whether writing them touching is safe, because a lexer may
read the joined text differently (`- -x` must not become `--x`). Tokens that
already touched in the source may always touch. Otherwise the style's touch test
decides: [`can_touch`](#can_touch) by default, or an exact test supplied with
[`Style::with_touch`](#stylewith_touch). If the answer is no, one space is
kept.

### Line endings, the start and the end of the file

- Output line breaks are `\n`. A token or comment that ends in a lone `\r` is
  followed by `\r\n`, so the `\r` is not read as part of the line break.
- Whitespace at the start of the file is removed; a byte-order mark is kept.
- Comments before the first and after the last token keep their line
  structure, with blank lines capped.
- Non-empty output ends with one line break when
  [`Rules::final_newline`](#rulesfinal_newline) is on (the default), unless
  the last gap touches a kept region, in which case the source's own ending is
  kept.

## `format`

```rust,ignore
pub fn format<K: TokenKind + Ord>(
    tree: &Node<K>,
    source: &str,
    style: &Style<K>,
    width: usize,
) -> Result<String, FormatError>
```

**Tier-1 entry point.** Formats the source `tree` was parsed from, at `width`
columns (counted in `char`s).

- `tree`: a lossless tree of `source`. Its tokens, trivia included, must cover a
  contiguous range of `source`; it may be a whole file or any subtree, and may
  contain error nodes. The output covers the tree's range.
- `source`: the text the tree's spans index.
- `style`: rules compiled for the tree's language.
- `width`: the line width pretty-lang fits groups to.

**Errors:** [`FormatError`](#formaterror) if the tree's tokens do not describe
`source`.

```rust
use fmt_lang::syntax_lang::{Builder, Span, Token, TokenKind};
use fmt_lang::{format, Rules, Space, TokenRule};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum K { Sum, Num, Plus, Ws }
impl TokenKind for K {
    fn is_trivia(&self) -> bool { matches!(self, K::Ws) }
}

let source = "1   +2";
let mut b = Builder::new();
b.start_node(K::Sum);
b.token(Token::new(K::Num, Span::new(0, 1)));
b.token(Token::new(K::Ws, Span::new(1, 4)));
b.token(Token::new(K::Plus, Span::new(4, 5)));
b.token(Token::new(K::Num, Span::new(5, 6)));
b.finish_node();
let tree = b.finish()?;

let style = Rules::new()
    .final_newline(false)
    .token(TokenRule::new("+").around(Space::Single))
    .compile(|name| (name == "+").then_some(K::Plus))?;
assert_eq!(format(&tree, source, &style, 80)?, "1 + 2");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## `format_keeping`

```rust,ignore
pub fn format_keeping<K: TokenKind + Ord>(
    tree: &Node<K>,
    source: &str,
    style: &Style<K>,
    width: usize,
    keep: &[Span],
) -> Result<String, FormatError>
```

Formats like [`format`](#format), but every gap touching one of the `keep`
regions is written exactly as in the source (see
[Kept regions](#kept-regions-lexical-errors)). A gap touches a region when the
region overlaps the token before the gap, the gap, or the token after it; an
empty region keeps the gap it falls in. For the first and last gaps of the
file, the comments in the gap count too. Regions may overlap and come in any
order.

Pass the spans of **lexical** errors. Parse errors such as "expected `;`" can
point somewhere else once whitespace has changed, so keeping their regions can
shift between runs.

**Errors:** as for [`format`](#format).

```rust
use fmt_lang::{format, format_keeping, NodeRule, Rules, Space, Trailing};
use lang_forge::Language;

let lang = Language::from_lsf(
    "[language]\nname = \"j\"\n[lexer]\nstrings = ['\"']\n[rules]\n\
     list = \"'[' (STRING (',' STRING)*)? ']'\"\n",
)?;
let style = Rules::new()
    .node(NodeRule::new("list").delimiters("[", "]", Space::None)
        .separator(",", Space::Single, Trailing::Preserve))
    .compile(|n| lang.kind(n))?;

// `"open` is unterminated: the string ends at the line break.
let parse = lang.parse("[\"open\n, \"b\"]");
let spans: Vec<_> = parse.diagnostics().iter().map(|d| d.primary().span()).collect();
assert_eq!(format_keeping(parse.tree(), parse.source(), &style, 80, &spans)?, "[\"open\n, \"b\"]\n");
// Without the region the break goes, and the comma joins the string.
assert_eq!(format(parse.tree(), parse.source(), &style, 80)?, "[\"open, \"b\"]\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## `format_doc`

```rust,ignore
pub fn format_doc<K: TokenKind + Ord>(
    tree: &Node<K>,
    source: &str,
    style: &Style<K>,
    keep: &[Span],
) -> Result<Doc, FormatError>
```

Builds the `pretty_lang::Doc` that [`format_keeping`](#format_keeping) (or,
with no `keep` regions, [`format`](#format)) renders. Use it to render into an
existing buffer (`Doc::render_into`), a writer (`Doc::render_writer`, with the
`std` feature), or at several widths without walking the tree again.

**Errors:** as for [`format`](#format).

```rust
use fmt_lang::syntax_lang::{Element, Node, Span, Token, TokenKind};
use fmt_lang::{format_doc, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Word;
impl TokenKind for Word {}

let tree = Node::new(Word, vec![Element::Token(Token::new(Word, Span::new(0, 5)))]);
let doc = format_doc(&tree, "hello", &Style::default(), &[])?;
let mut out = String::new();
doc.render_into(80, &mut out)?;
assert_eq!(out, "hello\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## `can_touch`

```rust,ignore
pub fn can_touch(left: &str, right: &str) -> bool
```

The default test for whether two tokens may be written with nothing between
them (`left` is the earlier token's text). It answers `false` when the
touching characters could plausibly lex as one token: two word characters, two
ASCII punctuation characters, a quote next to a word character or another
quote, or a dot next to a digit. Brackets, parentheses, commas, and semicolons
may touch anything except an identical bracket or brace. It is a heuristic;
see [`Style::with_touch`](#stylewith_touch).

```rust
use fmt_lang::can_touch;

assert!(can_touch("f", "("));
assert!(can_touch("\"key\"", ":"));
assert!(!can_touch("let", "x"));
assert!(!can_touch("-", "-"));
assert!(!can_touch("1", ".5"));
```

## `Rules`

```rust,ignore
pub struct Rules { /* private fields */ }
```

A language's formatting rules, as data. Built with chained calls, then
[compiled](#rulescompile) against the language's kinds. Implements `Clone`,
`Debug`, `PartialEq`, `Eq`, `Hash`, and `Default`.

| Method | Default | Meaning |
|---|---|---|
| `Rules::new()` | | No rules. |
| `Rules::conventional()` | | A preset of common token spacing (below). |
| `.indent(columns: u8)` | 4 | Columns per indentation level, at most [`MAX_INDENT_STEP`](#max_indent_step). |
| `.max_indent(columns: u16)` | 120 | The deepest indentation produced; nesting past it adds none. The output budget against hostile input. |
| `.max_blank_lines(max: u8)` | 1 | Blank lines kept at `Hard` gaps (gaps keeping their whitespace are not capped). |
| `.final_newline(yes: bool)` | `true` | Whether non-empty output ends with a line break. |
| `.verbatim(kind)` | | A node kind written exactly as in the source (e.g. `"ERROR"`). |
| `.token(rule: TokenRule)` | | A top-level token rule. Later rules for the same kind refine earlier ones, side by side. |
| `.node(rule: NodeRule)` | | A node rule. |

### `Rules::conventional`

Common C-family token spacing, every rule [optional](#tokenruleoptional) so the
preset compiles against any language: nothing before `,` `;` `)` `]`, one space
after `,` and `;`; nothing after `(` `[`; one space around `=` `==` `!=` `<=`
`>=` `+=` `-=` `*=` `/=` `%=` `&&` `||` `=>` `->`. Operators that are also
prefix operators (`-` `+` `*` `&` `!`) and the angle brackets are left out:
spacing them needs node context.

```rust
use fmt_lang::{NodeRule, Rules, Space};

let rules = Rules::conventional().node(NodeRule::new("stmt").before(Space::Hard));
# let _ = rules;
```

### `Rules::compile`

```rust,ignore
pub fn compile<K: Ord + Clone>(
    &self,
    resolve: impl FnMut(&str) -> Option<K>,
) -> Result<Style<K>, RuleError>
```

Resolves every kind name with `resolve` (`None` when the language has no such
kind) and compiles the rules into a [`Style`](#style). For a lang-forge
language, `resolve` is `|name| lang.kind(name)`.

**Errors:** [`RuleError::UnknownKind`](#ruleerror) for a non-optional rule
naming an unknown kind; `DuplicateNode` for two node rules of one kind;
`IndentTooWide`; `SeparatorIsDelimiter`; `TrailingNeedsDelimiters`.

```rust
use fmt_lang::{NodeRule, Rules, Space};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind { Stmt, Semi }

let style = Rules::new()
    .node(NodeRule::new("stmt").before(Space::Hard))
    .compile(|name| match name {
        "stmt" => Some(Kind::Stmt),
        ";" => Some(Kind::Semi),
        _ => None,
    })?;
# let _ = style;
# Ok::<(), fmt_lang::RuleError>(())
```

### `Rules::max_indent`

```rust
use fmt_lang::Rules;
let rules = Rules::new().max_indent(60);
# let _ = rules;
```

### `Rules::verbatim`

```rust
use fmt_lang::Rules;
let rules = Rules::new().verbatim("ERROR").verbatim("raw_block");
# let _ = rules;
```

### `Rules::final_newline`

```rust
use fmt_lang::Rules;
let rules = Rules::new().final_newline(false);
# let _ = rules;
```

## `NodeRule`

```rust,ignore
pub struct NodeRule { /* private fields */ }
```

Layout rules for one node kind. Implements `Clone`, `Debug`, `PartialEq`,
`Eq`, `Hash`.

| Method | Meaning |
|---|---|
| `NodeRule::new(kind)` | A rule for nodes named `kind`, nothing set. |
| `.group()` | The node is a group: flat if it fits, every line opportunity broken otherwise. |
| `.indent(Indent)` | How the node indents its contents (by [`Rules::indent`](#rules) columns). |
| `.before(Space)` / `.after(Space)` | Spacing in the gap before the node's first token / after its last. |
| `.delimiters(open, close, inner: Space)` | The node's delimiter kinds, and the spacing just inside them. Recognised only as the node's first / last significant child. |
| `.empty(Space)` | Spacing between the delimiters when the body is empty (default `None`). |
| `.separator(kind, after: Space, trailing: Trailing)` | The list separator among the node's direct child tokens (nothing before it), the spacing after it, and the trailing policy. |
| `.separator_text(text)` | The text `Trailing::Always` writes (default: the separator's kind name). |
| `.blank_lines(max: u8)` | Caps blank lines inside this node (overrides `Rules::max_blank_lines`). |
| `.token(TokenRule)` | A token rule for the node's direct child tokens; wins over top-level rules. |
| `.optional()` | Dropped at compile time if the language lacks a kind it names. |

```rust
use fmt_lang::{Indent, NodeRule, Space, TokenRule, Trailing};

let object = NodeRule::new("object")
    .group()
    .indent(Indent::Block)
    .delimiters("{", "}", Space::Line)
    .separator(",", Space::Line, Trailing::Never);
let block = NodeRule::new("block")
    .indent(Indent::Block)
    .delimiters("{", "}", Space::Hard)
    .empty(Space::Single);
let prefix = NodeRule::new("prefix").token(TokenRule::any().before(Space::None).after(Space::None));
# let _ = (object, block, prefix);
```

### `NodeRule::group`

See [Groups and indentation](#groups-and-indentation).

### `NodeRule::delimiters`

See [Lists and trailing separators](#lists-and-trailing-separators).

### `NodeRule::empty`

```rust
use fmt_lang::{NodeRule, Space};
// `{ }` rather than `{}` for an empty block.
let block = NodeRule::new("block").delimiters("{", "}", Space::Hard).empty(Space::Single);
# let _ = block;
```

### `NodeRule::separator`

```rust
use fmt_lang::{NodeRule, Space, Trailing};
let args = NodeRule::new("args")
    .delimiters("(", ")", Space::SoftLine)
    .separator(",", Space::Line, Trailing::Preserve);
# let _ = args;
```

## `TokenRule`

```rust,ignore
pub struct TokenRule { /* private fields */ }
```

Spacing rules for one token kind, or every token. At the top level of
[`Rules`](#rules) it is a default wherever the token appears; inside a
[`NodeRule`](#noderule) it applies to that node's direct child tokens only and
wins. Implements `Clone`, `Debug`, `PartialEq`, `Eq`, `Hash`.

| Method | Meaning |
|---|---|
| `TokenRule::new(kind)` | A rule for tokens named `kind`. |
| `TokenRule::any()` | A rule for every token (inside a node: every direct child token). |
| `.before(Space)` / `.after(Space)` | Spacing in the gap before / after the token. |
| `.around(Space)` | The same on both sides. |
| `.optional()` | Dropped at compile time if the language has no such kind. |

```rust
use fmt_lang::{Space, TokenRule};

let plus = TokenRule::new("+").around(Space::Single);
let comma = TokenRule::new(",").before(Space::None).after(Space::Single);
let arrow = TokenRule::new("=>").around(Space::Single).optional();
# let _ = (plus, comma, arrow);
```

### `TokenRule::optional`

```rust
use fmt_lang::{Rules, Space, TokenRule};

let rules = Rules::new().token(TokenRule::new("=>").around(Space::Single).optional());
let style = rules.compile(|name| (name == "+").then_some(1u8))?; // no `=>`: dropped
# let _ = style;
# Ok::<(), fmt_lang::RuleError>(())
```

## `Space`

```rust,ignore
#[non_exhaustive]
pub enum Space { None, Single, SoftLine, Line, Hard }
```

| Value | Group fits | Group breaks |
|---|---|---|
| `None` | nothing | nothing |
| `Single` | one space | one space |
| `SoftLine` | nothing | a line break |
| `Line` | one space | a line break |
| `Hard` | a line break | a line break |

Joining two values takes the larger on each axis (space, breaking), so
`SoftLine` joined with `Single` is `Line`, and anything joined with `Hard` is
`Hard`.

```rust
use fmt_lang::{Rules, Space, TokenRule};
let rules = Rules::new().token(TokenRule::new(",").before(Space::None).after(Space::Single));
# let _ = rules;
```

## `Indent`

```rust,ignore
#[non_exhaustive]
pub enum Indent { None, Block, Hanging }
```

- `None` (default): no indentation of its own.
- `Block`: indent the body between the node's delimiters; the closing delimiter
  returns to the node's level. Without delimiters in the tree, nothing.
- `Hanging`: indent every line break inside the node; chains of the same rule
  indent once.

```rust
use fmt_lang::{Indent, NodeRule, Space};
let block = NodeRule::new("block").delimiters("{", "}", Space::Hard).indent(Indent::Block);
# let _ = block;
```

## `Trailing`

```rust,ignore
#[non_exhaustive]
pub enum Trailing { Preserve, Always, Never }
```

- `Preserve` (default): keep a trailing separator if present; never add one.
- `Always`: a non-empty list ends with a separator.
- `Never`: remove a trailing separator.

`Always` and `Never` change the token stream, so they apply only to
well-formed lists (see [Lists](#lists-and-trailing-separators)), and need
[`delimiters`](#noderuledelimiters) (else
[`RuleError::TrailingNeedsDelimiters`](#ruleerror)). A separator inserted at
the end of a list that stays flat is written flat: `[1, 2,]`. A "comma only
when broken" policy needs a break-conditional document that pretty-lang 1.x
does not have; it is planned for a later release (see the ROADMAP).

```rust
use fmt_lang::{NodeRule, Space, Trailing};
let array = NodeRule::new("array")
    .delimiters("[", "]", Space::SoftLine)
    .separator(",", Space::Line, Trailing::Never);
# let _ = array;
```

## `Style`

```rust,ignore
pub struct Style<K> { /* private fields */ }
```

Rules compiled against one language, ready for [`format`](#format). Built with
[`Rules::compile`](#rulescompile). `Style::default()` is the empty style:
every gap keeps its original whitespace. Implements `Clone`, `Debug`
(when `K: Debug`), `Default`.

Lookups are a binary search over rules sorted by kind, done once per node and
once per token.

### `Style::with_touch`

```rust,ignore
pub fn with_touch(self, touch: fn(&str, &str) -> bool) -> Self
```

Replaces the [touch test](#when-tokens-may-touch) (default
[`can_touch`](#can_touch)). A language that knows its lexer supplies an exact
one.

```rust
use fmt_lang::{can_touch, Style};

fn no_minus_digit(left: &str, right: &str) -> bool {
    let digit = right.starts_with(|c: char| c.is_ascii_digit());
    !(left.ends_with('-') && digit) && can_touch(left, right)
}
let style = Style::<u8>::default().with_touch(no_minus_digit);
# let _ = style;
```

## `RuleError`

```rust,ignore
#[non_exhaustive]
pub enum RuleError {
    UnknownKind { name: String },
    DuplicateNode { name: String },
    IndentTooWide { columns: u8 },
    SeparatorIsDelimiter { node: String },
    TrailingNeedsDelimiters { node: String },
}
```

| Variant | Meaning | What to do |
|---|---|---|
| `UnknownKind` | A rule names a kind the language lacks. | Fix the spelling, or mark the rule `optional`. |
| `DuplicateNode` | Two node rules for one kind. | Merge them. |
| `IndentTooWide` | `Rules::indent` exceeds [`MAX_INDENT_STEP`](#max_indent_step). | Use a smaller step. |
| `SeparatorIsDelimiter` | A node's separator is one of its delimiters. | Use distinct kinds. |
| `TrailingNeedsDelimiters` | `Trailing::Always`/`Never` without delimiters. | Add delimiters or use `Preserve`. |

Implements `Display` and `core::error::Error`.

```rust
use fmt_lang::{NodeRule, RuleError, Rules};

let err = Rules::new().node(NodeRule::new("objekt")).compile(|_| None::<u8>).unwrap_err();
assert_eq!(err, RuleError::UnknownKind { name: "objekt".into() });
```

## `FormatError`

```rust,ignore
#[non_exhaustive]
pub enum FormatError {
    OutOfBounds { start: u32, end: u32, len: usize },
    NotCharBoundary { offset: u32 },
    NotContiguous { expected: u32, found: u32 },
}
```

The formatter accepts any tree shape, error nodes included; it refuses only a
tree whose tokens do not describe the source, because formatting it would lose
or invent text. In every case the tree was built from different text: parse the
source again.

| Variant | Meaning |
|---|---|
| `OutOfBounds` | A token's span reaches past the end of the source. |
| `NotCharBoundary` | A token boundary falls inside a multi-byte character. |
| `NotContiguous` | Consecutive tokens leave a gap or overlap. |

Implements `Display` and `core::error::Error`.

```rust
use fmt_lang::syntax_lang::{Element, Node, Span, Token, TokenKind};
use fmt_lang::{format, FormatError, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Word;
impl TokenKind for Word {}

let tree = Node::new(Word, vec![Element::Token(Token::new(Word, Span::new(0, 9)))]);
let err = format(&tree, "short", &Style::default(), 80).unwrap_err();
assert_eq!(err, FormatError::OutOfBounds { start: 0, end: 9, len: 5 });
```

## `MAX_INDENT_STEP`

```rust,ignore
pub const MAX_INDENT_STEP: u8 = 16;
```

The widest indentation step [`Rules::indent`](#rules) accepts.

## Re-exports

- `fmt_lang::syntax_lang`: the whole [`syntax-lang`](https://crates.io/crates/syntax-lang)
  crate (`Node`, `Element`, `Token`, `TokenKind`, `Span`, `Builder`).
- `fmt_lang::pretty_lang`: the whole [`pretty-lang`](https://crates.io/crates/pretty-lang)
  crate (`Doc`).

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `std` | yes | The standard library, forwarded to `syntax-lang` and `pretty-lang`. Without it the crate is `no_std` and needs only `alloc`. |

## Guarantees

Each is checked by property tests in [`tests/properties.rs`](../tests/properties.rs)
over random valid and invalid sources of two forged languages (a JSON-like
data language and a calc-like expression language) and over arbitrary
hand-built trees:

| Guarantee | How it is checked |
|---|---|
| Idempotent: `fmt(fmt(x)) == fmt(x)`. | Re-parse and re-format the output, every style, random widths. |
| The significant token sequence is unchanged (except trailing separators a `Trailing` policy adds or removes). | Compare (kind, text) of every non-trivia token before and after. |
| Every comment's text appears exactly once, in order. | Compare the non-whitespace trivia of the lexed input and output. |
| Line comments end their line. | Every `//` comment in the output is followed by a line break or the end. |
| No trailing spaces at line ends outside error nodes and kept regions. | Check every whitespace token of the re-parsed output. |
| No panic, error nodes included; mismatched trees are errors. | Random token soups, random strings, arbitrary trees, hostile spans. |
| Deterministic. | Formatting twice gives the same output. |
| With no rules, only whitespace changes, as specified. | Compared with a reference implementation over the lexer's flat token list. |

For sources with lexical errors the properties use
[`format_keeping`](#format_keeping) with the lexer's error spans, which is the
documented contract. Trees nested 200,000 levels deep format without
exhausting the stack (`tests/format.rs`).

## Limits

- Whether two tokens may touch is a heuristic unless the style supplies an
  exact test ([`Style::with_touch`](#stylewith_touch)).
- Recoveries that leave no trace in the tree (a parser assuming a missing
  token) cannot be seen: on such sources `Trailing::Always`/`Never` edits are
  made unless the list holds an error node or an unclosed delimited node. The
  properties pass on random invalid input, but a parser could recover in ways
  these checks do not catch. Use `Trailing::Preserve` when that matters.
- In languages whose line breaks are significant tokens, rules that break lines
  would add tokens; use [`Line`/`Hard`](#space) only where the language allows
  a break.
- Indentation is spaces. Widths are counted in `char`s (pretty-lang's measure),
  not terminal columns, so wide characters and tabs count as one.
- A trailing comma "only when the list breaks" needs a break-conditional
  document pretty-lang 1.x lacks (see the ROADMAP).
- Range formatting (only the nodes covering a byte range) is planned for 0.5.
